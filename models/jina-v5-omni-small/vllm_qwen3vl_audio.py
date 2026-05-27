"""
vLLM model implementation for Qwen3VLAudioModel (small multimodal embedding model).

Combines:
  - Vision: Qwen3VL vision encoder (from vLLM's qwen3_vl)
  - Audio: Qwen2_5OmniAudioEncoder (from Qwen2.5-Omni-7B) + Linear projector
  - Text: Qwen3 language model (from vLLM's qwen3)

Usage:
    from vllm import ModelRegistry
    from vllm_qwen3vl_audio import Qwen3VLAudioForEmbedding
    ModelRegistry.register_model(
        "Qwen3VLAudioModel",
        Qwen3VLAudioForEmbedding,
    )

    vllm serve /path/to/model --task embedding --trust-remote-code
"""

from collections.abc import Iterable, Mapping, Sequence
from typing import Annotated, Any, Literal, TypeAlias

import torch
import torch.nn as nn
from transformers import BatchFeature
from transformers.models.qwen2_5_omni.configuration_qwen2_5_omni import (
    Qwen2_5OmniAudioEncoderConfig,
)
from transformers.models.qwen2_5_omni.modeling_qwen2_5_omni import Qwen2_5OmniAudioEncoder
from transformers.models.whisper import WhisperFeatureExtractor

from vllm.config import VllmConfig
from vllm.config.multimodal import BaseDummyOptions, VideoDummyOptions
from vllm.inputs import MultiModalDataDict
from vllm.multimodal import MULTIMODAL_REGISTRY
from vllm.multimodal.inputs import (
    AudioItem,
    MultiModalFieldConfig,
    MultiModalKwargsItems,
)
from vllm.multimodal.parse import (
    AudioProcessorItems,
    DictEmbeddingItems,
    ModalityDataItems,
    MultiModalDataItems,
    MultiModalDataParser,
)
from vllm.multimodal.processing import (
    BaseDummyInputsBuilder,
    BaseMultiModalProcessor,
    BaseProcessingInfo,
    PromptReplacement,
    PromptUpdate,
    PromptUpdateDetails,
)
from vllm.sequence import IntermediateTensors
from vllm.utils.tensor_schema import TensorSchema, TensorShape

from vllm.model_executor.models.interfaces import (
    MultiModalEmbeddings,
    SupportsMultiModal,
    SupportsPP,
)
from vllm.model_executor.models.utils import (
    AutoWeightsLoader,
    init_vllm_registered_model,
    maybe_prefix,
)

from vllm.inputs import ModalityData


# Re-use Qwen3VL vision components and processing from vLLM
from vllm.model_executor.models.qwen3_vl import (
    Qwen3_VisionTransformer,
    Qwen3VLProcessingInfo,
    Qwen3VLDummyInputsBuilder,
)
from vllm.model_executor.models.qwen2_vl import (
    Qwen2VLMultiModalDataParser,
)

# --------------------------------------------------------------------------- #
#  Audio input schemas
# --------------------------------------------------------------------------- #


class Qwen3VLAudioFeatureInputs(TensorSchema):
    type: Literal["audio_features"]
    input_features: Annotated[
        torch.Tensor | list[torch.Tensor],
        TensorShape("na", "nmb", 3000),
    ]
    feature_attention_mask: Annotated[
        torch.Tensor,
        TensorShape("na", 3000),
    ]


class Qwen3VLAudioEmbeddingInputs(TensorSchema):
    type: Literal["audio_embeds"] = "audio_embeds"
    audio_embeds: Annotated[
        list[torch.Tensor],
        TensorShape("bn", "naf", "hs", dynamic_dims={"naf"}),
    ]


Qwen3VLAudioInputs: TypeAlias = (
    Qwen3VLAudioFeatureInputs | Qwen3VLAudioEmbeddingInputs
)


def _get_feat_extract_output_lengths(input_lengths: torch.Tensor):
    feat_lengths = (input_lengths - 1) // 2 + 1
    output_lengths = (feat_lengths - 2) // 2 + 1
    return feat_lengths, output_lengths


# --------------------------------------------------------------------------- #
#  Combined multimodal data parser (image + video + audio)
# --------------------------------------------------------------------------- #


