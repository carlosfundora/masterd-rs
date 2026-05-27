"""
Unified jina-embeddings-v5-omni-nano: vision + audio + text with task-specific routing.

Shared:  Qwen3VLVisionModel + Qwen2.5-Omni audio encoder + LlamaModel (EuroBERT, bidirectional)
Per-task: vision merger, audio projector, special token embeddings, LoRA adapter

Modality loading:
    model = AutoModel.from_pretrained(path, trust_remote_code=True)                      # all components (default)
    model = AutoModel.from_pretrained(path, trust_remote_code=True, modality="vision")    # no audio tower/projectors
    model = AutoModel.from_pretrained(path, trust_remote_code=True, modality="audio")     # no vision tower/mergers

Usage:
    model = AutoModel.from_pretrained("jinaai/jina-embeddings-v5-omni-nano", trust_remote_code=True)
    embeddings = model.encode(["hello world"], task="retrieval")
"""

from typing import List, Optional
import os

import torch
import torch.nn as nn
import torch.nn.functional as F

from huggingface_hub import snapshot_download
from transformers import AutoTokenizer, LlamaConfig, PreTrainedModel, PretrainedConfig
from transformers.modeling_outputs import BaseModelOutputWithPast
from transformers.models.llama.modeling_llama import LlamaModel
from transformers.models.qwen3_vl.configuration_qwen3_vl import Qwen3VLVisionConfig
from transformers.models.qwen3_vl.modeling_qwen3_vl import Qwen3VLVisionModel
from transformers.models.qwen2_5_omni.configuration_qwen2_5_omni import Qwen2_5OmniAudioEncoderConfig
from transformers.models.qwen2_5_omni.modeling_qwen2_5_omni import Qwen2_5OmniAudioEncoder
from peft import PeftMixedModel, PeftConfig

TASK_NAMES = ["retrieval", "text-matching", "clustering", "classification"]
_VALID_MODALITIES = ("omni", "vision", "audio", "text")


def _key(task):
    return task.replace("-", "_")


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


