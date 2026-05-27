"""
vLLM model implementation for LlavaEuroBertAudioForEmbedding (nano multimodal embedding).

Combines:
  - Vision: Qwen3VL vision encoder + PretrainedMerger
  - Audio: Qwen2_5OmniAudioEncoder (from Qwen2.5-Omni-7B) + Linear projector
  - Text: LlamaModel / EuroBERT (bidirectional)

Usage:
    from vllm import ModelRegistry
    ModelRegistry.register_model(
        "LlavaEuroBertAudioForEmbedding",
        "vllm_llava_eurobert_audio:LlavaEuroBertAudioForVLLMEmbedding",
    )

    vllm serve /path/to/model --task embedding --trust-remote-code
"""

import os
from collections.abc import Iterable, Mapping, Sequence
from pathlib import Path
from typing import Annotated, Any, Literal, TypeAlias

import numpy as np
import torch
import torch.nn as nn
from transformers import BatchFeature
from transformers.models.qwen2_5_omni.configuration_qwen2_5_omni import (
    Qwen2_5OmniAudioEncoderConfig,
)
from transformers.models.qwen2_5_omni.modeling_qwen2_5_omni import Qwen2_5OmniAudioEncoder
from transformers.models.qwen3_vl.configuration_qwen3_vl import Qwen3VLVisionConfig
from transformers.models.qwen3_vl.modeling_qwen3_vl import Qwen3VLVisionModel
from transformers.models.whisper import WhisperFeatureExtractor

from vllm.config import VllmConfig
try:
    # vllm >= 0.11
    from vllm.config.multimodal import BaseDummyOptions
except ImportError:
    # vllm < 0.11 — BaseDummyOptions didn't exist; use a lightweight stand-in
    # so the signature annotation still parses.
    class BaseDummyOptions:  # type: ignore[no-redef]
        pass
try:
    # vllm < 0.11: types re-exported via vllm.inputs
    from vllm.inputs import MultiModalDataDict, ModalityData
except ImportError:
    # vllm >= 0.11: moved to vllm.multimodal.inputs
    from vllm.multimodal.inputs import MultiModalDataDict, ModalityData
from vllm.multimodal import MULTIMODAL_REGISTRY
from vllm.multimodal.inputs import (
    AudioItem,
    MultiModalFieldConfig,
    MultiModalKwargsItems,
)
from vllm.multimodal.parse import (
    DictEmbeddingItems,
    ModalityDataItems,
    MultiModalDataItems,
    MultiModalDataParser,
)
try:
    # vllm < 0.11
    from vllm.multimodal.processing import BaseDummyInputsBuilder
except ImportError:
    # vllm >= 0.11: moved to vllm.multimodal.profiling
    from vllm.multimodal.profiling import BaseDummyInputsBuilder
from vllm.multimodal.processing import (
    BaseMultiModalProcessor,
    BaseProcessingInfo,
    PromptReplacement,
    PromptUpdate,
)
from vllm.sequence import IntermediateTensors
from vllm.utils.tensor_schema import TensorSchema, TensorShape
from vllm.model_executor.models.interfaces import (
    MultiModalEmbeddings,
    SupportsMultiModal,
    SupportsPP,
)
from vllm.model_executor.models.qwen2_vl import _create_qwen2vl_field_factory
from vllm.model_executor.models.utils import (
    AutoWeightsLoader,
    init_vllm_registered_model,
    maybe_prefix,
)


# --------------------------------------------------------------------------- #
#  PretrainedMerger (same architecture as HuggingFace version)
# --------------------------------------------------------------------------- #


class PretrainedMerger(nn.Module):
    def __init__(self, hidden_size, out_hidden_size, spatial_merge_size=2):
        super().__init__()
        self.hidden_size = hidden_size * (spatial_merge_size ** 2)
        self.norm = nn.LayerNorm(hidden_size, eps=1e-6)
        self.linear_fc1 = nn.Linear(self.hidden_size, self.hidden_size)
        self.act = nn.GELU()
        self.linear_fc2 = nn.Linear(self.hidden_size, out_hidden_size)

    def forward(self, x):
        x = self.norm(x)
        x = x.view(-1, self.hidden_size)
        x = self.linear_fc2(self.act(self.linear_fc1(x)))
        return x