class Qwen3VLAudioMultiModalDataParser(Qwen2VLMultiModalDataParser):
    def __init__(self, spatial_merge_size, target_sr, target_channels,
                 expected_hidden_size=None, **kwargs):
        super().__init__(
            spatial_merge_size,
            expected_hidden_size=expected_hidden_size,
            **kwargs,
        )
        self._target_sr = target_sr
        self._target_channels = target_channels

    def _parse_audio_data(
        self,
        data: dict[str, torch.Tensor] | ModalityData[AudioItem],
    ) -> ModalityDataItems[Any, Any] | None:
        if isinstance(data, dict):
            return DictEmbeddingItems(
                data,
                modality="audio",
                required_fields={"audio_embeds"},
                fields_factory=lambda hf: dict(
                    audio_embeds=MultiModalFieldConfig.batched("audio"),
                    input_features=MultiModalFieldConfig.batched("audio"),
                    feature_attention_mask=MultiModalFieldConfig.batched("audio"),
                ),
            )
        return super()._parse_audio_data(data)


# --------------------------------------------------------------------------- #
#  Processing info (combines Qwen3VL vision + Qwen2Audio)
# --------------------------------------------------------------------------- #


class Qwen3VLAudioProcessingInfo(Qwen3VLProcessingInfo):
    def get_hf_config(self):
        # Use the actual class of the already-loaded hf_config to dodge double-
        # import races where modeling_qwen3vl_audio.Qwen3VLAudioConfig has two
        # distinct identities (one via transformers_modules.<...>, one via
        # PYTHONPATH-imported modeling_qwen3vl_audio).
        cfg = self.ctx.model_config.hf_config
        return self.ctx.get_hf_config(type(cfg))

    def get_feature_extractor(self, **kwargs) -> WhisperFeatureExtractor:
        hf_processor = self.get_hf_processor(**kwargs)
        if hasattr(hf_processor, "feature_extractor"):
            feature_extractor = hf_processor.feature_extractor
            assert isinstance(feature_extractor, WhisperFeatureExtractor)
            return feature_extractor
        # The audio tower expects 128 mel channels (matches Qwen2.5-Omni
        # audio config). Whisper's default is 80, which crashes the tower
        # convolution at runtime.
        return WhisperFeatureExtractor(feature_size=128)

    def get_data_parser(self):
        hf_config = self.get_hf_config()
        spatial_merge_size = hf_config.vision_config.get(
            "spatial_merge_size", 2
        ) if isinstance(hf_config.vision_config, dict) else getattr(
            hf_config.vision_config, "spatial_merge_size", 2
        )
        feature_extractor = self.get_feature_extractor()
        return Qwen3VLAudioMultiModalDataParser(
            spatial_merge_size=spatial_merge_size,
            target_sr=feature_extractor.sampling_rate,
            target_channels=1,
            expected_hidden_size=self._get_expected_hidden_size(),
        )

    def get_supported_mm_limits(self) -> Mapping[str, int | None]:
        return {"image": None, "video": None, "audio": None}

    def get_mm_max_tokens_per_item(
        self,
        seq_len: int,
        mm_counts: Mapping[str, int] | None = None,
    ) -> Mapping[str, int]:
        result = super().get_mm_max_tokens_per_item(seq_len, mm_counts)

        mm_counts = mm_counts or {}
        if mm_counts.get("audio", 0) > 0:
            feature_extractor = self.get_feature_extractor()
            chunk_length = min(feature_extractor.chunk_length, 30)
            audio_len = int(chunk_length * feature_extractor.sampling_rate)
            hop_length = feature_extractor.hop_length
            max_mel_seq_len = audio_len // hop_length
            input_lengths = torch.tensor([max_mel_seq_len], dtype=torch.long)
            _, output_lengths = _get_feat_extract_output_lengths(input_lengths)
            result = {**result, "audio": int(output_lengths.item())}

        return result


# --------------------------------------------------------------------------- #
#  Dummy inputs builder
# --------------------------------------------------------------------------- #


