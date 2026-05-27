"""Sentence-transformers integration for jina-embeddings-v5-omni-small (base + LoRA).

Supports text, image, video, and audio with per-task adapter routing:

    from sentence_transformers import SentenceTransformer
    model = SentenceTransformer(
        "jinaai/jina-embeddings-v5-omni-small",
        trust_remote_code=True,
        model_kwargs={"default_task": "retrieval"},
    )
    q = model.encode("What is ML?", prompt_name="query", task="retrieval")
    d = model.encode("ML is ...", prompt_name="document", task="retrieval")
    img = model.encode(Image.open("photo.jpg"), task="retrieval")
    vid = model.encode("clip.mp4", task="retrieval")
    aud = model.encode("speech.wav", task="retrieval")
"""

import json
import os
from typing import Any, Dict, List, Optional, Union

import torch
import torch.nn.functional as F
from torch import nn
from transformers import AutoConfig, AutoModel, AutoTokenizer

MAX_SEQ_LENGTH = 32768
IMAGE_PROMPT = "<|vision_start|><|image_pad|><|vision_end|>"
VIDEO_PROMPT = "<|vision_start|><|video_pad|><|vision_end|>"
AUDIO_EXTENSIONS = {".wav", ".mp3", ".flac", ".ogg", ".m4a", ".opus", ".webm"}
VIDEO_EXTENSIONS = {".mp4", ".avi", ".mov", ".mkv", ".webm", ".flv", ".wmv"}
PDF_EXTENSIONS = {".pdf"}
SVG_EXTENSIONS = {".svg"}
PDF_DPI = 150
TASK_NAMES = ["retrieval", "text-matching", "clustering", "classification"]
EVAL_IMAGE_MIN_PIXELS = 262144
EVAL_IMAGE_MAX_PIXELS = 1310720
EVAL_VIDEO_MAX_PIXELS = 12845056
EVAL_VIDEO_NUM_FRAMES = 32


def _pil_image():
    """Return the PIL.Image module, with a clean ImportError if pillow is not
    installed. Wrapped in `try` so transformers' AST-based `check_imports`
    does not list PIL as a top-level required dependency: text-only and
    audio-only users should not need pillow installed.
    """
    try:
        from PIL import Image as _PILImage
    except ImportError as e:
        raise ImportError(
            "Encoding images or rasterising PDFs needs `pip install pillow`."
        ) from e
    return _PILImage


def _is_image(x) -> bool:
    try:
        from PIL import Image as PILImage
        return isinstance(x, PILImage.Image)
    except ImportError:
        return False


def _is_video_path(x) -> bool:
    if not isinstance(x, str):
        return False
    return any(x.lower().endswith(ext) for ext in VIDEO_EXTENSIONS)


def _is_audio_path(x) -> bool:
    if not isinstance(x, str):
        return False
    return any(x.lower().endswith(ext) for ext in AUDIO_EXTENSIONS)


def _is_pdf_path(x) -> bool:
    if not isinstance(x, str):
        return False
    return any(x.lower().endswith(ext) for ext in PDF_EXTENSIONS)


def _is_svg_path(x) -> bool:
    if not isinstance(x, str):
        return False
    return any(x.lower().split("?", 1)[0].endswith(ext) for ext in SVG_EXTENSIONS)


def _is_audio_array(x) -> bool:
    try:
        import numpy as np
    except ImportError:
        return False
    return isinstance(x, np.ndarray) and x.ndim == 1 and np.issubdtype(x.dtype, np.floating)


class _AudioWrapper:
    def __init__(self, array, sampling_rate: int = 16000):
        self.array = array
        self.sampling_rate = sampling_rate


def _download_if_url(x):
    """If x is an http(s) URL, download to a hashed local cache and return the
    local path. Otherwise return x unchanged.
    """
    if not isinstance(x, str):
        return x
    if not (x.startswith("http://") or x.startswith("https://")):
        return x
    import hashlib, os, tempfile, urllib.request
    from urllib.parse import urlparse
    cache = os.path.join(tempfile.gettempdir(), "jina_omni_media_cache")
    os.makedirs(cache, exist_ok=True)
    h = hashlib.sha256(x.encode("utf-8")).hexdigest()[:16]
    url_path = urlparse(x).path
    _, ext = os.path.splitext(url_path)
    local = os.path.join(cache, f"{h}{ext}" if ext else h)
    if not os.path.isfile(local) or os.path.getsize(local) == 0:
        urllib.request.urlretrieve(x, local)
    return local


def _looks_like_svg(data):
    if not data:
        return False
    head = data[:4096].lstrip().lower()
    return b"<svg" in head


def _svg_to_image(svg):
    try:
        import cairosvg
    except ImportError as e:
        raise ImportError("Encoding SVG images needs `pip install cairosvg pillow`.") from e
    import io
    png = cairosvg.svg2png(bytestring=svg if isinstance(svg, (bytes, bytearray)) else None,
                           url=svg if isinstance(svg, str) else None)
    _PILImage = _pil_image()
    return _PILImage.open(io.BytesIO(png)).convert("RGB")