# --------------------------------------------------------------------------- #
#  Audio input schemas
# --------------------------------------------------------------------------- #


class NanoAudioFeatureInputs(TensorSchema):
    type: Literal["audio_features"]
    input_features: Annotated[
        torch.Tensor | list[torch.Tensor],
        TensorShape("na", "nmb", 3000),
    ]
    feature_attention_mask: Annotated[
        torch.Tensor,
        TensorShape("na", 3000),
    ]


class NanoAudioEmbeddingInputs(TensorSchema):
    type: Literal["audio_embeds"] = "audio_embeds"
    audio_embeds: Annotated[
        list[torch.Tensor],
        TensorShape("bn", "naf", "hs", dynamic_dims={"naf"}),
    ]


NanoAudioInputs: TypeAlias = NanoAudioFeatureInputs | NanoAudioEmbeddingInputs


def _get_feat_extract_output_lengths(input_lengths: torch.Tensor):
    feat_lengths = (input_lengths - 1) // 2 + 1
    output_lengths = (feat_lengths - 2) // 2 + 1
    return feat_lengths, output_lengths


# --------------------------------------------------------------------------- #
#  Processing info
# --------------------------------------------------------------------------- #


class NanoMMAudioMultiModalDataParser(MultiModalDataParser):
    def __init__(self, target_sr, target_channels, expected_hidden_size=None):
        super().__init__(
            target_sr=target_sr,
            target_channels=target_channels,
            expected_hidden_size=expected_hidden_size,
        )

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