class Qwen3VLAudioDummyInputsBuilder(
    Qwen3VLDummyInputsBuilder
):
    def get_dummy_text(self, mm_counts: Mapping[str, int]) -> str:
        text = super().get_dummy_text(mm_counts)
        num_audios = mm_counts.get("audio", 0)
        audio_token = "<|audio_start|><|audio_pad|><|audio_end|>"
        text += audio_token * num_audios
        return text

    def get_dummy_mm_data(
        self,
        seq_len: int,
        mm_counts: Mapping[str, int],
        mm_options: Mapping[str, BaseDummyOptions],
    ) -> MultiModalDataDict:
        result = super().get_dummy_mm_data(seq_len, mm_counts, mm_options)

        num_audios = mm_counts.get("audio", 0)
        if num_audios > 0:
            feature_extractor = self.info.get_feature_extractor()
            sampling_rate = feature_extractor.sampling_rate
            audio_len = feature_extractor.chunk_length * sampling_rate
            result["audio"] = self._get_dummy_audios(
                length=audio_len,
                num_audios=num_audios,
                overrides=mm_options.get("audio"),
            )

        return result


# --------------------------------------------------------------------------- #
#  Multimodal processor
# --------------------------------------------------------------------------- #


def _qwen3vl_audio_field_config(hf_inputs):
    """Field config for Qwen3VL+Audio multimodal kwargs.

    pixel_values{,_videos} and input_features are flat per-token tensors whose
    first dim is the sum across items (each item contributes grid.prod() rows
    for vision, or feat_len/4 rows for audio). The corresponding *_grid_thw /
    feature_attention_mask are per-item. Use flat_from_sizes for the flat
    tensors and batched(keep_on_cpu) for the per-item grids — same pattern as
    vLLM's canonical qwen2_vl._create_qwen2vl_field_factory.
    """
    config = {}
    if "pixel_values" in hf_inputs:
        img_thw = hf_inputs["image_grid_thw"]
        config["pixel_values"] = MultiModalFieldConfig.flat_from_sizes(
            "image", img_thw.prod(-1))
        config["image_grid_thw"] = MultiModalFieldConfig.batched(
            "image", keep_on_cpu=True)
    if "pixel_values_videos" in hf_inputs:
        vid_thw = hf_inputs["video_grid_thw"]
        config["pixel_values_videos"] = MultiModalFieldConfig.flat_from_sizes(
            "video", vid_thw.prod(-1))
        config["video_grid_thw"] = MultiModalFieldConfig.batched(
            "video", keep_on_cpu=True)
    if "input_features" in hf_inputs:
        # feature_attention_mask is per-clip (1 row per audio item).
        # input_features has the per-clip mel frames as inner shape — fed clip-
        # by-clip, batched is correct here (each clip is one row whose 2nd dim
        # is the (channels, frames) mel slab).
        config["input_features"] = MultiModalFieldConfig.batched("audio")
        config["feature_attention_mask"] = MultiModalFieldConfig.batched("audio")
    if "audio_embeds" in hf_inputs:
        config["audio_embeds"] = MultiModalFieldConfig.batched("audio")
    return config