def _sniff_media_type_bytes(head):
    """Return 'image'/'svg'/'video'/'audio'/'pdf'/None from content headers."""
    if _looks_like_svg(head):
        return "svg"
    if not head or len(head) < 8:
        return None
    if head[:3] == b"\xff\xd8\xff":                                return "image"
    if head[:8] == b"\x89PNG\r\n\x1a\n":                         return "image"
    if head[:6] in (b"GIF87a", b"GIF89a"):                            return "image"
    if head[:4] == b"RIFF" and head[8:12] == b"WEBP":                 return "image"
    if head[:2] == b"BM":                                             return "image"
    if head[:4] in (b"II*\x00", b"MM\x00*"):                        return "image"
    if head[4:12] in (b"ftypavif", b"ftypavis"):                      return "image"
    if head[4:12] in (b"ftypheic", b"ftypheix", b"ftypmif1", b"ftypmsf1"):
        return "image"
    if head[:3] == b"ID3":                                            return "audio"
    if head[:2] in (b"\xff\xfb", b"\xff\xf3", b"\xff\xf2"):     return "audio"
    if head[:4] == b"fLaC":                                           return "audio"
    if head[:4] == b"OggS":                                           return "audio"
    if head[:4] == b"RIFF" and head[8:12] == b"WAVE":                 return "audio"
    if head[4:12] in (b"M4A ", b"M4B ", b"M4P "):                     return "audio"
    if head[:4] == b"\x1a\x45\xdf\xa3":                           return "video"
    if head[4:8] == b"ftyp":                                          return "video"
    if head[:4] == b"RIFF" and head[8:12] == b"AVI ":                 return "video"
    if head[:3] == b"FLV":                                            return "video"
    if head[:4] == b"0&\xb2u":                                       return "video"
    if head[:5] == b"%PDF-":                                          return "pdf"
    return None


def _sniff_media_type(path):
    try:
        with open(path, "rb") as f:
            data = f.read(4096)
            kind = _sniff_media_type_bytes(data)
            if kind is None and _is_svg_path(path):
                return "svg"
            return kind
    except OSError:
        return None


def _resolve_input(x):
    """Normalize any input to (kind, value). Accepts:
        - PIL.Image                            -> image
        - np.ndarray HxWx3 uint8               -> image (via PIL.fromarray)
        - np.ndarray TxHxWx3 uint8             -> video (saved to /tmp via imageio)
        - np.ndarray 1-D float                 -> audio
        - np.ndarray 2-D float (C,N) or (N,C)  -> audio (mono mixdown)
        - torch.Tensor                         -> converted to numpy, recurse
        - bytes / io.IOBase                    -> sniff + route
        - str URL                              -> downloaded + routed
        - str path                             -> content-sniffed + routed
        - str                                  -> text
    """
    import os as _os
    import io

    if _is_image(x):
        return ("image", x)
    if _is_audio_array(x):
        return ("audio", x)

    try:
        import numpy as _np
    except ImportError:
        _np = None

    if _np is not None and isinstance(x, _np.ndarray):
        # Image (H,W,3|4) uint8
        if x.ndim == 3 and x.shape[-1] in (3, 4) and x.dtype == _np.uint8:
            _PILImage = _pil_image()
            mode = "RGBA" if x.shape[-1] == 4 else "RGB"
            return ("image", _PILImage.fromarray(x, mode).convert("RGB"))
        # Video (T,H,W,3|4) uint8
        if x.ndim == 4 and x.shape[-1] in (3, 4) and x.dtype == _np.uint8:
            # Pass frames straight to the processor — no mp4 round-trip, no
            # av/imageio needed. Drop alpha if present.
            return ("video", x if x.shape[-1] == 3 else x[..., :3])
        # Audio multichannel 2D float -> mono mixdown
        if x.ndim == 2 and _np.issubdtype(x.dtype, _np.floating):
            audio = x.mean(axis=0 if x.shape[0] <= 8 else 1).astype(_np.float32)
            return ("audio", audio)

    # torch.Tensor -> numpy and recurse
    try:
        import torch as _torch
    except ImportError:
        _torch = None
    if _torch is not None and isinstance(x, _torch.Tensor):
        return _resolve_input(x.detach().cpu().numpy())

    # bytes / BytesIO / file-like
    if isinstance(x, (bytes, bytearray)):
        data = bytes(x)
    elif isinstance(x, io.IOBase):
        data = x.read()
    else:
        data = None

    if data is not None:
        kind = _sniff_media_type_bytes(data[:4096])
        if kind == "image":
            _PILImage = _pil_image()
            return ("image", _PILImage.open(io.BytesIO(data)).convert("RGB"))
        if kind == "svg":
            return ("image", _svg_to_image(bytes(data)))
        if kind in ("video", "audio"):
            import tempfile as _tf
            ext = ".mp4" if kind == "video" else ".wav"
            tf = _tf.NamedTemporaryFile(suffix=ext, delete=False)
            tf.write(data); tf.close()
            return (kind, tf.name)
        if kind == "pdf":
            # pypdfium2 reads bytes directly — no temp file needed.
            return ("pdf", bytes(data))

    if isinstance(x, str):
        local = _download_if_url(x)
        if _os.path.isfile(local):
            kind = _sniff_media_type(local)
            if kind == "image":
                _PILImage = _pil_image()
                return ("image", _PILImage.open(local).convert("RGB"))
            if kind == "svg":
                return ("image", _svg_to_image(local))
            if kind in ("video", "audio", "pdf"):
                return (kind, local)
        return ("text", x)

    return ("text", str(x))