class NanoMMProcessingInfo(BaseProcessingInfo):
    def get_hf_config(self):
        return self.ctx.get_hf_config()

    def get_feature_extractor(self, **kwargs) -> WhisperFeatureExtractor:
        return WhisperFeatureExtractor(feature_size=128)

    def get_data_parser(self):
        feature_extractor = self.get_feature_extractor()
        return NanoMMAudioMultiModalDataParser(
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
        result = {}
        mm_counts = mm_counts or {}

        hf_config = self.get_hf_config()
        vis_cfg = hf_config.vision_config
        if isinstance(vis_cfg, dict):
            spatial_merge_size = vis_cfg.get("spatial_merge_size", 2)
        else:
            spatial_merge_size = getattr(vis_cfg, "spatial_merge_size", 2)

        # Always return per-item max for all modalities — vLLM calls this
        # during profiling with empty mm_counts; a missing key is treated as 0
        # which causes "At most 0 video(s) may be provided" errors.
        result["image"] = 256 // (spatial_merge_size ** 2)

        # 32-frame videos at typical resolution produce ~7040 tokens
        # (measured: 16 frames → 3520 tokens with spatial_merge_size=1).
        # Cap at 64 frames worth to handle evaluation edge cases.
        result["video"] = 64 * 256 // max(spatial_merge_size ** 2, 1)

        feature_extractor = self.get_feature_extractor()
        chunk_length = min(feature_extractor.chunk_length, 30)
        audio_len = int(chunk_length * feature_extractor.sampling_rate)
        hop_length = feature_extractor.hop_length
        max_mel_seq_len = audio_len // hop_length
        input_lengths = torch.tensor([max_mel_seq_len], dtype=torch.long)
        _, output_lengths = _get_feat_extract_output_lengths(input_lengths)
        result["audio"] = int(output_lengths.item())

        return result


# --------------------------------------------------------------------------- #
#  Dummy inputs builder
# --------------------------------------------------------------------------- #


class NanoMMDummyInputsBuilder(BaseDummyInputsBuilder[NanoMMProcessingInfo]):
    def get_dummy_text(self, mm_counts: Mapping[str, int]) -> str:
        text = ""
        num_images = mm_counts.get("image", 0)
        num_videos = mm_counts.get("video", 0)
        num_audios = mm_counts.get("audio", 0)

        image_token = "<image>"
        video_token = "<image>"
        audio_token = "<|audio_bos|><|AUDIO|><|audio_eos|>"

        text += image_token * num_images
        text += video_token * num_videos
        text += audio_token * num_audios
        return text

    def get_dummy_mm_data(
        self,
        seq_len: int,
        mm_counts: Mapping[str, int],
        mm_options: Mapping[str, BaseDummyOptions],
    ) -> MultiModalDataDict:
        result: dict[str, Any] = {}

        num_images = mm_counts.get("image", 0)
        if num_images > 0:
            result["image"] = self._get_dummy_images(
                width=224, height=224, num_images=num_images,
                overrides=mm_options.get("image"),
            )

        num_videos = mm_counts.get("video", 0)
        if num_videos > 0:
            result["video"] = self._get_dummy_videos(
                width=224, height=224, num_frames=2, num_videos=num_videos,
                overrides=mm_options.get("video"),
            )

        num_audios = mm_counts.get("audio", 0)
        if num_audios > 0:
            feature_extractor = self.info.get_feature_extractor()
            sampling_rate = feature_extractor.sampling_rate
            audio_len = feature_extractor.chunk_length * sampling_rate
            result["audio"] = self._get_dummy_audios(
                length=audio_len, num_audios=num_audios,
                overrides=mm_options.get("audio"),
            )

        return result


# --------------------------------------------------------------------------- #
#  Multimodal processor
# --------------------------------------------------------------------------- #


class NanoMMMultiModalProcessor(BaseMultiModalProcessor[NanoMMProcessingInfo]):
    def _call_hf_processor(
        self,
        prompt: str,
        mm_data: Mapping[str, object],
        mm_kwargs: Mapping[str, Any],
        tok_kwargs: Mapping[str, object],
    ) -> BatchFeature:
        if not isinstance(mm_data, dict):
            mm_data = dict(mm_data)
        audios = mm_data.pop("audios", [])
        if audios:
            mm_data["audio"] = audios

        has_audio = bool(mm_data.get("audio", []))
        has_images = bool(mm_data.get("images", []))
        has_videos = bool(mm_data.get("videos", []))

        if not has_audio and not has_images and not has_videos:
            prompt_ids = self.info.get_tokenizer().encode(prompt)
            prompt_ids = self._apply_hf_processor_tokens_only(prompt_ids)
            return BatchFeature(dict(input_ids=[prompt_ids]), tensor_type="pt")

        if has_audio and not has_images and not has_videos:
            feature_extractor = self.info.get_feature_extractor(**mm_kwargs)
            tokenizer = self.info.get_tokenizer()

            audio_items = mm_data.get("audio", [])
            if not isinstance(audio_items, list):
                audio_items = [audio_items]

            def _to_audio_array(item: object) -> np.ndarray:
                if hasattr(item, "data"):
                    item = item.data
                if isinstance(item, tuple) and len(item) >= 1:
                    item = item[0]
                if isinstance(item, dict):
                    for key in ("array", "audio", "data", "samples"):
                        if key in item:
                            item = item[key]
                            break
                if hasattr(item, "array"):
                    item = item.array
                if hasattr(item, "audio"):
                    item = item.audio
                arr = np.asarray(item, dtype=np.float32)
                if arr.ndim > 1:
                    arr = arr.squeeze()
                return arr

            processed_audio = []
            for item in audio_items:
                processed_audio.append(_to_audio_array(item))

            audio_features = feature_extractor(
                processed_audio,
                sampling_rate=feature_extractor.sampling_rate,
                return_tensors="pt",
                padding="max_length",
            )
            max_mel_len = audio_features["input_features"].shape[-1]

            # Keep audio prompts aligned with torch reference path
            # (audio BOS + repeated audio token + audio EOS, no added special token).
            prompt_ids = tokenizer.encode(prompt, add_special_tokens=False)
            prompt_ids = self._apply_hf_processor_tokens_only(prompt_ids)

            feature_attention_mask = torch.zeros(
                (audio_features["input_features"].shape[0], max_mel_len),
                dtype=torch.long,
            )
            feature_attention_mask[:] = 1
            output = {
                "input_ids": [prompt_ids],
                "input_features": audio_features["input_features"],
                "feature_attention_mask": feature_attention_mask,
            }
            return BatchFeature(output, tensor_type="pt")

        if has_audio:
            feature_extractor = self.info.get_feature_extractor(**mm_kwargs)
            mm_kwargs = dict(**mm_kwargs, sampling_rate=feature_extractor.sampling_rate)

        if has_videos:
            mm_kwargs = dict(mm_kwargs, do_sample_frames=False)

        return super()._call_hf_processor(
            prompt=prompt,
            mm_data=mm_data,
            mm_kwargs=mm_kwargs,
            tok_kwargs=tok_kwargs,
        )

    def _get_mm_fields_config(
        self,
        hf_inputs: BatchFeature,
        hf_processor_mm_kwargs: Mapping[str, object],
    ) -> Mapping[str, MultiModalFieldConfig]:
        hf_cfg = self.info.get_hf_config()
        spatial_merge_size = getattr(hf_cfg.vision_config, "spatial_merge_size", 2)
        fields = dict(_create_qwen2vl_field_factory(spatial_merge_size)(hf_inputs))
        if "input_features" in hf_inputs:
            fields["input_features"] = MultiModalFieldConfig.batched("audio")
        if "feature_attention_mask" in hf_inputs:
            fields["feature_attention_mask"] = MultiModalFieldConfig.batched(
                "audio", keep_on_cpu=True
            )
        if "audio_embeds" in hf_inputs:
            fields["audio_embeds"] = MultiModalFieldConfig.batched("audio")
        return fields

    def _get_prompt_updates(
        self,
        mm_items: MultiModalDataItems,
        hf_processor_mm_kwargs: Mapping[str, object],
        out_mm_kwargs: MultiModalKwargsItems,
    ) -> Sequence[PromptUpdate]:
        updates = []
        hf_config = self.info.get_hf_config()
        out_mm_data = out_mm_kwargs.get_data()

        image_token_index = getattr(hf_config, "image_token_index", None)
        audio_token_id = getattr(hf_config, "audio_token_id", None)
        has_image_items = any(
            key in out_mm_data for key in ("pixel_values", "image_embeds", "image_grid_thw")
        )
        has_video_items = any(
            key in out_mm_data for key in ("pixel_values_videos", "video_embeds", "video_grid_thw")
        )
        has_audio_items = any(
            key in out_mm_data
            for key in ("audio_embeds", "input_features", "feature_attention_mask")
        )

        spatial_merge_size = getattr(
            hf_config.vision_config, "spatial_merge_size", 2
        )

        def _vision_replacement(grid_thw, item_idx: int):
            if grid_thw is not None:
                thw = grid_thw[item_idx]
                t, h, w = thw.tolist() if hasattr(thw, "tolist") else (int(thw[0]), int(thw[1]), int(thw[2]))
                n = int(t) * (int(h) // spatial_merge_size) * (int(w) // spatial_merge_size)
            else:
                n = 1
            return [image_token_index] * n

        if image_token_index is not None and has_image_items:
            image_grid_thw = out_mm_data.get("image_grid_thw")
            updates.append(
                PromptReplacement(
                    modality="image",
                    target=[image_token_index],
                    replacement=lambda idx: _vision_replacement(image_grid_thw, idx),
                )
            )

        if image_token_index is not None and has_video_items:
            # processing_llava_eurobert.py maps both image and video tokens to
            # "<image>"; the prompt uses <image> for video too.
            video_grid_thw = out_mm_data.get("video_grid_thw")
            updates.append(
                PromptReplacement(
                    modality="video",
                    target=[image_token_index],
                    replacement=lambda idx: _vision_replacement(video_grid_thw, idx),
                )
            )

        if audio_token_id is not None and has_audio_items:
            feature_attention_mask = out_mm_data.get("feature_attention_mask")
            if feature_attention_mask is not None:
                assert isinstance(feature_attention_mask, torch.Tensor)
                _, audio_output_lens = _get_feat_extract_output_lengths(
                    feature_attention_mask.sum(-1)
                )
                audio_output_lengths = audio_output_lens.tolist()
            else:
                audio_output_lengths = []

            def get_audio_replacement(item_idx: int):
                if audio_output_lengths:
                    n = audio_output_lengths[item_idx]
                elif "audio_embeds" in out_mm_data:
                    embeds = out_mm_data["audio_embeds"][item_idx]
                    n = embeds.shape[0]
                elif "input_features" in out_mm_data:
                    raw_feats = out_mm_data["input_features"]
                    if isinstance(raw_feats, torch.Tensor):
                        feats = raw_feats[item_idx]
                    else:
                        feat_item = raw_feats[item_idx]
                        feats = feat_item.data if hasattr(feat_item, "data") else feat_item
                    feature_len = int(feats.shape[-1])
                    _, output_lengths = _get_feat_extract_output_lengths(
                        torch.tensor([feature_len], dtype=torch.long)
                    )
                    n = int(output_lengths.item())
                else:
                    n = 1
                return [audio_token_id] * n

            updates.append(
                PromptReplacement(
                    modality="audio",
                    target=[audio_token_id],
                    replacement=get_audio_replacement,
                )
            )

        return updates


# --------------------------------------------------------------------------- #
#  Model
# --------------------------------------------------------------------------- #


@MULTIMODAL_REGISTRY.register_processor(
    NanoMMMultiModalProcessor,
    info=NanoMMProcessingInfo,
    dummy_inputs=NanoMMDummyInputsBuilder,
)
class LlavaEuroBertAudioForVLLMEmbedding(nn.Module, SupportsMultiModal, SupportsPP):
    """vLLM model for LlavaEuroBertAudioForEmbedding (nano multimodal embedding)."""

    @classmethod
    def get_placeholder_str(cls, modality: str, i: int) -> str | None:
        if modality == "image":
            return "<image>"
        if modality == "video":
            return "<image>"
        if modality.startswith("audio"):
            return f"Audio {i}: <|audio_bos|><|AUDIO|><|audio_eos|>"
        return None

    def __init__(self, *, vllm_config: VllmConfig, prefix: str = ""):
        super().__init__()
        config = vllm_config.model_config.hf_config
        self.config = config
        self.audio_token_id = getattr(config, "audio_token_id", None)

        vis_cfg = config.vision_config
        if not isinstance(vis_cfg, Qwen3VLVisionConfig):
            if hasattr(vis_cfg, "to_dict"):
                d = vis_cfg.to_dict()
            else:
                d = dict(vis_cfg)
            d.pop("model_type", None)
            d.pop("transformers_version", None)
            vis_cfg = Qwen3VLVisionConfig(**d)
        vis_cfg.deepstack_visual_indexes = []

        txt_cfg = config.text_config
        if isinstance(txt_cfg, dict):
            from transformers import LlamaConfig
            txt_cfg = LlamaConfig(**txt_cfg)
        # transformers 5.x aliases rope_scaling and rope_parameters via the
        # same proxy. Assigning rope_scaling = None silently nulls
        # rope_parameters too, making vLLM's get_rope fall back to
        # base=10000 (default Llama) instead of the model's rope_theta.
        # Set only rope_theta + rope_parameters; never touch rope_scaling.
        rope_params = getattr(txt_cfg, "rope_parameters", None)
        if rope_params:
            rope_theta = float(rope_params.get("rope_theta", 10000.0))
            clean = dict(rope_params)
            clean["rope_theta"] = rope_theta
            txt_cfg.rope_theta = rope_theta
            txt_cfg.rope_parameters = clean
        text_hidden = txt_cfg.hidden_size

        aud_cfg = config.audio_config
        if isinstance(aud_cfg, dict):
            aud_cfg = Qwen2_5OmniAudioEncoderConfig(**aud_cfg)

        spatial_merge_size = getattr(vis_cfg, "spatial_merge_size", 2)
        self._spatial_merge_size = spatial_merge_size

        with self._mark_tower_model(vllm_config, {"image", "video"}):
            self.vision_tower = Qwen3VLVisionModel(vis_cfg)
            self.vision_tower.merger = nn.Identity()
            self.vision_tower.deepstack_merger_list = nn.ModuleList()
            self.vision_tower.deepstack_visual_indexes = []

            self.merger = PretrainedMerger(
                vis_cfg.hidden_size, text_hidden, spatial_merge_size
            )

        with self._mark_tower_model(vllm_config, "audio"):
            self.audio_tower = Qwen2_5OmniAudioEncoder(aud_cfg)
            self.audio_tower.proj = nn.Identity()  # fused into audio_projector
            d_model = getattr(aud_cfg, "d_model", 1280)
            self.audio_projector = nn.Linear(d_model, text_hidden)

        self.multi_modal_projector = nn.Identity()
        self.lm_head = nn.Identity()

        with self._mark_language_model(vllm_config):
            self.language_model = init_vllm_registered_model(
                vllm_config=vllm_config,
                hf_config=txt_cfg,
                prefix=maybe_prefix(prefix, "language_model"),
                architectures=["LlamaBidirectionalModel"],
            )

        self.make_empty_intermediate_tensors = (
            self.language_model.make_empty_intermediate_tensors
        )
        self._init_audio_alignment(text_hidden, vllm_config)
        # Default audio prompt path expands to:
        # <audio_bos> + 750 audio tokens + <audio_eos> = 752 tokens.
        self._audio_default_seq_len = 752
        # Set in embed_multimodal when the batch contains audio; consumed and
        # cleared in forward so the seq_len fallback never fires for text-only.
        self._pending_audio_in_batch = False

    def _init_audio_alignment(self, hidden_size: int, vllm_config: VllmConfig) -> None:
        if self.audio_token_id is None:
            return
        if os.getenv("JINA_OMNI_DISABLE_AUDIO_ALIGNMENT") == "1":
            return

        candidate_paths: list[Path] = [Path(__file__).with_name("audio_linear_alignment.pt")]
        model_path = getattr(vllm_config.model_config, "model", None)
        if isinstance(model_path, str):
            candidate_paths.append(Path(model_path) / "audio_linear_alignment.pt")

        alignment_path = next((p for p in candidate_paths if p.exists()), None)

        if alignment_path is None and isinstance(model_path, str) and "/" in model_path:
            try:
                from huggingface_hub import hf_hub_download
                alignment_path = Path(hf_hub_download(
                    model_path, "audio_linear_alignment.pt",
                ))
            except Exception:
                pass

        if alignment_path is None:
            return

        payload = torch.load(alignment_path, map_location="cpu")
        matrix = payload.get("W") if isinstance(payload, dict) else payload
        if not isinstance(matrix, torch.Tensor):
            return
        if matrix.ndim != 2:
            return
        if matrix.shape[0] != hidden_size or matrix.shape[1] != hidden_size:
            return

        self.register_buffer(
            "audio_linear_alignment", matrix.to(torch.float32), persistent=False
        )

    def _apply_audio_alignment(
        self,
        hidden_states: torch.Tensor,
        input_ids: torch.Tensor | None,
        positions: torch.Tensor | None,
        has_audio: bool = False,
    ) -> torch.Tensor:
        alignment_matrix = getattr(self, "audio_linear_alignment", None)
        if alignment_matrix is None:
            return hidden_states
        if positions is None:
            return hidden_states

        flat_positions = positions.reshape(-1)
        if flat_positions.shape[0] != hidden_states.shape[0]:
            return hidden_states
        flat_input_ids = input_ids.reshape(-1) if input_ids is not None else None

        seq_starts = torch.nonzero(flat_positions.eq(0), as_tuple=False).flatten()
        if seq_starts.numel() == 0:
            seq_starts = flat_positions.new_tensor([0])
        elif seq_starts[0].item() != 0:
            seq_starts = torch.cat([flat_positions.new_tensor([0]), seq_starts], dim=0)

        seq_ends = torch.cat(
            [seq_starts[1:], flat_positions.new_tensor([flat_positions.numel()])], dim=0
        )

        alignment_matrix = alignment_matrix.to(
            device=hidden_states.device, dtype=torch.float32
        )
        aligned_hidden_states = hidden_states.float()
        for start, end in zip(seq_starts.tolist(), seq_ends.tolist()):
            seq_len = end - start
            apply_alignment = False

            if flat_input_ids is not None and self.audio_token_id is not None:
                apply_alignment = bool(torch.any(flat_input_ids[start:end].eq(self.audio_token_id)))
            elif has_audio:
                # vLLM pooling runner passes only inputs_embeds (input_ids=None).
                # Only trust the default-length marker when embed_multimodal
                # actually processed audio for this batch — otherwise a text
                # prompt that happens to pack to 752 tokens would be poisoned.
                apply_alignment = seq_len == self._audio_default_seq_len

            if apply_alignment:
                aligned_hidden_states[start:end] = aligned_hidden_states[start:end] @ alignment_matrix
        return aligned_hidden_states.to(hidden_states.dtype)

    # ---- vision processing ---- #

    def _process_image_input(self, pixel_values, image_grid_thw):
        vision_output = self.vision_tower(
            hidden_states=pixel_values, grid_thw=image_grid_thw
        )
        raw_hidden = vision_output[0] if isinstance(vision_output, tuple) else vision_output[0]

        image_features = self.merger(raw_hidden)

        merge = self._spatial_merge_size
        tokens_per_image = []
        if isinstance(image_grid_thw, list):
            for t, h, w in image_grid_thw:
                n = int(t) * (int(h) // merge) * (int(w) // merge)
                tokens_per_image.append(n)
        else:
            for i in range(image_grid_thw.shape[0]):
                t, h, w = image_grid_thw[i].tolist()
                n = int(t) * (int(h) // merge) * (int(w) // merge)
                tokens_per_image.append(n)

        per_image_features = []
        offset = 0
        for n in tokens_per_image:
            feat = image_features[offset : offset + n]
            per_image_features.append(feat)
            offset += n

        return per_image_features

    # ---- audio processing ---- #

    def _parse_and_validate_audio_input(
        self, **kwargs: object
    ) -> NanoAudioInputs | None:
        input_features = kwargs.pop("input_features", None)
        audio_embeds = kwargs.pop("audio_embeds", None)
        feature_attention_mask = kwargs.pop("feature_attention_mask", None)

        if input_features is None and audio_embeds is None:
            return None

        if audio_embeds is not None:
            return NanoAudioEmbeddingInputs(
                type="audio_embeds", audio_embeds=audio_embeds
            )

        return NanoAudioFeatureInputs(
            type="audio_features",
            input_features=input_features,
            feature_attention_mask=feature_attention_mask,
        )

    def _process_audio_input(
        self, audio_input: NanoAudioInputs
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

    # ---- embed_multimodal ---- #

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
        kwargs.pop("timestamps", None)
        if pixel_values_videos is not None and video_grid_thw is not None:
            embeddings.extend(
                self._process_image_input(pixel_values_videos, video_grid_thw)
            )

        audio_input = self._parse_and_validate_audio_input(**kwargs)
        if audio_input is not None:
            self._pending_audio_in_batch = True
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

        has_audio = self._pending_audio_in_batch
        self._pending_audio_in_batch = False

        hidden_states = self.language_model.model(
            input_ids, positions, intermediate_tensors, inputs_embeds=inputs_embeds
        )
        hidden_states = self._apply_audio_alignment(
            hidden_states, input_ids, positions, has_audio=has_audio
        )
        return hidden_states

    def compute_logits(self, hidden_states: torch.Tensor) -> torch.Tensor | None:
        return self.language_model.compute_logits(hidden_states)

    # ---- weight loading ---- #

    @staticmethod
    def _remap_weights(
        weights: Iterable[tuple[str, torch.Tensor]],
    ) -> Iterable[tuple[str, torch.Tensor]]:
        for name, tensor in weights:
            if name.startswith("language_model.") and not name.startswith(
                "language_model.model."
            ) and not name.startswith("language_model.lm_head."):
                name = "language_model.model." + name[len("language_model."):]
            yield name, tensor

    def load_weights(self, weights: Iterable[tuple[str, torch.Tensor]]) -> set[str]:
        loader = AutoWeightsLoader(self)
        return loader.load_weights(self._remap_weights(weights))


_IMAGE_TOKEN = "<image>"
_IMAGE_PLACEHOLDER = "<image>"
_VIDEO_PLACEHOLDER = "<image>"
_AUDIO_PLACEHOLDER = "<|audio_start|><|audio_pad|><|audio_end|>"


def _image_chat_prompt(text: str = "") -> str:
    return f"<|im_start|>user\n<|vision_start|>{_IMAGE_TOKEN}<|vision_end|>{text}<|im_end|>\n"


def format_prompt(text: str = "", image=None, video=None, audio=None) -> dict:
    """Build a `llm.embed(...)` request dict for jina-embeddings-v5-omni-nano.

    Inserts the model's vision/audio placeholder tokens for you so callers
    don't need to spell them out.

    For audio, also pass ``tokenization_kwargs={"add_special_tokens": False}``
    to ``llm.embed`` so that LAST-token pooling lands on `<|audio_end|>` rather
    than the tokenizer's auto-appended `<|end_of_text|>`.
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


import sys as _sys
_sys.modules.setdefault("jina_v5_omni", _sys.modules[__name__])