class Qwen3VLAudioMultiModalProcessor(BaseMultiModalProcessor[Qwen3VLAudioProcessingInfo]):
    def _call_hf_processor(
        self,
        prompt: str,
        mm_data: Mapping[str, object],
        mm_kwargs: Mapping[str, Any],
        tok_kwargs: Mapping[str, object],
    ) -> BatchFeature:
        audios = mm_data.pop("audios", []) if isinstance(mm_data, dict) else []
        if not isinstance(mm_data, dict):
            mm_data = dict(mm_data)
        if audios:
            mm_data["audio"] = audios

        has_audio = bool(mm_data.get("audio", []))
        has_images = bool(mm_data.get("images", []))
        has_videos = bool(mm_data.get("videos", []))

        if not has_audio and not has_images and not has_videos:
            prompt_ids = self.info.get_tokenizer().encode(prompt)
            prompt_ids = self._apply_hf_processor_tokens_only(prompt_ids)
            return BatchFeature(dict(input_ids=[prompt_ids]), tensor_type="pt")

        # Audio: pop before delegating to the Qwen3VL parent processor —
        # Qwen3VL doesn't know about audio and silently drops it, which
        # makes vLLM's _merge_mm_kwargs see 1 expected hash but 0 actual
        # mm_kwargs items and crash with IndexError. Run WhisperFE
        # ourselves and stitch the result back into the BatchFeature.
        audio_inputs_to_inject: dict | None = None
        if has_audio:
            import torch as _torch
            feature_extractor = self.info.get_feature_extractor(**mm_kwargs)
            audios_raw = mm_data.pop("audio")
            if not isinstance(audios_raw, list):
                audios_raw = [audios_raw]
            sr = feature_extractor.sampling_rate
            mm_kwargs = dict(**mm_kwargs, sampling_rate=sr)
            audio_outs = feature_extractor(
                audios_raw, sampling_rate=sr, return_tensors="pt",
                return_attention_mask=True,
            )
            # Match HF README path: process the FULL padded input through
            # audio_tower (no masking). The README always emits
            # feat.shape[-1]//4 audio_pad tokens regardless of audio length;
            # to match those embeddings bit-exact we must feed audio_tower
            # the full 3000-frame padded input. Override the per-frame
            # attention mask to all-ones so feature_lens=3000 → 750 outputs.
            input_features = audio_outs["input_features"]
            audio_inputs_to_inject = {
                "input_features": input_features,
                "feature_attention_mask": _torch.ones(
                    input_features.shape[0],
                    input_features.shape[-1],
                    dtype=_torch.long,
                ),
            }

        out = super()._call_hf_processor(
            prompt=prompt,
            mm_data=mm_data,
            mm_kwargs=mm_kwargs,
            tok_kwargs=tok_kwargs,
        )
        if audio_inputs_to_inject is not None:
            for k, v in audio_inputs_to_inject.items():
                out[k] = v
        return out

    def _get_mm_fields_config(
        self,
        hf_inputs: BatchFeature,
        hf_processor_mm_kwargs: Mapping[str, object],
    ) -> Mapping[str, MultiModalFieldConfig]:
        return _qwen3vl_audio_field_config(hf_inputs)

    def _get_prompt_updates(
        self,
        mm_items: MultiModalDataItems,
        hf_processor_mm_kwargs: Mapping[str, object],
        out_mm_kwargs: MultiModalKwargsItems,
    ) -> Sequence[PromptUpdate]:
        updates = []
        hf_config = self.info.get_hf_config()

        image_token_id = getattr(hf_config, "image_token_id", None)
        video_token_id = getattr(hf_config, "video_token_id", None)
        audio_token_id = getattr(hf_config, "audio_token_id", None)

        # Spatial merge factor: Qwen3VL collapses 2x2 patches per merge token.
        merge_size = getattr(
            getattr(hf_config, "vision_config", hf_config), "spatial_merge_size", 2
        )

        out_mm_data = out_mm_kwargs.get_data()
        image_grid_thw = out_mm_data.get("image_grid_thw")
        video_grid_thw = out_mm_data.get("video_grid_thw")

        if image_token_id is not None:
            def get_image_replacement(item_idx: int):
                if image_grid_thw is None:
                    return [image_token_id]
                n = int(image_grid_thw[item_idx].prod().item()) // (merge_size ** 2)
                return [image_token_id] * max(n, 1)

            updates.append(
                PromptReplacement(
                    modality="image",
                    target=[image_token_id],
                    replacement=get_image_replacement,
                )
            )
        if video_token_id is not None:
            # HF's Qwen3VLProcessor expands `<|vision_start|><|video_pad|><|vision_end|>`
            # to T per-frame blocks of `<{t:.1f} seconds><|vision_start|><|video_pad|>×frame_seqlen<|vision_end|>`.
            # Returning a flat `[video_token_id]*N` instead drops the inner
            # vision_start/end pairs and timestamp text, costing ~5 cos points
            # on video parity. Mirror HF's expansion here.
            tokenizer = self.info.get_tokenizer()
            vision_cfg = getattr(hf_config, "vision_config", hf_config)
            temporal_patch_size = (
                vision_cfg["temporal_patch_size"] if isinstance(vision_cfg, dict)
                else getattr(vision_cfg, "temporal_patch_size", 2)
            )
            vision_start_token_id = getattr(hf_config, "vision_start_token_id", 151652)
            vision_end_token_id = getattr(hf_config, "vision_end_token_id", 151653)
            # HF defaults `metadata.fps = 24` when raw frames are passed without
            # metadata (see processing_qwen3_vl.py:140-145).
            default_fps = 24

            def get_video_replacement(item_idx: int):
                if video_grid_thw is None:
                    return [video_token_id]
                grid_thw = video_grid_thw[item_idx]
                T = int(grid_thw[0])
                frame_seqlen = (int(grid_thw[1]) * int(grid_thw[2])) // (merge_size ** 2)
                # Replicate Qwen3VLProcessingInfo._calculate_timestamps for the
                # path-A (raw frames, no metadata) case: indices=range(N),
                # fps = default_fps, pair-averaged by temporal_patch_size.
                N = T * temporal_patch_size
                per_idx = [i / float(default_fps) for i in range(N)]
                timestamps = [
                    (per_idx[i] + per_idx[i + temporal_patch_size - 1]) / 2
                    for i in range(0, N, temporal_patch_size)
                ]

                out: list[int] = []
                for t in range(T):
                    ts_ids = tokenizer.encode(
                        f"<{timestamps[t]:.1f} seconds>",
                        add_special_tokens=False,
                    )
                    out.extend(ts_ids)
                    out.append(vision_start_token_id)
                    out.extend([video_token_id] * max(frame_seqlen, 1))
                    out.append(vision_end_token_id)
                # select_token_id marks only video_token_id positions as
                # embedding placeholders; timestamps + vision_start/end keep
                # their text embeddings. Without this vLLM tries to gather N
                # visual embeddings into the full T*(timestamp+2+frame_seqlen)
                # slot count → CUDA device-side assert at pool time.
                return PromptUpdateDetails.select_token_id(out, video_token_id)

            updates.append(
                PromptReplacement(
                    modality="video",
                    # String target absorbs the outer vision_start/end since
                    # each frame block emits its own pair.
                    target="<|vision_start|><|video_pad|><|vision_end|>",
                    replacement=get_video_replacement,
                )
            )

        if audio_token_id is not None:
            audio_out_mm_data = out_mm_kwargs.get_data()
            feature_attention_mask = audio_out_mm_data.get("feature_attention_mask")
            audio_embeds = audio_out_mm_data.get("audio_embeds")
            # _call_hf_processor overrides feature_attention_mask to all-ones,
            # so this resolves to (full_padded_frames // 4) — matching the
            # 750 audio_pad tokens the HF README emits per audio item.
            if feature_attention_mask is not None:
                _, audio_output_lens = _get_feat_extract_output_lengths(
                    feature_attention_mask.sum(-1)
                )
                audio_output_lengths = audio_output_lens.tolist()
            elif audio_embeds is not None:
                audio_output_lengths = [e.shape[0] for e in audio_embeds]
            else:
                audio_output_lengths = []

            # Only register the audio replacement when we have audio data;
            # registering it during text/image-only runs causes vLLM to expect
            # audio placeholders that don't exist in the prompt.
            if audio_output_lengths:
                def get_audio_replacement(item_idx: int):
                    n = audio_output_lengths[item_idx]
                    return [audio_token_id] * max(n, 1)

                updates.append(
                    PromptReplacement(
                        modality="audio",
                        target=[audio_token_id],
                        replacement=get_audio_replacement,
                    )
                )

        return updates