def _is_media_string(x) -> bool:
    if not isinstance(x, str):
        return False
    return _resolve_input(x)[0] in ("image", "video", "audio", "pdf")


def _prompt_from_kwargs(st_model, kwargs):
    prompt = kwargs.get("prompt")
    if prompt is None:
        prompt_name = kwargs.get("prompt_name") or getattr(st_model, "default_prompt_name", None)
        prompt = (getattr(st_model, "prompts", {}) or {}).get(prompt_name, "") if prompt_name else ""
    return prompt or ""


def _raw_media_parts(st_model, value, kwargs):
    prompt = _prompt_from_kwargs(st_model, kwargs)
    return (prompt, value) if prompt else (value,)


def _prompted_parts(st_model, value, kwargs):
    parts = value if isinstance(value, tuple) else (value,)
    prompt = _prompt_from_kwargs(st_model, kwargs)
    return (prompt, *parts) if prompt else parts


def _align_eval_processor(processor):
    video_processor = getattr(processor, "video_processor", None)
    if video_processor is None:
        return
    if hasattr(video_processor, "do_sample_frames"):
        video_processor.do_sample_frames = False
    for attr in ("max_frames", "num_frames"):
        if hasattr(video_processor, attr):
            setattr(video_processor, attr, EVAL_VIDEO_NUM_FRAMES)
    if hasattr(video_processor, "size") and isinstance(video_processor.size, dict):
        video_processor.size = {
            **video_processor.size,
            "longest_edge": EVAL_VIDEO_MAX_PIXELS,
            "shortest_edge": EVAL_IMAGE_MIN_PIXELS,
        }
    if hasattr(video_processor, "max_pixels"):
        video_processor.max_pixels = EVAL_VIDEO_MAX_PIXELS
    if hasattr(video_processor, "min_pixels"):
        video_processor.min_pixels = EVAL_IMAGE_MIN_PIXELS


def _build_eval_image_prompt(processor, prefix: str = ""):
    image_token = getattr(processor, "image_token", IMAGE_PROMPT)
    text = f"{prefix or ''}<|vision_start|>{image_token}<|vision_end|>"
    try:
        return processor.apply_chat_template(
            [{"role": "user", "content": text}],
            tokenize=False,
            add_generation_prompt=False,
        )
    except (ValueError, AttributeError):
        return f"{prefix or ''}{IMAGE_PROMPT}"


