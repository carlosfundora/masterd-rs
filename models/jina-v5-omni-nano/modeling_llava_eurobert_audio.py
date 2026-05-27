"""
LlavaEuroBertAudioForEmbedding: Qwen3VL vision + Qwen2.5-Omni audio + EuroBERT text.

Architecture:
  - Vision: Qwen3VLVisionModel (with RoPE, 3D Conv3d patch embed, all layers)
  - Merger: PretrainedMerger (top-level, NOT inside vision_tower)
  - Audio: Qwen2_5OmniAudioEncoder (Qwen2.5-Omni) + Linear projector
  - Text: LlamaModel (EuroBERT, bidirectional)
  - LM head: Identity (embedding model, no vocab projection)

Modality loading:
  model = AutoModel.from_pretrained(path, trust_remote_code=True, modality="omni")   # all components (default)
  model = AutoModel.from_pretrained(path, trust_remote_code=True, modality="vision") # no audio tower/projector
  model = AutoModel.from_pretrained(path, trust_remote_code=True, modality="audio")  # no vision tower/merger
"""

from typing import List, Optional, Union

import torch
import torch.nn as nn
from transformers import LlamaConfig, PreTrainedModel, PretrainedConfig
from transformers.modeling_outputs import BaseModelOutputWithPast
from transformers.models.llama.modeling_llama import LlamaModel
from transformers.models.qwen3_vl.configuration_qwen3_vl import Qwen3VLVisionConfig
from transformers.models.qwen3_vl.modeling_qwen3_vl import Qwen3VLVisionModel
from transformers.models.qwen2_5_omni.configuration_qwen2_5_omni import Qwen2_5OmniAudioEncoderConfig
from transformers.models.qwen2_5_omni.modeling_qwen2_5_omni import Qwen2_5OmniAudioEncoder


_VALID_MODALITIES = ("omni", "vision", "audio", "text")


class PretrainedMerger(nn.Module):
    def __init__(self, hidden_size, out_hidden_size, spatial_merge_size=2):
        super().__init__()
        self.hidden_size = hidden_size * (spatial_merge_size**2)
        self.norm = nn.LayerNorm(hidden_size, eps=1e-6)
        self.linear_fc1 = nn.Linear(self.hidden_size, self.hidden_size)
        self.act = nn.GELU()
        self.linear_fc2 = nn.Linear(self.hidden_size, out_hidden_size)

    def forward(self, x):
        x = self.norm(x)
        x = x.view(-1, self.hidden_size)
        x = self.linear_fc2(self.act(self.linear_fc1(x)))
        return x


class LlavaEuroBertAudioConfig(PretrainedConfig):
    model_type = "llava_eurobert_audio"

    def __init__(
        self,
        vision_config=None,
        text_config=None,
        audio_config=None,
        image_token_index=None,
        audio_token_id=None,
        audio_start_token_id=None,
        audio_end_token_id=None,
        projector_hidden_act="gelu",
        tie_word_embeddings=False,
        modality="omni",
        **kwargs,
    ):
        if isinstance(vision_config, dict):
            vision_config = PretrainedConfig(**vision_config)
        self.vision_config = vision_config or PretrainedConfig()
        if isinstance(text_config, dict):
            text_config = PretrainedConfig(**text_config)
        self.text_config = text_config or PretrainedConfig()
        if isinstance(audio_config, dict):
            audio_config = PretrainedConfig(**audio_config)
        self.audio_config = audio_config or PretrainedConfig()
        self.image_token_index = image_token_index
        self.audio_token_id = audio_token_id
        self.audio_start_token_id = audio_start_token_id
        self.audio_end_token_id = audio_end_token_id
        self.projector_hidden_act = projector_hidden_act
        if modality not in _VALID_MODALITIES:
            raise ValueError(f"modality must be one of {_VALID_MODALITIES}, got '{modality}'")
        self.modality = modality
        super().__init__(tie_word_embeddings=tie_word_embeddings, **kwargs)

    def get_text_config(self, **kwargs):
        return self.text_config