# --------------------------------------------------------------------------- #
#  Audio projector
# --------------------------------------------------------------------------- #


class AudioMultiModalProjector(nn.Module):
    def __init__(self, audio_hidden_size: int, text_hidden_size: int):
        super().__init__()
        self.linear = nn.Linear(audio_hidden_size, text_hidden_size, bias=True)

    def forward(self, audio_features):
        return self.linear(audio_features)


def _remap_language_model_weights(
    weights: Iterable[tuple[str, torch.Tensor]],
) -> Iterable[tuple[str, torch.Tensor]]:
    for name, tensor in weights:
        if name.startswith("language_model.") and not name.startswith(
            "language_model.model."
        ) and not name.startswith("language_model.lm_head."):
            name = "language_model.model." + name[len("language_model."):]
        if name == "audio_projector.weight":
            name = "audio_projector.linear.weight"
        elif name == "audio_projector.bias":
            name = "audio_projector.linear.bias"
        yield name, tensor


# --------------------------------------------------------------------------- #
#  Model
# --------------------------------------------------------------------------- #


@MULTIMODAL_REGISTRY.register_processor(
    Qwen3VLAudioMultiModalProcessor,
    info=Qwen3VLAudioProcessingInfo,
    dummy_inputs=Qwen3VLAudioDummyInputsBuilder,
)
class Qwen3VLAudioForEmbedding(nn.Module, SupportsMultiModal, SupportsPP):
    """vLLM model for Qwen3VLAudioModel (small multimodal embedding)."""

    @classmethod
    def get_placeholder_str(cls, modality: str, i: int) -> str | None:
        if modality == "image":
            return f"<|vision_start|><|image_pad|><|vision_end|>"
        if modality == "video":
            return f"<|vision_start|><|video_pad|><|vision_end|>"
        if modality.startswith("audio"):
            return f"Audio {i}: <|audio_start|><|audio_pad|><|audio_end|>"
        return None

    def __init__(self, *, vllm_config: VllmConfig, prefix: str = ""):
        super().__init__()
        config = vllm_config.model_config.hf_config
        self.config = config
        quant_config = vllm_config.quant_config

        vis_cfg = config.vision_config
        if isinstance(vis_cfg, dict):
            from transformers.models.qwen3_vl.configuration_qwen3_vl import (
                Qwen3VLVisionConfig,
            )
            vis_cfg = Qwen3VLVisionConfig(**vis_cfg)

        aud_cfg = config.audio_config
        if isinstance(aud_cfg, dict):
            aud_cfg = Qwen2_5OmniAudioEncoderConfig(**aud_cfg)

        txt_cfg = config.text_config

        with self._mark_tower_model(vllm_config, {"image", "video"}):
            self.visual = Qwen3_VisionTransformer(
                vis_cfg,
                quant_config=quant_config,
                prefix=maybe_prefix(prefix, "visual"),
            )

        with self._mark_tower_model(vllm_config, "audio"):
            self.audio_tower = Qwen2_5OmniAudioEncoder(aud_cfg)
            # Match modeling_qwen3vl_audio.py: this checkpoint never trains the
            # internal proj layer; audio→text projection lives in audio_projector.
            self.audio_tower.proj = nn.Identity()
            # Audio projector input is d_model (encoder hidden), not output_dim
            # (which is the upstream Qwen2.5-Omni-7B text-hidden, unused here).
            output_dim = aud_cfg.d_model
            hidden_size = (
                txt_cfg["hidden_size"] if isinstance(txt_cfg, dict) else txt_cfg.hidden_size
            )
            self.audio_projector = AudioMultiModalProjector(output_dim, hidden_size)

        with self._mark_language_model(vllm_config):
            self.language_model = init_vllm_registered_model(
                vllm_config=vllm_config,
                hf_config=txt_cfg,
                prefix=maybe_prefix(prefix, "language_model"),
                architectures=["Qwen3ForCausalLM"],
            )

        self.make_empty_intermediate_tensors = (
            self.language_model.make_empty_intermediate_tensors
        )

    # ---- multimodal processing ---- #

    def _parse_and_validate_audio_input(
        self, **kwargs: object
    ) -> Qwen3VLAudioInputs | None:
        input_features = kwargs.pop("input_features", None)
        audio_embeds = kwargs.pop("audio_embeds", None)
        feature_attention_mask = kwargs.pop("feature_attention_mask", None)

        if input_features is None and audio_embeds is None:
            return None

        if audio_embeds is not None:
            return Qwen3VLAudioEmbeddingInputs(
                type="audio_embeds", audio_embeds=audio_embeds
            )

        return Qwen3VLAudioFeatureInputs(
            type="audio_features",
            input_features=input_features,
            feature_attention_mask=feature_attention_mask,
        )

    def _process_audio_input(
        self, audio_input: Qwen3VLAudioInputs
    ) -> torch.Tensor | tuple[torch.Tensor, ...]:
        if audio_input["type"] == "audio_embeds":
            return tuple(audio_input["audio_embeds"])

        input_features = audio_input["input_features"]
        feature_attention_mask = audio_input["feature_attention_mask"]

        feature_lens = feature_attention_mask.sum(-1).long()
        aftercnn_lens, output_lengths = (
            self.audio_tower._get_feat_extract_output_lengths(feature_lens)
        )

        packed = input_features.permute(0, 2, 1)[feature_attention_mask.bool()].permute(1, 0)

        audio_outputs = self.audio_tower(
            packed, feature_lens=feature_lens, aftercnn_lens=aftercnn_lens,
        )
        audio_features = self.audio_projector(audio_outputs.last_hidden_state)

        return torch.split(audio_features, output_lengths.tolist())

    def _process_image_input(self, pixel_values, image_grid_thw):
        pixel_values = pixel_values.to(
            dtype=self.visual.dtype, device=self.visual.device
        )
        image_embeds = self.visual(pixel_values, grid_thw=image_grid_thw)
        merge_sq = self.visual.spatial_merge_size ** 2
        if isinstance(image_grid_thw, list):
            split_sizes = [
                int(t * h * w) // merge_sq for t, h, w in image_grid_thw
            ]
        else:
            split_sizes = (image_grid_thw.prod(-1) // merge_sq).tolist()
        return list(torch.split(image_embeds, split_sizes))

    def embed_multimodal(self, **kwargs: object) -> MultiModalEmbeddings:
        embeddings: list[torch.Tensor] = []

        pixel_values = kwargs.pop("pixel_values", None)
        image_grid_thw = kwargs.pop("image_grid_thw", None)
        if pixel_values is not None and image_grid_thw is not None:
            embeddings.extend(
                self._process_image_input(pixel_values, image_grid_thw)
            )

        pixel_values_videos = kwargs.pop("pixel_values_videos", None)
        video_grid_thw = kwargs.pop("video_grid_thw", None)
        if pixel_values_videos is not None and video_grid_thw is not None:
            embeddings.extend(
                self._process_image_input(pixel_values_videos, video_grid_thw)
            )

        audio_input = self._parse_and_validate_audio_input(**kwargs)
        if audio_input is not None:
            audio_embeds = self._process_audio_input(audio_input)
            if isinstance(audio_embeds, tuple):
                embeddings.extend(audio_embeds)
            else:
                embeddings.append(audio_embeds)

        return embeddings if embeddings else []

    # ---- forward ---- #

    def forward(
        self,
        input_ids: torch.Tensor | None,
        positions: torch.Tensor,
        intermediate_tensors: IntermediateTensors | None = None,
        inputs_embeds: torch.Tensor | None = None,
        **kwargs: object,
    ) -> torch.Tensor | IntermediateTensors:
        if intermediate_tensors is not None:
            inputs_embeds = None

        hidden_states = self.language_model.model(
            input_ids, positions, intermediate_tensors, inputs_embeds=inputs_embeds
        )
        return hidden_states

    def compute_logits(self, hidden_states: torch.Tensor) -> torch.Tensor | None:
        return self.language_model.compute_logits(hidden_states)

    # ---- weight loading ---- #

    def load_weights(self, weights: Iterable[tuple[str, torch.Tensor]]) -> set[str]:
        loader = AutoWeightsLoader(self)
        return loader.load_weights(_remap_language_model_weights(weights))


# ─────────────────────────────────────────────────────────────────────────── #
#  Self-registration with vLLM's ModelRegistry on import.
#
#  This file is shipped inside the HF model repo. It is auto-imported through
#  modeling_qwen3vl_audio.py (which imports us from a side-effect block) when
#  trust_remote_code=True is passed to vLLM. By calling register_model at
#  module load time we make sure both the parent process and any vLLM worker
#  subprocess that re-imports this module find the architecture without
#  requiring a separately-installed plugin.
# ─────────────────────────────────────────────────────────────────────────── #
def _register_with_vllm() -> None:
    try:
        from vllm import ModelRegistry
    except ImportError:
        return
    # Register with the actual *class* (not a "<module>:<class>" string) so
    # vLLM uses _RegisteredModel rather than _LazyRegisteredModel. The lazy
    # path runs `inspect_model_cls()` via `python3 -m vllm.model_executor.
    # models.registry` in a fresh subprocess, which then does
    # `importlib.import_module("vllm_qwen3vl_audio")` — and that fails when
    # the module lives inside the HF transformers_modules cache (or anywhere
    # else not on the bare PYTHONPATH of a clean subprocess). The class-based
    # registration short-circuits the inspection: vLLM serializes the class
    # itself and never spawns a subprocess.
    #
    # We also keep the directory on sys.path so any other code path that
    # later does a string-based re-registration (e.g. the test harness)
    # still resolves correctly.
    import os, sys
    here = os.path.dirname(os.path.abspath(__file__))
    if here not in sys.path:
        sys.path.insert(0, here)
    ModelRegistry.register_model(
        "Qwen3VLAudioModel",
        Qwen3VLAudioForEmbedding,
    )

    # Force vLLM to use our registered architecture even when its
    # auto-resolution path returned `TransformersMultiModalEmbeddingModel`.
    #
    # vLLM 0.19's flow for an unknown arch is:
    #   1. `_normalize_arch("Qwen3VLAudioModel")` → not registered → no info
    #   2. fall through to `_try_resolve_transformers` which:
    #      a. loads the `auto_map` modeling file (this triggers
    #         `_register_vllm()` in the modeling file, which imports THIS
    #         module — registering `Qwen3VLAudioModel` and applying these
    #         monkey-patches), then
    #      b. ignores the registry and returns the generic backend cls
    #         name (`TransformersMultiModalEmbeddingModel`) regardless.
    #   3. vLLM does `_try_load_model_cls("TransformersMultiModalEmbeddingModel")`.
    #      This is the FIRST call that fires AFTER our patch is in place.
    #
    # We patch `_try_load_model_cls` to redirect the generic-backend lookup
    # to our registered custom class when it's available. The override is
    # bracketed by an attribute flag so concurrent loads of multiple
    # variants only patch once.
    try:
        from vllm.model_executor.models.registry import _ModelRegistry
        if not getattr(_ModelRegistry, "_jinaai_v5_omni_resolve_patch", False):
            _orig_load = _ModelRegistry._try_load_model_cls
            _orig_inspect = _ModelRegistry._try_inspect_model_cls

            def _redirect_arch(self, arch):
                """Redirect generic backend arch to our custom arch when
                BOTH the generic and `Qwen3VLAudioModel` are registered —
                meaning we ran our trust_remote_code side-effect register
                before this call. The generic name still must point at
                vLLM's built-in transformers backend, so we only redirect
                for our specific architecture."""
                if (
                    arch == "TransformersMultiModalEmbeddingModel"
                    and "Qwen3VLAudioModel" in self.models
                ):
                    return "Qwen3VLAudioModel"
                return arch

            def _patched_load(self, model_arch):
                return _orig_load(self, _redirect_arch(self, model_arch))

            def _patched_inspect(self, model_arch):
                return _orig_inspect(self, _redirect_arch(self, model_arch))

            _ModelRegistry._try_load_model_cls = _patched_load
            _ModelRegistry._try_inspect_model_cls = _patched_inspect
            _ModelRegistry._jinaai_v5_omni_resolve_patch = True
    except Exception:
        pass

    # Expose this module under a stable top-level name so end users can do
    # `import jina_v5_omni; jina_v5_omni.format_prompt(...)` without having to
    # spell out the cache path inside `transformers_modules.jinaai....`.
    sys.modules.setdefault("jina_v5_omni", sys.modules[__name__])


_register_with_vllm()


_IMAGE_TOKEN = "<|image_pad|>"
_IMAGE_PLACEHOLDER = "<|vision_start|><|image_pad|><|vision_end|>"
_VIDEO_PLACEHOLDER = "<|vision_start|><|video_pad|><|vision_end|>"
_AUDIO_PLACEHOLDER = "<|audio_start|><|audio_pad|><|audio_end|>"


def _image_chat_prompt(text: str = "") -> str:
    return f"<|im_start|>user\n<|vision_start|>{_IMAGE_TOKEN}<|vision_end|>{text}<|im_end|>\n"


def format_prompt(text: str = "", image=None, video=None, audio=None) -> dict:
    """Build a `llm.embed(...)` request dict for jina-embeddings-v5-omni-small-*.

    Inserts the model's vision/audio placeholder tokens for you so callers
    don't need to spell them out.

    Example::

        from vllm import LLM
        from PIL import Image
        llm = LLM(model="jinaai/jina-embeddings-v5-omni-small-retrieval",
                  trust_remote_code=True, runner="pooling", dtype="bfloat16")
        import jina_v5_omni
        req = jina_v5_omni.format_prompt(text="A blue square.",
                                         image=Image.open("foo.png"))
        out = llm.embed([req])[0].outputs.embedding
    """
    if image is not None and video is None and audio is None:
        return {"prompt": _image_chat_prompt(text), "multi_modal_data": {"image": image}}

    parts: list[str] = []
    mm: dict = {}
    if image is not None:
        parts.append(_IMAGE_PLACEHOLDER)
        mm["image"] = image
    if video is not None:
        parts.append(_VIDEO_PLACEHOLDER)
        mm["video"] = video
    if audio is not None:
        parts.append(_AUDIO_PLACEHOLDER)
        mm["audio"] = audio
    req: dict = {"prompt": "".join(parts) + text}
    if mm:
        req["multi_modal_data"] = mm
    return req