def _audio_output_length(feature_attention_mask):
    real_frames = feature_attention_mask.sum(-1)
    aftercnn = (real_frames - 1) // 2 + 1
    return int(((aftercnn - 2) // 2 + 1).item())


def _load_audio_array(audio_input):
    import numpy as np

    if isinstance(audio_input, _AudioWrapper):
        return audio_input.array.astype(np.float32), audio_input.sampling_rate
    if isinstance(audio_input, str):
        try:
            import librosa
        except ImportError as e:
            raise ImportError(
                "Loading audio from a file path needs `pip install librosa`"
                " (or pass a 1-D numpy float32 waveform at 16 kHz)."
            ) from e
        audio, sr = librosa.load(audio_input, sr=16000)
        return audio.astype(np.float32), sr
    if isinstance(audio_input, np.ndarray):
        return audio_input.astype(np.float32), 16000
    raise TypeError(f"Unsupported audio input type: {type(audio_input)}")


def _build_audio_model_inputs(owner, audio_input, device, prefix: str = ""):
    import numpy as np
    from transformers import WhisperFeatureExtractor

    audio, sr = _load_audio_array(audio_input)
    if not np.isfinite(audio).all():
        audio = np.nan_to_num(audio, nan=0.0, posinf=0.0, neginf=0.0)
    peak = float(np.max(np.abs(audio))) if audio.size else 0.0
    if peak > 1.0:
        audio = audio / peak

    feat_ext = WhisperFeatureExtractor(feature_size=128)
    audio_inputs = feat_ext(
        audio,
        sampling_rate=sr,
        return_tensors="pt",
        padding="max_length",
        return_attention_mask=True,
    )
    input_features = audio_inputs["input_features"]
    feature_attention_mask = audio_inputs["attention_mask"]
    n_tokens = _audio_output_length(feature_attention_mask)

    start = owner.tokenizer.convert_ids_to_tokens(owner.config.audio_start_token_id)
    token = owner.tokenizer.convert_ids_to_tokens(owner.config.audio_token_id)
    end = owner.tokenizer.convert_ids_to_tokens(owner.config.audio_end_token_id)
    audio_seq = start + token * n_tokens + end
    text = f"{prefix or ''}{audio_seq}"
    try:
        prompt = owner.processor.apply_chat_template(
            [{"role": "user", "content": text}],
            tokenize=False,
            add_generation_prompt=False,
        )
    except (ValueError, AttributeError):
        prompt = text

    out = owner.processor(text=[prompt], return_tensors="pt", padding=False, truncation=False)
    model_dtype = next(owner.model.parameters()).dtype
    inputs = {k: v.to(device) for k, v in out.items() if torch.is_tensor(v)}
    inputs["input_features"] = input_features.to(device=device, dtype=model_dtype)
    inputs["feature_attention_mask"] = feature_attention_mask.to(device)
    pos_builder = globals().get("_get_1d_position_ids")
    if pos_builder is not None:
        inputs["position_ids"] = pos_builder(inputs["attention_mask"])
    return inputs


def _extract_audio_from_video(video_path):
    """Return mono float32 audio @ 16 kHz decoded from the video's audio track, or
    None if no audio stream is present. PyAV is already a dep for video decoding."""
    try:
        import av
        import numpy as np
        from av.audio.resampler import AudioResampler
    except ImportError:
        return None
    container = av.open(video_path)
    try:
        audio_stream = next((s for s in container.streams if s.type == "audio"), None)
        if audio_stream is None:
            return None
        resampler = AudioResampler(format="flt", layout="mono", rate=16000)
        samples = []
        for frame in container.decode(audio=0):
            for rf in resampler.resample(frame):
                samples.append(rf.to_ndarray().flatten())
        for rf in resampler.resample(None):
            samples.append(rf.to_ndarray().flatten())
        if not samples:
            return None
        return np.concatenate(samples).astype(np.float32)
    finally:
        container.close()


def _eval_video_frames(video_path):
    if not isinstance(video_path, str):
        return video_path
    try:
        import av
        import numpy as np
    except ImportError:
        return video_path
    container = av.open(video_path)
    try:
        frames = [frame.to_image().convert("RGB") for frame in container.decode(video=0)]
    finally:
        container.close()
    if not frames:
        return video_path
    if len(frames) <= EVAL_VIDEO_NUM_FRAMES:
        return frames
    indices = np.linspace(0, len(frames) - 1, EVAL_VIDEO_NUM_FRAMES, dtype=int).tolist()
    return [frames[i] for i in indices]


def _pdf_to_images(pdf, dpi: int = PDF_DPI):
    """Rasterise every page of a PDF to a list of PIL.Image (RGB).

    `pdf` may be a path, raw bytes, BytesIO, or an existing list of PIL.Images
    (returned as-is). Lazy-imports `pypdfium2` so users who never touch PDFs
    are not forced to install it.
    """
    _PILImage = _pil_image()  # PIL is a hard dep of the image path
    if isinstance(pdf, list) and pdf and all(isinstance(p, _PILImage.Image) for p in pdf):
        return pdf
    try:
        import pypdfium2 as pdfium
    except ImportError as e:
        raise ImportError(
            "Decoding PDF pages needs `pip install pypdfium2`."
        ) from e
    import io as _io
    if isinstance(pdf, (bytes, bytearray)):
        doc = pdfium.PdfDocument(bytes(pdf))
    elif isinstance(pdf, _io.IOBase):
        doc = pdfium.PdfDocument(pdf.read())
    else:
        doc = pdfium.PdfDocument(pdf)
    scale = dpi / 72.0
    pages = []
    try:
        for page in doc:
            pil = page.render(scale=scale).to_pil().convert("RGB")
            pages.append(pil)
    finally:
        doc.close()
    return pages


def _patch_st_encode_multipart():
    """Intercept ST.encode for multipart tuple inputs so PIL.Image and
    np.ndarray media parts bypass ST's length-sort."""
    import importlib
    import torch
    try:
        st_mod = importlib.import_module("sentence_transformers.SentenceTransformer")
    except ImportError:
        return
    _ST = st_mod.SentenceTransformer
    if getattr(_ST.encode, "_omni_multipart_patched", False):
        return
    _orig = _ST.encode

    def _encode(self, sentences, *args, **kwargs):
        def _is_nonstring_input(x):
            # anything other than a pure string becomes a 1-part multipart item
            return not isinstance(x, str)
        single_bare = _is_nonstring_input(sentences) and not isinstance(sentences, list)
        list_with_nonstr = (isinstance(sentences, list) and sentences
                            and any(_is_nonstring_input(s) for s in sentences))
        single_media_string = isinstance(sentences, str) and _is_media_string(sentences)
        list_with_media_string = (isinstance(sentences, list) and sentences
                                  and any(isinstance(s, str) and _is_media_string(s) for s in sentences))
        fwd_keys = getattr(self[0], "forward_kwargs", set())
        forward_kwargs = {k: kwargs[k] for k in fwd_keys if k in kwargs}
        if single_media_string or list_with_media_string:
            if single_media_string:
                batch = [_raw_media_parts(self, sentences, kwargs)]
            else:
                batch = [_raw_media_parts(self, s, kwargs) for s in sentences]
            features = {"_multipart_batch": batch, "_is_multipart_batch": True}
            with torch.no_grad():
                out = self[0](features, **forward_kwargs)
            emb = out["sentence_embedding"]
            if kwargs.get("convert_to_numpy", True):
                emb = emb.detach().cpu().float().numpy()
            if single_media_string:
                emb = emb[0] if hasattr(emb, "__getitem__") else emb
            return emb
        if single_bare or list_with_nonstr:
            if single_bare:
                batch = [_prompted_parts(self, sentences, kwargs)]
            else:
                batch = [_prompted_parts(self, s, kwargs) for s in sentences]
            features = {"_multipart_batch": batch, "_is_multipart_batch": True}
            with torch.no_grad():
                out = self[0](features, **forward_kwargs)
            emb = out["sentence_embedding"]
            if kwargs.get("convert_to_numpy", True):
                emb = emb.detach().cpu().float().numpy()
            if single_bare:
                emb = emb[0] if hasattr(emb, "__getitem__") else emb
            return emb
        result = _orig(self, sentences, *args, **kwargs)
        # ST 5.x applies truncate_dim without L2 renormalization; the README
        # promises unit-norm truncated embeddings, so restore that here.
        if kwargs.get("truncate_dim") is not None and not kwargs.get("normalize_embeddings", False):
            import numpy as _np
            if torch.is_tensor(result):
                result = torch.nn.functional.normalize(result, p=2, dim=-1)
            elif isinstance(result, _np.ndarray):
                n = _np.linalg.norm(result, axis=-1, keepdims=True) + 1e-12
                result = result / n
        return result

    _encode._omni_multipart_patched = True
    _ST.encode = _encode

    def encode_query(self, sentences, *args, **kwargs):
        kwargs.setdefault("prompt_name", "query")
        return self.encode(sentences, *args, **kwargs)

    def encode_document(self, sentences, *args, **kwargs):
        kwargs.setdefault("prompt_name", "document")
        return self.encode(sentences, *args, **kwargs)

    _ST.encode_query = encode_query
    _ST.encode_document = encode_document


_patch_st_encode_multipart()


class Transformer(nn.Module):
    save_in_root: bool = True
    # Tells sentence-transformers to thread these kwargs from encode() through
    # to our forward() — otherwise ST filters unknown kwargs out.
    forward_kwargs = {"task", "truncate_dim"}

    def __init__(
        self,
        model_name_or_path: str = "jinaai/jina-embeddings-v5-omni-small",
        max_seq_length: Optional[int] = None,
        config_args: Optional[Dict[str, Any]] = None,
        model_args: Optional[Dict[str, Any]] = None,
        tokenizer_args: Optional[Dict[str, Any]] = None,
        cache_dir: Optional[str] = None,
        backend: str = "torch",
        task: Optional[str] = None,
        default_task: Optional[str] = None,
        **kwargs,
    ) -> None:
        super().__init__()
        if backend != "torch":
            raise ValueError(
                f"Backend '{backend}' is not supported, please use 'torch' instead"
            )

        config_kwargs = dict(config_args or {})
        model_kwargs = dict(model_args or {})
        tokenizer_kwargs = dict(tokenizer_args or {})

        # Default-task resolution precedence (highest to lowest):
        #   1. `task` / `default_task` kwarg to this __init__
        #   2. `model_args={'default_task': ...}` (legacy path)
        #   3. JINA_V5_TASK env var
        #   4. unset -> encode() must pass task=
        self.default_task = (
            task
            or default_task
            or model_kwargs.pop("default_task", None)
            or os.environ.get("JINA_V5_TASK")
        )
        if self.default_task and self.default_task not in TASK_NAMES:
            raise ValueError(
                f"Invalid task: {self.default_task}. Must be one of {TASK_NAMES}."
            )

        # setdefault so caller-provided trust_remote_code isn't duplicated
        config_kwargs.setdefault("trust_remote_code", True)
        model_kwargs.setdefault("trust_remote_code", True)
        tokenizer_kwargs.setdefault("trust_remote_code", True)
        # Dedupe cache_dir: we pass it explicitly below, so strip any copy
        # that sentence-transformers may have also threaded through *_args.
        for _kw in (config_kwargs, model_kwargs, tokenizer_kwargs):
            _kw.pop("cache_dir", None)

        self.config = AutoConfig.from_pretrained(
            model_name_or_path, cache_dir=cache_dir, **config_kwargs
        )
        self.model = AutoModel.from_pretrained(
            model_name_or_path, cache_dir=cache_dir, **model_kwargs,
        )
        self.tokenizer = self.model.tokenizer
        # AutoProcessor pulls in PIL transitively; lazy-import so users on
        # text-only setups (no pillow installed) can still load the model.
        try:
            from transformers import AutoProcessor as _AutoProcessor
            processor_kwargs = dict(tokenizer_kwargs)
            processor_kwargs.setdefault("min_pixels", EVAL_IMAGE_MIN_PIXELS)
            processor_kwargs.setdefault("max_pixels", EVAL_IMAGE_MAX_PIXELS)
            self.processor = _AutoProcessor.from_pretrained(
                model_name_or_path, cache_dir=cache_dir, **processor_kwargs,
            )
            _align_eval_processor(self.processor)
        except Exception:
            self.processor = None

        tc = getattr(self.config, "text_config", self.config)
        max_pos = getattr(tc, "max_position_embeddings", MAX_SEQ_LENGTH)
        self.max_seq_length = max_seq_length or min(max_pos, MAX_SEQ_LENGTH)

    def tokenize(
        self,
        texts: Union[List[str], List[Dict], list],
        padding: Union[str, bool] = True,
        **kwargs,
    ) -> Dict[str, torch.Tensor]:
        if texts and any(isinstance(t, tuple) for t in texts):
            # Wrap non-tuple entries as 1-tuples so every batch slot goes
            # through _encode_parts. Lets users mix: [(t,img), "plain text"].
            wrapped = [t if isinstance(t, tuple) else (t,) for t in texts]
            return {"_multipart_batch": wrapped, "_is_multipart_batch": True}
        resolved = [_resolve_input(t) for t in texts]
        # Heterogeneous batch (e.g. ["speech.wav", "plain text"]) — route through
        # the multipart path where each element is dispatched on its own kind.
        if len({k for k, _ in resolved}) > 1:
            wrapped = [t if isinstance(t, tuple) else (t,) for t in texts]
            return {"_multipart_batch": wrapped, "_is_multipart_batch": True}
        first_kind = resolved[0][0]
        values = [v for _, v in resolved]

        if first_kind == "image":
            return {"_images": values, "_is_image_batch": True}
        if first_kind == "video":
            return {"_video_paths": values, "_is_video_batch": True}
        if first_kind == "audio":
            return {"_audio_paths": values, "_is_audio_batch": True}
        if first_kind == "pdf":
            return {"_pdfs": values, "_is_pdf_batch": True}

        if isinstance(texts[0], dict):
            texts = [next(iter(t.values())) for t in texts]
        elif isinstance(texts[0], (list, tuple)):
            texts = [t[0] for t in texts]

        return self.tokenizer(
            [str(s) for s in texts],
            max_length=self.max_seq_length,
            truncation=True,
            padding=padding,
            return_tensors="pt",
        )

    def _resolve_task(self, task: Optional[str]) -> str:
        if task is None:
            if self.default_task is None:
                raise ValueError(
                    "Task must be specified. Set it during loading "
                    "(model_kwargs={'default_task': 'retrieval'}) or pass "
                    "task='retrieval' to encode()."
                )
            task = self.default_task
        if task not in TASK_NAMES:
            raise ValueError(f"Invalid task: {task}. Must be one of {TASK_NAMES}.")
        return task

    def _last_token_pool(self, hidden, attention_mask):
        seq_lens = attention_mask.sum(dim=1) - 1
        pooled = hidden[torch.arange(hidden.shape[0], device=hidden.device), seq_lens]
        return F.normalize(pooled, p=2, dim=-1).float()

    def _encode_single_image(self, image, device, prefix: str = "") -> torch.Tensor:
        prompt = _build_eval_image_prompt(self.processor, prefix=prefix)
        inputs = self.processor(images=image, text=prompt, return_tensors="pt", truncation=False)
        inputs = {k: v.to(device) for k, v in inputs.items() if torch.is_tensor(v)}
        with torch.no_grad():
            hidden = self.model(**inputs).last_hidden_state
        return self._last_token_pool(hidden, inputs["attention_mask"]).squeeze(0)

    def _encode_single_video(self, video_path, device) -> torch.Tensor:
        video = _eval_video_frames(video_path)
        inputs = self.processor(videos=video, text=VIDEO_PROMPT, return_tensors="pt", truncation=False)
        inputs = {k: v.to(device) for k, v in inputs.items() if torch.is_tensor(v)}
        with torch.no_grad():
            hidden = self.model(**inputs).last_hidden_state
        return self._last_token_pool(hidden, inputs["attention_mask"]).squeeze(0)

    def _encode_single_audio(self, audio_input, device, prefix: str = "") -> torch.Tensor:
        inputs = _build_audio_model_inputs(self, audio_input, device, prefix=prefix)
        with torch.no_grad():
            hidden = self.model(**inputs).last_hidden_state
        return self._last_token_pool(hidden, inputs["attention_mask"]).squeeze(0)

    def _encode_single_pdf(self, pdf, device) -> torch.Tensor:
        """Encode a PDF as a fused sequence of page images (single embedding).

        Pages are rasterised with pypdfium2 then fed through the same
        multipart fusion path used for tuples — so a 3-page PDF produces
        a single embedding spanning all three rendered pages.
        """
        pages = _pdf_to_images(pdf)
        if not pages:
            raise ValueError("PDF has 0 pages — nothing to encode.")
        return self._encode_parts(tuple(pages), device)

    def _encode_composite_parts(self, expanded, device) -> torch.Tensor:
        import numpy as np
        from transformers import WhisperFeatureExtractor

        content = []
        images, videos = [], []
        audio_features, feature_masks = [], []
        feat_ext = None
        for kind, p in expanded:
            if kind == "text":
                content.append({"type": "text", "text": str(p)})
            elif kind == "image":
                content.append({"type": "image"})
                images.append(p)
            elif kind == "video":
                content.append({"type": "video"})
                videos.append(_eval_video_frames(p) if isinstance(p, str) else p)
            elif kind == "audio":
                if feat_ext is None:
                    feat_ext = WhisperFeatureExtractor(feature_size=128)
                audio_arr, sr = _load_audio_array(p)
                if not np.isfinite(audio_arr).all():
                    audio_arr = np.nan_to_num(audio_arr, nan=0.0, posinf=0.0, neginf=0.0)
                peak = float(np.max(np.abs(audio_arr))) if audio_arr.size else 0.0
                if peak > 1.0:
                    audio_arr = audio_arr / peak
                audio_inputs = feat_ext(
                    audio_arr,
                    sampling_rate=sr,
                    return_tensors="pt",
                    padding="max_length",
                    return_attention_mask=True,
                )
                feat_mask = audio_inputs["attention_mask"]
                n_tokens = _audio_output_length(feat_mask)
                start = self.tokenizer.convert_ids_to_tokens(self.config.audio_start_token_id)
                token = self.tokenizer.convert_ids_to_tokens(self.config.audio_token_id)
                end = self.tokenizer.convert_ids_to_tokens(self.config.audio_end_token_id)
                content.append({"type": "text", "text": start + token * n_tokens + end})
                audio_features.append(audio_inputs["input_features"])
                feature_masks.append(feat_mask)

        has_chat_template = getattr(self.processor, "chat_template", None) is not None
        if has_chat_template:
            prompt = self.processor.apply_chat_template(
                [{"role": "user", "content": content}],
                tokenize=False,
                add_generation_prompt=False,
            )
            if images or videos:
                image_token = getattr(self.processor, "image_token", "<|image_pad|>")
                video_token = getattr(self.processor, "video_token", "<|video_pad|>")
                flat = []
                for c in content:
                    if c.get("type") == "text":
                        flat.append(c["text"])
                    elif c.get("type") == "image":
                        flat.append(f"<|vision_start|>{image_token}<|vision_end|>")
                    elif c.get("type") == "video":
                        flat.append(f"<|vision_start|>{video_token}<|vision_end|>")
                prompt_flat = self.processor.apply_chat_template(
                    [{"role": "user", "content": "".join(flat)}],
                    tokenize=False,
                    add_generation_prompt=False,
                )
                if "<|vision_start|>" in prompt_flat:
                    prompt = prompt_flat
        else:
            pieces = []
            for c in content:
                if c.get("type") == "text":
                    pieces.append(c["text"])
                elif c.get("type") == "image":
                    pieces.append(IMAGE_PROMPT)
                elif c.get("type") == "video":
                    pieces.append(VIDEO_PROMPT)
            prompt = "".join(pieces)

        proc_kwargs = {"text": [prompt], "return_tensors": "pt", "padding": False, "truncation": False}
        if images:
            proc_kwargs["images"] = images
        if videos:
            proc_kwargs["videos"] = videos
        out = self.processor(**proc_kwargs)
        model_dtype = next(self.model.parameters()).dtype
        inputs = {k: v.to(device) if torch.is_tensor(v) else v for k, v in out.items()}
        if audio_features:
            inputs["input_features"] = torch.cat(audio_features, dim=0).to(device=device, dtype=model_dtype)
            inputs["feature_attention_mask"] = torch.cat(feature_masks, dim=0).to(device)

        if "Qwen" in type(self.processor).__name__:
            ids = inputs["input_ids"].squeeze(0)
            mm_ids = torch.zeros_like(ids, dtype=torch.int32)
            image_token_id = self.processor.tokenizer.convert_tokens_to_ids(getattr(self.processor, "image_token", "<image>"))
            video_token_id = self.processor.tokenizer.convert_tokens_to_ids(getattr(self.processor, "video_token", "<video>"))
            audio_token_id = self.processor.tokenizer.convert_tokens_to_ids(self.tokenizer.convert_ids_to_tokens(self.config.audio_token_id))
            mm_ids += (ids == image_token_id).to(torch.int32)
            mm_ids += 2 * (ids == video_token_id).to(torch.int32)
            mm_ids += 3 * (ids == audio_token_id).to(torch.int32)
            inputs["mm_token_type_ids"] = mm_ids.unsqueeze(0)
            mask = inputs["attention_mask"]
            pos = mask.long().cumsum(-1) - 1
            pos = pos.masked_fill(mask == 0, 0)
            inputs["position_ids"] = pos.unsqueeze(0).expand(3, -1, -1).contiguous()
        else:
            pos_builder = globals().get("_get_1d_position_ids")
            if pos_builder is not None:
                inputs["position_ids"] = pos_builder(inputs["attention_mask"])

        with torch.no_grad():
            hidden = self.model(**inputs).last_hidden_state
        return self._last_token_pool(hidden, inputs["attention_mask"]).squeeze(0)

    def _encode_parts(self, parts, device) -> torch.Tensor:
        """Fuse a tuple of parts into one embedding in a single forward pass.

        Each part may be a URL, a local path (sniffed by magic bytes if no
        extension), a PIL.Image, a 1-D numpy audio array, a PDF (rasterised
        to one image per page), or plain text. A video with an audio track
        is auto-expanded to [extracted_audio, video] so the audio tokens
        precede the video tokens.
        """
        import numpy as np
        from transformers import WhisperFeatureExtractor

        # Normalize every part first (URL -> path, content-sniff if needed).
        resolved = [_resolve_input(p) for p in parts]

        # Expand videos-with-audio: prepend extracted audio.
        # Expand PDFs: rasterise into one image-part per page.
        expanded = []
        for kind, value in resolved:
            if kind == "video":
                if isinstance(value, str):
                    aud = _extract_audio_from_video(value)
                    if aud is not None and aud.size > 0:
                        expanded.append(("audio", aud))
                expanded.append(("video", value))
            elif kind == "pdf":
                for page in _pdf_to_images(value):
                    expanded.append(("image", page))
            else:
                expanded.append((kind, value))

        ids_chunks, mask_chunks = [], []
        pix_images, img_grid = [], []
        pix_videos, vid_grid = [], []
        audio_features = []
        feat_ext = None

        if len(expanded) == 1 and expanded[0][0] == "image":
            return self._encode_single_image(expanded[0][1], device)
        if len(expanded) == 1 and expanded[0][0] == "audio":
            return self._encode_single_audio(expanded[0][1], device)
        if len(expanded) == 2 and expanded[0][0] == "text" and expanded[1][0] == "image":
            return self._encode_single_image(expanded[1][1], device, prefix=str(expanded[0][1]))
        if len(expanded) == 2 and expanded[0][0] == "text" and expanded[1][0] == "audio":
            return self._encode_single_audio(expanded[1][1], device, prefix=str(expanded[0][1]))

        return self._encode_composite_parts(expanded, device)

    def forward(
        self,
        features: Dict[str, torch.Tensor],
        task: Optional[str] = None,
        truncate_dim: Optional[int] = None,
        **kwargs,
    ) -> Dict[str, torch.Tensor]:
        self.model.eval()
        device = next(self.model.parameters()).device
        task = self._resolve_task(task)
        self.model.set_adapter([task])

        if features.get("_is_multipart_batch"):
            embs = [self._encode_parts(parts, device) for parts in features["_multipart_batch"]]
            features["sentence_embedding"] = torch.stack(embs)
            return self._maybe_truncate(features, truncate_dim)

        if features.get("_is_image_batch"):
            embs = [self._encode_single_image(img, device) for img in features["_images"]]
            features["sentence_embedding"] = torch.stack(embs)
            return self._maybe_truncate(features, truncate_dim)

        if features.get("_is_video_batch"):
            embs = [self._encode_single_video(p, device) for p in features["_video_paths"]]
            features["sentence_embedding"] = torch.stack(embs)
            return self._maybe_truncate(features, truncate_dim)

        if features.get("_is_audio_batch"):
            embs = [self._encode_single_audio(p, device) for p in features["_audio_paths"]]
            features["sentence_embedding"] = torch.stack(embs)
            return self._maybe_truncate(features, truncate_dim)

        if features.get("_is_pdf_batch"):
            embs = [self._encode_single_pdf(p, device) for p in features["_pdfs"]]
            features["sentence_embedding"] = torch.stack(embs)
            return self._maybe_truncate(features, truncate_dim)

        batch = {k: v.to(device) for k, v in features.items() if torch.is_tensor(v)}
        with torch.no_grad():
            hidden = self.model(**batch).last_hidden_state

        features["sentence_embedding"] = self._last_token_pool(hidden, batch["attention_mask"])
        return self._maybe_truncate(features, truncate_dim)

    @staticmethod
    def _maybe_truncate(features, truncate_dim):
        if truncate_dim is not None:
            emb = features["sentence_embedding"][..., :truncate_dim]
            features["sentence_embedding"] = F.normalize(emb, p=2, dim=-1)
        return features

    def get_word_embedding_dimension(self) -> int:
        tc = getattr(self.config, "text_config", self.config)
        return getattr(tc, "hidden_size", 1024)

    def get_sentence_embedding_dimension(self) -> int:
        return self.get_word_embedding_dimension()

    def get_max_seq_length(self) -> int:
        return self.max_seq_length

    def save(self, output_path: str, safe_serialization: bool = True, **kwargs) -> None:
        self.model.save_pretrained(output_path, safe_serialization=safe_serialization)
        self.tokenizer.save_pretrained(output_path)
        config = {"max_seq_length": self.max_seq_length}
        with open(os.path.join(output_path, "sentence_bert_config.json"), "w") as f:
            json.dump(config, f, indent=2)

    @classmethod
    def load(cls, input_path: str) -> "Transformer":
        # Signature must have exactly 1 param so ST routes through the direct
        # constructor path (which maps model_kwargs -> model_args correctly).
        config_path = os.path.join(input_path, "sentence_bert_config.json")
        extra = {}
        if os.path.exists(config_path):
            with open(config_path) as f:
                extra = json.load(f)
        return cls(model_name_or_path=input_path, **extra)