class JinaEmbeddingsV5OmniConfig(PretrainedConfig):
    model_type = "jina_embeddings_v5_omni"

    def __init__(
        self,
        vision_config=None,
        text_config=None,
        audio_config=None,
        task_names=None,
        special_token_ids=None,
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
        self.task_names = task_names or TASK_NAMES
        self.special_token_ids = special_token_ids or []
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


class JinaEmbeddingsV5OmniBase(PreTrainedModel):
    config_class = JinaEmbeddingsV5OmniConfig
    supports_gradient_checkpointing = True
    _supports_sdpa = True
    _supports_flash_attn_2 = True
    _supports_attention_backend = True
    _tied_weights_keys = []
    _keys_to_ignore_on_load_missing = ["lm_head.weight"]
    _keys_to_ignore_on_load_unexpected = []

    def __init__(self, config: JinaEmbeddingsV5OmniConfig):
        super().__init__(config)

        modality = getattr(config, "modality", "omni")
        if modality not in _VALID_MODALITIES:
            raise ValueError(f"modality must be one of {_VALID_MODALITIES}, got '{modality}'")
        self._modality = modality

        vision_cfg = config.vision_config
        if not isinstance(vision_cfg, Qwen3VLVisionConfig):
            d = vision_cfg.to_dict() if hasattr(vision_cfg, "to_dict") else dict(vision_cfg)
            d.pop("model_type", None)
            d.pop("transformers_version", None)
            vision_cfg = Qwen3VLVisionConfig(**d)
        vision_cfg.deepstack_visual_indexes = []

        spatial_merge_size = getattr(vision_cfg, "spatial_merge_size", 2)
        self._spatial_merge_size = spatial_merge_size
        self._vision_hidden_size = vision_cfg.hidden_size

        text_cfg = config.text_config
        txt_dict = text_cfg.to_dict() if hasattr(text_cfg, "to_dict") else text_cfg
        if not isinstance(text_cfg, LlamaConfig):
            text_cfg = LlamaConfig(**txt_dict)
        text_hidden = text_cfg.hidden_size

        if modality not in ("audio", "text"):
            self.vision_tower = Qwen3VLVisionModel(vision_cfg)
            self.vision_tower.merger = nn.Identity()
            self.vision_tower.deepstack_merger_list = nn.ModuleList()
            self.vision_tower.deepstack_visual_indexes = []
            self.mergers = nn.ModuleDict({
                _key(t): PretrainedMerger(vision_cfg.hidden_size, text_hidden, spatial_merge_size)
                for t in config.task_names
            })

        self.language_model = LlamaModel(text_cfg)
        for layer in self.language_model.layers:
            layer.self_attn.is_causal = False

        self.multi_modal_projector = nn.Identity()
        self.lm_head = nn.Identity()

        if modality not in ("vision", "text"):
            aud_cfg = config.audio_config
            aud_dict = aud_cfg.to_dict() if hasattr(aud_cfg, "to_dict") else aud_cfg
            audio_encoder_config = Qwen2_5OmniAudioEncoderConfig(**aud_dict)
            self.audio_tower = Qwen2_5OmniAudioEncoder(audio_encoder_config)
            self.audio_tower.proj = nn.Identity()  # fused into audio_projector(s)
            output_dim = aud_dict.get('d_model', 1280)  # fused: audio_projector(s) now take d_model
            self.audio_projectors = nn.ModuleDict({
                _key(t): nn.Linear(output_dim, text_hidden) for t in config.task_names
            })

        ignore = []
        if modality in ("audio", "text"):
            ignore.extend([r"^vision_tower\.", r"^mergers\."])
        if modality in ("vision", "text"):
            ignore.extend([r"^audio_tower\.", r"^audio_projectors\."])
        if ignore:
            self._keys_to_ignore_on_load_unexpected = ignore

        n_special = len(config.special_token_ids)
        self.task_token_embeddings = nn.ParameterDict({
            _key(t): nn.Parameter(torch.zeros(n_special, text_hidden))
            for t in config.task_names
        })

        self._active_task_key = _key(config.task_names[0])
        self._special_token_ids = config.special_token_ids
        self.post_init()

    @property
    def modality(self) -> str:
        return self._modality

    def set_task(self, task):
        k = _key(task)
        self._active_task_key = k
        with torch.no_grad():
            w = self.language_model.embed_tokens.weight.data
            te = self.task_token_embeddings[k]
            for i, tid in enumerate(self._special_token_ids):
                w[tid] = te[i]

    def get_input_embeddings(self):
        return self.language_model.embed_tokens

    def set_input_embeddings(self, value):
        self.language_model.embed_tokens = value

    def get_output_embeddings(self):
        return None

    def get_image_features(self, pixel_values, image_grid_thw, num_image_tokens=None):
        if self._modality in ("audio", "text"):
            raise ValueError(
                f"Vision inputs are not available in {self._modality}-only mode. "
                "Load with modality='omni' or modality='vision'."
            )

        out = self.vision_tower(hidden_states=pixel_values, grid_thw=image_grid_thw)
        raw = out[0] if isinstance(out, tuple) else getattr(out, "last_hidden_state", out[0])
        merged = self.mergers[self._active_task_key](raw)

        merge = self._spatial_merge_size
        sizes = []
        for i in range(image_grid_thw.shape[0]):
            t, h, w = image_grid_thw[i].tolist()
            sizes.append(int(t) * (int(h) // merge) * (int(w) // merge))

        # Default: return the un-padded per-image feature slices. Their
        # concatenation has exactly sum(sizes) rows == number of <image>
        # placeholder tokens in input_ids, which is what masked_scatter
        # consumes. Padding is only meaningful when callers want a square
        # [N, max_tok, dim] block (e.g. multi-sample batched forward where
        # each row owns its own image), and that path passes
        # num_image_tokens explicitly to opt in.
        dim = merged.shape[-1]
        features, offset = [], 0
        if num_image_tokens is not None:
            max_tok = num_image_tokens
            for n in sizes:
                feat = merged[offset:offset + n]
                if n < max_tok:
                    feat = torch.cat([feat, feat.new_zeros(max_tok - n, dim)], dim=0)
                features.append(feat)
                offset += n
        else:
            for n in sizes:
                features.append(merged[offset:offset + n])
                offset += n
        return features

    def get_audio_features(self, input_features, feature_attention_mask=None):
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
        return self.audio_projectors[self._active_task_key](audio_output.last_hidden_state)

    def forward(
        self,
        input_ids=None,
        pixel_values=None,
        attention_mask=None,
        position_ids=None,
        past_key_values=None,
        inputs_embeds=None,
        input_features=None,
        feature_attention_mask=None,
        cache_position=None,
        output_hidden_states=None,
        **kwargs,
    ):
        image_grid_thw = kwargs.pop("image_grid_thw", None)
        num_image_tokens = kwargs.pop("num_image_tokens", None)
        pixel_values_videos = kwargs.pop("pixel_values_videos", None)
        video_grid_thw = kwargs.pop("video_grid_thw", None)
        num_video_tokens = kwargs.pop("num_video_tokens", None)
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
            raise ValueError("You must specify exactly one of input_ids or inputs_embeds")

        if inputs_embeds is None:
            inputs_embeds = self.get_input_embeddings()(input_ids)

        # Image and video both use config.image_token_index (the processor
        # remaps <|video_pad|> to <image>). When a single forward pass mixes
        # both modalities, the mask matches both sets of placeholders, so we
        # build one combined source with image features first then video
        # features, matching the order of placeholders in input_ids.
        all_feats = []
        if pixel_values is not None and image_grid_thw is not None:
            all_feats.extend(self.get_image_features(pixel_values, image_grid_thw, num_image_tokens))
        if pixel_values_videos is not None and video_grid_thw is not None:
            all_feats.extend(self.get_image_features(pixel_values_videos, video_grid_thw, num_video_tokens))
        if all_feats:
            feats = torch.cat(all_feats, dim=0).to(inputs_embeds.device, inputs_embeds.dtype)
            mask = (input_ids == self.config.image_token_index).unsqueeze(-1).expand_as(inputs_embeds)
            inputs_embeds = inputs_embeds.masked_scatter(mask, feats)

        if input_features is not None:
            aud = self.get_audio_features(input_features, feature_attention_mask)
            aud_flat = aud.reshape(-1, aud.shape[-1]).to(inputs_embeds.device, inputs_embeds.dtype)
            mask = (input_ids == self.config.audio_token_id).unsqueeze(-1).expand_as(inputs_embeds)
            inputs_embeds = inputs_embeds.masked_scatter(mask, aud_flat)

        if attention_mask is not None and attention_mask.dim() == 2:
            dtype = inputs_embeds.dtype
            seq_len = inputs_embeds.shape[1]
            bidi = attention_mask[:, None, None, :].to(dtype=dtype)
            bidi = (1.0 - bidi) * torch.finfo(dtype).min
            attention_mask = bidi.expand(-1, -1, seq_len, -1)

        out = self.language_model(
            attention_mask=attention_mask,
            position_ids=position_ids,
            past_key_values=past_key_values,
            inputs_embeds=inputs_embeds,
            cache_position=cache_position,
            output_hidden_states=output_hidden_states,
        )

        return BaseModelOutputWithPast(
            last_hidden_state=self.lm_head(out[0]),
            past_key_values=out.past_key_values,
            hidden_states=out.hidden_states,
            attentions=out.attentions,
        )


class JinaEmbeddingsV5OmniModel(PeftMixedModel):
    config_class = JinaEmbeddingsV5OmniConfig

    @classmethod
    def register_for_auto_class(cls, auto_class="AutoModel"):
        return PreTrainedModel.register_for_auto_class.__func__(cls, auto_class)

    @classmethod
    def from_pretrained(cls, pretrained_model_name_or_path, *args, **kwargs):
        modality = kwargs.pop("modality", None)
        task_kwarg = kwargs.pop("task", None)
        config = kwargs.pop("config", None)
        if config is None:
            config = JinaEmbeddingsV5OmniConfig.from_pretrained(pretrained_model_name_or_path)
        if modality is not None:
            config.modality = modality
        elif not hasattr(config, "modality") or config.modality is None:
            config.modality = "omni"

        default_dtype = getattr(config, "torch_dtype", None) or torch.float32
        base_model = JinaEmbeddingsV5OmniBase.from_pretrained(
            pretrained_model_name_or_path,
            config=config,
            torch_dtype=kwargs.pop("torch_dtype", kwargs.pop("dtype", default_dtype)),
        )

        if os.path.isdir(pretrained_model_name_or_path):
            adapters_dir = os.path.join(pretrained_model_name_or_path, "adapters")
        else:
            cache = snapshot_download(
                repo_id=pretrained_model_name_or_path,
                allow_patterns=["adapters/*"],
            )
            adapters_dir = os.path.join(cache, "adapters")

        adapter_paths = {
            name: os.path.join(adapters_dir, name) for name in config.task_names
        }

        peft_config = PeftConfig.from_pretrained(adapter_paths["retrieval"], **kwargs)
        model = cls(base_model, peft_config, adapter_name="retrieval")
        model._pretrained_path = pretrained_model_name_or_path
        for name in config.task_names:
            model.load_adapter(adapter_paths[name], adapter_name=name, **kwargs)

        model.tokenizer = AutoTokenizer.from_pretrained(
            pretrained_model_name_or_path, trust_remote_code=True,
        )
        # Task precedence: kwarg > config.task (hf_overrides path) > env var > default.
        task = task_kwarg
        if task is None:
            task = getattr(config, "task", None)
        if task is None:
            task = os.environ.get("JINA_V5_TASK")
        if task is None:
            task = config.task_names[0]
        if task not in config.task_names:
            raise ValueError(
                f"task must be one of {config.task_names}, got '{task}'"
            )
        model.set_adapter(task)
        return model

    @property
    def modality(self) -> str:
        return self.base_model.model.modality

    def set_adapter(self, adapters):
        super().set_adapter(adapters)
        task = adapters[0] if isinstance(adapters, list) else adapters
        self.base_model.model.set_task(task)

    def encode(
        self,
        texts: List[str],
        task: str,
        prompt_name: Optional[str] = "document",
        truncate_dim: Optional[int] = None,
        max_length: Optional[int] = None,
    ) -> torch.Tensor:
        cfg = self.base_model.model.config
        if task not in cfg.task_names:
            raise ValueError(f"Unknown task: {task}")
        if prompt_name is None:
            prompt_name = "document"
        if prompt_name not in {"query", "document"}:
            raise ValueError(f"Unknown prompt_name: {prompt_name}")

        prefix = "Query: " if prompt_name == "query" else "Document: "
        inputs = [f"{prefix}{t}" for t in texts]

        max_length = max_length or cfg.text_config.max_position_embeddings
        batch = self.tokenizer(
            inputs, return_tensors="pt", padding=True, truncation=True, max_length=max_length,
        )
        device = next(self.parameters()).device
        batch = {k: v.to(device) for k, v in batch.items()}
        self.set_adapter([task])
        self.eval()
        with torch.no_grad():
            hidden = self(**batch).last_hidden_state
            mask = batch.get("attention_mask")
            if mask is None:
                pooled = hidden[:, -1]
            else:
                seq_lens = mask.sum(dim=1) - 1
                pooled = hidden[torch.arange(hidden.shape[0], device=hidden.device), seq_lens]
            if truncate_dim is not None:
                pooled = pooled[:, :truncate_dim]
            return F.normalize(pooled, p=2, dim=-1)

    def embed(self, truncate_dim: Optional[int] = None, **inputs):
        """Encode processor outputs into L2-normalized last-token embeddings.

        Matryoshka: pass `truncate_dim=N` to get an N-dim unit-norm vector
        (truncation is applied before L2-normalization).
        """
        attention_mask = inputs.get("attention_mask", None)
        self.eval()
        with torch.no_grad():
            out = self(**inputs)
        hidden = out.last_hidden_state
        if attention_mask is not None and attention_mask.dim() == 2:
            idx = attention_mask.sum(dim=1) - 1
        else:
            idx = torch.full(
                (hidden.shape[0],), hidden.shape[1] - 1,
                device=hidden.device, dtype=torch.long,
            )
        pooled = hidden[torch.arange(hidden.shape[0], device=hidden.device), idx]
        if truncate_dim is not None:
            pooled = pooled[:, :truncate_dim]
        return torch.nn.functional.normalize(pooled, dim=-1)



# ---------------------------------------------------------------------------
# vLLM registration (side-effect on module import).
#
# Triggered via config.json "auto_map.AutoConfig" -> this module.
# HF / sentence-transformers path unaffected: any failure is silently swallowed
# so that pure transformers users never see a vLLM error.
# ---------------------------------------------------------------------------

def _register_vllm() -> None:
    # All vLLM references are resolved via importlib so transformers'
    # static check_imports does NOT flag vllm as a required dependency.
    # Pure-HF / sentence-transformers usage is unaffected.
    #
    # When loaded via transformers' `trust_remote_code=True`, only the
    # modeling_*.py referenced in auto_map is fetched into the
    # transformers_modules cache — sibling vLLM adapter files are NOT.
    # We pull them from HF Hub before registering; otherwise vLLM falls
    # back to its transformers backend (wrong attention semantics) and
    # multi-request batches collapse.
    import importlib.util as _iu
    if _iu.find_spec("vllm") is None:
        return
    try:
        import os
        import sys
        import importlib
        import inspect
        import shutil

        pkg = __package__ or ""
        current_dir = os.path.dirname(os.path.abspath(__file__))
        siblings = ("vllm_llava_eurobert_audio", "vllm_jina_v5_omni")

        for sibling_name in siblings:
            sibling_path = os.path.join(current_dir, sibling_name + ".py")
            if os.path.exists(sibling_path):
                continue
            parts = pkg.split(".")
            if len(parts) < 4 or parts[0] != "transformers_modules":
                continue
            from huggingface_hub import hf_hub_download
            repo_name = parts[2].replace("_hyphen_", "-").replace("_dot_", ".")
            repo_id = f"{parts[1]}/{repo_name}"
            downloaded = hf_hub_download(
                repo_id=repo_id,
                filename=sibling_name + ".py",
                revision=parts[3],
            )
            shutil.copy(downloaded, sibling_path)

        os.environ.setdefault("VLLM_ENABLE_V1_MULTIPROCESSING", "0")

        _kvc = importlib.import_module("vllm.v1.core.kv_cache_coordinator")
        _orig = _kvc.get_kv_cache_coordinator
        _NoPrefix = _kvc.KVCacheCoordinatorNoPrefixCache

        _orig_sig = inspect.signature(_orig)
        _noprefix_sig = inspect.signature(_NoPrefix)

        def _patched(kv_cache_config, max_model_len, *args, **kwargs):
            if len(kv_cache_config.kv_cache_groups) == 0:
                bound = _orig_sig.bind(kv_cache_config, max_model_len, *args, **kwargs)
                return _NoPrefix(**{
                    name: bound.arguments[name]
                    for name in _noprefix_sig.parameters
                    if name in bound.arguments
                })
            return _orig(kv_cache_config, max_model_len, *args, **kwargs)

        _kvc.get_kv_cache_coordinator = _patched

        # Make sibling-dir importable from a fresh subprocess too — vLLM's
        # inspect_model_cls runs in a child Python process that doesn't
        # inherit our sys.modules. Without this on PYTHONPATH the
        # string-spec model registration below can't be resolved.
        if current_dir not in sys.path:
            sys.path.insert(0, current_dir)
        existing = os.environ.get("PYTHONPATH", "")
        if current_dir not in existing.split(os.pathsep):
            os.environ["PYTHONPATH"] = (
                current_dir if not existing else current_dir + os.pathsep + existing
            )

        if pkg:
            _lla = importlib.import_module(".vllm_llava_eurobert_audio", package=pkg)
            _omni = importlib.import_module(".vllm_jina_v5_omni", package=pkg)
        else:
            _lla = importlib.import_module("vllm_llava_eurobert_audio")
            _omni = importlib.import_module("vllm_jina_v5_omni")
        _ = _lla.LlavaEuroBertAudioForVLLMEmbedding  # keep reference

        ModelRegistry = importlib.import_module(
            "vllm.model_executor.models"
        ).ModelRegistry
        # String spec ("module:Class") — survives vLLM's cloudpickle-into-
        # subprocess flow because the child re-imports by name. Passing the
        # class object directly registers __module__ as the qualified
        # transformers_modules.jinaai.<...> path, which the subprocess
        # can't resolve without HF's dynamic-module setup.
        ModelRegistry.register_model(
            "JinaEmbeddingsV5OmniModel",
            "vllm_jina_v5_omni:JinaV5OmniForVLLMEmbedding",
        )
    except Exception as e:
        import warnings
        warnings.warn(
            f"jina-embeddings-v5-omni base: vLLM registration failed "
            f"({type(e).__name__}: {e}); embeddings will fall back to "
            f"vLLM's generic transformers backend (wrong tensor layout).",
            stacklevel=2,
        )


_register_vllm()