class LlavaEuroBertAudioForEmbedding(PreTrainedModel):
    config_class = LlavaEuroBertAudioConfig
    supports_gradient_checkpointing = True
    _supports_sdpa = True
    _supports_flash_attn_2 = True
    _supports_attention_backend = True
    _tied_weights_keys = []
    _keys_to_ignore_on_load_missing = ["lm_head.weight"]
    _keys_to_ignore_on_load_unexpected = []

    def __init__(self, config: LlavaEuroBertAudioConfig):
        super().__init__(config)

        modality = getattr(config, "modality", "omni")
        if modality not in _VALID_MODALITIES:
            raise ValueError(f"modality must be one of {_VALID_MODALITIES}, got '{modality}'")
        self._modality = modality

        vision_cfg = config.vision_config
        if not isinstance(vision_cfg, Qwen3VLVisionConfig):
            if hasattr(vision_cfg, "to_dict"):
                d = vision_cfg.to_dict()
            else:
                d = dict(vision_cfg)
            d.pop("model_type", None)
            d.pop("transformers_version", None)
            vision_cfg = Qwen3VLVisionConfig(**d)

        vision_cfg.deepstack_visual_indexes = []
        spatial_merge_size = getattr(vision_cfg, "spatial_merge_size", 2)

        text_cfg = config.text_config
        if not isinstance(text_cfg, LlamaConfig):
            txt_dict = text_cfg.to_dict() if hasattr(text_cfg, 'to_dict') else dict(text_cfg)
            _saved_attn_impl = getattr(text_cfg, "_attn_implementation", None)
            text_cfg = LlamaConfig(**txt_dict)
            if _saved_attn_impl is not None:
                text_cfg._attn_implementation = _saved_attn_impl
        text_hidden = text_cfg.hidden_size

        self._spatial_merge_size = spatial_merge_size
        self._vision_hidden_size = getattr(vision_cfg, "hidden_size", 768)

        if modality not in ("audio", "text"):
            self.vision_tower = Qwen3VLVisionModel(vision_cfg)
            self.vision_tower.merger = nn.Identity()
            self.vision_tower.deepstack_merger_list = nn.ModuleList()
            self.vision_tower.deepstack_visual_indexes = []
            self.merger = PretrainedMerger(
                vision_cfg.hidden_size, text_hidden, spatial_merge_size
            )

        self.multi_modal_projector = nn.Identity()
        self.language_model = LlamaModel(text_cfg)
        self.lm_head = nn.Identity()

        for layer in self.language_model.layers:
            layer.self_attn.is_causal = False

        if modality not in ("vision", "text"):
            aud_cfg = config.audio_config
            aud_dict = aud_cfg.to_dict() if hasattr(aud_cfg, 'to_dict') else aud_cfg
            audio_encoder_config = Qwen2_5OmniAudioEncoderConfig(**aud_dict)
            self.audio_tower = Qwen2_5OmniAudioEncoder(audio_encoder_config)
            output_dim = aud_dict.get('output_dim', 3584)
            self.audio_projector = nn.Linear(output_dim, text_hidden)

        ignore = []
        if modality in ("audio", "text"):
            ignore.extend([r"^vision_tower\.", r"^merger\."])
        if modality in ("vision", "text"):
            ignore.extend([r"^audio_tower\.", r"^audio_projector\."])
        if ignore:
            self._keys_to_ignore_on_load_unexpected = ignore

        self.post_init()

    @property
    def modality(self) -> str:
        return self._modality

    def get_input_embeddings(self):
        return self.language_model.embed_tokens

    def set_input_embeddings(self, value):
        self.language_model.embed_tokens = value

    def get_output_embeddings(self):
        return None

    def get_image_features(
        self,
        pixel_values: torch.FloatTensor,
        image_grid_thw: torch.LongTensor,
        num_image_tokens: Optional[int] = None,
    ) -> List[torch.Tensor]:
        if self._modality in ("audio", "text"):
            raise ValueError(
                f"Vision inputs are not available in {self._modality}-only mode. "
                "Load with modality='omni' or modality='vision'."
            )

        vision_output = self.vision_tower(
            hidden_states=pixel_values, grid_thw=image_grid_thw
        )
        if isinstance(vision_output, tuple):
            raw_hidden = vision_output[0]
        elif hasattr(vision_output, "pooler_output") and vision_output.pooler_output is not None:
            raw_hidden = vision_output.pooler_output
        else:
            raw_hidden = vision_output[0]

        image_features = self.merger(raw_hidden)

        merge_sq = self._spatial_merge_size ** 2
        split_sizes = (image_grid_thw.prod(-1) // merge_sq).tolist()
        return list(torch.split(image_features, split_sizes))

    def get_audio_features(
        self,
        input_features: torch.FloatTensor,
        feature_attention_mask: Optional[torch.LongTensor] = None,
    ) -> torch.Tensor:
        if self._modality in ("vision", "text"):
            raise ValueError(
                f"Audio inputs are not available in {self._modality}-only mode. "
                "Load with modality='omni' or modality='audio'."
            )

        batch_size = input_features.shape[0]
        if batch_size > 1:
            # Serialize per-sample so the packed-frames GEMM shape stays invariant
            # across batch sizes. Makes batched audio bit-exact to B=1 in bf16,
            # and is substantially faster for B>=16 because B=1 hits a
            # well-optimized kernel while the packed-B=N path thrashes on a
            # (total_frames)^2 sdpa matrix.
            outs = [
                self.get_audio_features(
                    input_features[i : i + 1],
                    feature_attention_mask[i : i + 1] if feature_attention_mask is not None else None,
                )
                for i in range(batch_size)
            ]
            return torch.cat(outs, dim=0)
        if feature_attention_mask is not None:
            feature_lens = feature_attention_mask.sum(-1).long()
            packed = input_features.permute(0, 2, 1)[feature_attention_mask.bool()].permute(1, 0)
        else:
            feature_lens = torch.full(
                (batch_size,), input_features.shape[2],
                device=input_features.device, dtype=torch.long,
            )
            packed = input_features.transpose(1, 2).reshape(-1, input_features.shape[1]).T
        aftercnn_lens, _ = self.audio_tower._get_feat_extract_output_lengths(feature_lens)
        audio_output = self.audio_tower(
            packed, feature_lens=feature_lens, aftercnn_lens=aftercnn_lens,
        )
        return self.audio_projector(audio_output.last_hidden_state)

    def forward(
        self,
        input_ids: Optional[torch.LongTensor] = None,
        pixel_values: Optional[torch.FloatTensor] = None,
        attention_mask: Optional[torch.Tensor] = None,
        position_ids: Optional[torch.LongTensor] = None,
        past_key_values=None,
        inputs_embeds: Optional[torch.FloatTensor] = None,
        input_features: Optional[torch.FloatTensor] = None,
        feature_attention_mask: Optional[torch.LongTensor] = None,
        cache_position: Optional[torch.LongTensor] = None,
        output_hidden_states: Optional[bool] = None,
        **kwargs,
    ):
        image_grid_thw = kwargs.pop("image_grid_thw", None)
        num_image_tokens = kwargs.pop("num_image_tokens", None)
        kwargs.pop("spatial_shapes", None)
        kwargs.pop("pixel_attention_mask", None)

        if pixel_values is not None and self._modality in ("audio", "text"):
            raise ValueError(
                f"Vision inputs are not available in {self._modality}-only mode. "
                "Load with modality='omni' or modality='vision'."
            )
        if input_features is not None and self._modality in ("vision", "text"):
            raise ValueError(
                f"Audio inputs are not available in {self._modality}-only mode. "
                "Load with modality='omni' or modality='audio'."
            )

        if (input_ids is None) ^ (inputs_embeds is not None):
            raise ValueError(
                "You must specify exactly one of input_ids or inputs_embeds"
            )

        if inputs_embeds is None:
            inputs_embeds = self.get_input_embeddings()(input_ids)

        if pixel_values is not None and image_grid_thw is not None:
            image_features = self.get_image_features(
                pixel_values=pixel_values,
                image_grid_thw=image_grid_thw,
                num_image_tokens=num_image_tokens,
            )
            image_features = torch.cat(image_features, dim=0).to(
                inputs_embeds.device, inputs_embeds.dtype
            )
            special_image_mask = (
                (input_ids == self.config.image_token_index)
                .unsqueeze(-1)
                .expand_as(inputs_embeds)
            )
            inputs_embeds = inputs_embeds.masked_scatter(
                special_image_mask, image_features
            )

        if input_features is not None:
            audio_embeds = self.get_audio_features(
                input_features, feature_attention_mask
            )
            audio_embeds_flat = audio_embeds.reshape(
                -1, audio_embeds.shape[-1]
            ).to(inputs_embeds.device, inputs_embeds.dtype)
            audio_mask = (
                (input_ids == self.config.audio_token_id)
                .unsqueeze(-1)
                .expand_as(inputs_embeds)
            )
            inputs_embeds = inputs_embeds.masked_scatter(
                audio_mask, audio_embeds_flat
            )

        if attention_mask is not None and attention_mask.dim() == 2:
            dtype = inputs_embeds.dtype
            seq_len = inputs_embeds.shape[1]
            bidi_mask = attention_mask[:, None, None, :].to(dtype=dtype)
            bidi_mask = (1.0 - bidi_mask) * torch.finfo(dtype).min
            attention_mask = bidi_mask.expand(-1, -1, seq_len, -1)

        # vLLM's transformers backend passes `return_dict=False` + `attention_instances`.
        # Force dict-style output internally, and forward remaining kwargs so the
        # vllm attention hook receives its `attention_instances` dict.
        kwargs.pop("return_dict", None)
        outputs = self.language_model(
            attention_mask=attention_mask,
            position_ids=position_ids,
            past_key_values=past_key_values,
            inputs_embeds=inputs_embeds,
            cache_position=cache_position,
            output_hidden_states=output_hidden_states,
            return_dict=True,
            **kwargs,
        )

        hidden_states = outputs[0]
        logits = self.lm_head(hidden_states)

        return BaseModelOutputWithPast(
            last_hidden_state=logits,
            past_key_values=outputs.past_key_values,
            hidden_states=outputs.hidden_states,
            attentions=outputs.attentions,
        )


def _register_vllm() -> None:
    import importlib.util as _iu
    if _iu.find_spec("vllm") is None:
        return
    try:
        import os, sys, importlib, shutil
        pkg = __package__ or ""
        current_dir = os.path.dirname(os.path.abspath(__file__))
        sibling_name = "vllm_llava_eurobert_audio"
        sibling_path = os.path.join(current_dir, sibling_name + ".py")
        if not os.path.exists(sibling_path):
            parts = pkg.split(".")
            if len(parts) >= 4 and parts[0] == "transformers_modules":
                from huggingface_hub import hf_hub_download
                repo_name = parts[2].replace("_hyphen_", "-").replace("_dot_", ".")
                repo_id = f"{parts[1]}/{repo_name}"
                downloaded = hf_hub_download(
                    repo_id=repo_id,
                    filename=sibling_name + ".py",
                    revision=parts[3],
                )
                shutil.copy(downloaded, sibling_path)
        if current_dir not in sys.path:
            sys.path.insert(0, current_dir)
        existing = os.environ.get("PYTHONPATH", "")
        if current_dir not in existing.split(os.pathsep):
            os.environ["PYTHONPATH"] = (
                current_dir if not existing else current_dir + os.pathsep + existing
            )
        if pkg:
            _lla = importlib.import_module("." + sibling_name, package=pkg)
        else:
            _lla = importlib.import_module(sibling_name)
        from vllm import ModelRegistry
        ModelRegistry.register_model(
            "LlavaEuroBertAudioForEmbedding",
            _lla.LlavaEuroBertAudioForVLLMEmbedding,
        )
    except Exception as e:
        import warnings
        warnings.warn(
            f"jina-embeddings-v5-omni nano: vLLM registration failed "
            f"({type(e).__name__}: {e}); falling back to Transformers backend.",
            stacklevel=2,
        )


_register_vllm()
