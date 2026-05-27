"""
Unified jina-embeddings-v5-omni-small: vision + audio + text with task-specific routing.

Shared:  Qwen3VL vision encoder + Qwen2.5-Omni audio encoder + Qwen3 LLM
Per-task: vision merger, audio projector, special token embeddings, LoRA adapter

The LLM uses standard 1D RoPE (not M-RoPE). When position_ids are not provided,
sequential 1D positions are computed and expanded to the 3-dim M-RoPE format
(all 3 dims identical), making it equivalent to standard RoPE.

Usage:
    model = AutoModel.from_pretrained("jinaai/jina-embeddings-v5-omni-small", trust_remote_code=True)
    embeddings = model.encode(["hello world"], task="retrieval")

Selective modality loading:
    model = AutoModel.from_pretrained("jinaai/jina-embeddings-v5-omni-small", trust_remote_code=True, modality="vision")
"""

from typing import List, Optional
import os

import torch
import torch.nn as nn
import torch.nn.functional as F

from huggingface_hub import snapshot_download
from transformers import AutoTokenizer, PreTrainedModel, PretrainedConfig
from transformers import Qwen3VLConfig, Qwen3VLForConditionalGeneration
from transformers.modeling_outputs import BaseModelOutputWithPast
from transformers.models.qwen2_5_omni.configuration_qwen2_5_omni import Qwen2_5OmniAudioEncoderConfig
from transformers.models.qwen2_5_omni.modeling_qwen2_5_omni import Qwen2_5OmniAudioEncoder
from peft import PeftMixedModel, PeftConfig

TASK_NAMES = ["retrieval", "text-matching", "clustering", "classification"]
_VALID_MODALITIES = {"omni", "vision", "audio", "text"}


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
        image_token_id=None,
        video_token_id=None,
        vision_start_token_id=None,
        vision_end_token_id=None,
        audio_token_id=None,
        audio_start_token_id=None,
        audio_end_token_id=None,
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
        self.image_token_id = image_token_id
        self.video_token_id = video_token_id
        self.vision_start_token_id = vision_start_token_id
        self.vision_end_token_id = vision_end_token_id
        self.audio_token_id = audio_token_id
        self.audio_start_token_id = audio_start_token_id
        self.audio_end_token_id = audio_end_token_id
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

    def __init__(self, config: JinaEmbeddingsV5OmniConfig):
        super().__init__(config)

        self._modality = getattr(config, "modality", "omni")
        if self._modality not in _VALID_MODALITIES:
            raise ValueError(f"modality must be one of {_VALID_MODALITIES}, got '{self._modality}'")

        vis_cfg = config.vision_config
        txt_cfg = config.text_config
        aud_cfg = config.audio_config

        vis_dict = vis_cfg.to_dict() if hasattr(vis_cfg, "to_dict") else vis_cfg
        txt_dict = txt_cfg.to_dict() if hasattr(txt_cfg, "to_dict") else txt_cfg
        aud_dict = aud_cfg.to_dict() if hasattr(aud_cfg, "to_dict") else aud_cfg

        rope_params = txt_dict.get("rope_parameters", {})
        rope_theta = rope_params.get("rope_theta", 3500000)
        rope_cfg = {
            "rope_type": "default",
            "rope_theta": rope_theta,
            "mrope_section": [24, 20, 20],
        }
        txt_dict["rope_scaling"] = rope_cfg
        txt_dict["rope_parameters"] = rope_cfg

        spatial_merge_size = 2
        vis_hidden = vis_dict.get("hidden_size", 1024)
        text_hidden = txt_dict.get("hidden_size", 1024)

        qwen3vl_config = Qwen3VLConfig(
            vision_config=vis_dict,
            text_config=txt_dict,
            tie_word_embeddings=config.tie_word_embeddings,
            image_token_id=config.image_token_id,
            video_token_id=config.video_token_id,
            vision_start_token_id=config.vision_start_token_id,
            vision_end_token_id=config.vision_end_token_id,
        )
        qwen3vl = Qwen3VLForConditionalGeneration(qwen3vl_config)
        m = qwen3vl.model
        self.language_model = m.language_model

        if self._modality not in ("audio", "text"):
            self.visual = m.visual
            self.visual.merger = nn.Identity()
            spatial_merge_size = getattr(self.visual, "spatial_merge_size", 2)
            self.mergers = nn.ModuleDict({
                _key(t): PretrainedMerger(vis_hidden, text_hidden, spatial_merge_size)
                for t in config.task_names
            })

        del qwen3vl

        if self._modality not in ("vision", "text"):
            audio_encoder_config = Qwen2_5OmniAudioEncoderConfig(**aud_dict)
            self.audio_tower = Qwen2_5OmniAudioEncoder(audio_encoder_config)
            self.audio_tower.proj = nn.Identity()  # fused into audio_projector(s)
            output_dim = aud_dict.get('d_model', 1280)  # fused: audio_projector(s) now take d_model
            self.audio_projectors = nn.ModuleDict({
                _key(t): nn.Linear(output_dim, text_hidden) for t in config.task_names
            })

        n_special = len(config.special_token_ids)
        self.task_token_embeddings = nn.ParameterDict({
            _key(t): nn.Parameter(torch.zeros(n_special, text_hidden))
            for t in config.task_names
        })

        ignore = []
        if self._modality in ("audio", "text"):
            ignore.append("visual.")
            ignore.append("mergers.")
        if self._modality in ("vision", "text"):
            ignore.append("audio_tower.")
            ignore.append("audio_projectors.")
        if ignore:
            self._keys_to_ignore_on_load_unexpected = ignore

        self._active_task_key = _key(config.task_names[0])
        self._special_token_ids = config.special_token_ids
        self._vllm_adapter_applied = False
        self._vllm_pending_deltas: Optional[dict] = None
        self._vllm_pending_task: Optional[str] = None
        self.post_init()

        # Pre-load the LoRA adapter from disk NOW (at __init__ time). The
        # actual merge is deferred to load_weights() (called by vLLM's
        # AutoWeightsLoader after parallel-linear swap and weight load).
        # A forward pre-hook would be traced by Dynamo in fullgraph mode
        # and crash on the RemovableHandle reference.
        self._prefetch_vllm_adapter_deltas()

    def _prefetch_vllm_adapter_deltas(self) -> None:
        """Read LoRA adapter from disk and compute per-proj deltas.

        Stores pre-computed `delta = (B @ A) * scale` tensors in
        `self._vllm_pending_deltas`. All filesystem I/O (snapshot_download,
        os.path, json, safe_open) happens here at init time, never inside the
        pre-hook that runs during the first compiled forward.
        """
        # Skip if PeftMixedModel wrapper is active (peft injects lora_A on
        # wrapped linears in that path).
        try:
            q_proj = self.language_model.layers[0].self_attn.q_proj
        except (AttributeError, IndexError):
            return
        if hasattr(q_proj, "lora_A"):
            return

        model_path = getattr(self.config, "_name_or_path", None)
        if not model_path:
            return
        # Task precedence: config.task (hf_overrides) > env var > default.
        task = getattr(self.config, "task", None)
        if task is None:
            task = os.environ.get("JINA_V5_TASK")
        if task is None:
            task = self.config.task_names[0]
        if task not in self.config.task_names:
            raise ValueError(f"task must be one of {self.config.task_names}, got '{task}'")

        if os.path.isdir(model_path):
            adapter_dir = os.path.join(model_path, "adapters", task)
        else:
            adapter_dir = os.path.join(
                snapshot_download(repo_id=model_path, allow_patterns=["adapters/*"]),
                "adapters", task,
            )
        if not os.path.isdir(adapter_dir):
            return

        import json
        from safetensors import safe_open
        with open(os.path.join(adapter_dir, "adapter_config.json")) as f:
            adapter_cfg = json.load(f)
        scale = adapter_cfg["lora_alpha"] / adapter_cfg["r"]
        with safe_open(os.path.join(adapter_dir, "adapter_model.safetensors"), framework="pt") as f:
            adapter = {k: f.get_tensor(k) for k in f.keys()}

        deltas: dict = {}
        for ak in list(adapter.keys()):
            if "lora_A" not in ak:
                continue
            bk = ak.replace("lora_A", "lora_B")
            if bk not in adapter:
                continue
            target_name = (
                ak.replace("base_model.model.", "")
                .replace(".lora_A.weight", ".weight")
            )
            # Store target_name (str) instead of a proj reference. vLLM's
            # transformers backend may swap the Linear modules with vLLM
            # parallel-linear variants AFTER our __init__ runs, leaving any
            # captured ref pointing at a now-orphan module whose weight no
            # longer participates in forward. Re-resolve from current state
            # at hook fire time.
            delta = (adapter[bk].float() @ adapter[ak].float()) * scale
            deltas[target_name] = delta

        self._vllm_pending_deltas = deltas
        self._vllm_pending_task = task

    def load_weights(self, weights):
        # Called by vLLM's AutoWeightsLoader. We delegate the standard load,
        # then merge the pre-fetched LoRA delta into the (now parallel-linear)
        # weights. Doing the merge here keeps it outside the compiled forward.
        # The import is wrapped in try/except ImportError so transformers'
        # static check_imports treats vllm as optional — non-vLLM users
        # (sentence-transformers, mteb, etc.) can still load the model.
        try:
            from vllm.model_executor.models.utils import AutoWeightsLoader
        except ImportError as e:
            raise ImportError(
                "load_weights() is for vLLM only — install vllm to use it."
            ) from e
        loader = AutoWeightsLoader(self)
        loaded_params = loader.load_weights(weights)
        self._apply_vllm_adapter_after_load()
        return loaded_params

    def _apply_vllm_adapter_after_load(self):
        if self._vllm_adapter_applied or self._vllm_pending_deltas is None:
            return
        self._vllm_adapter_applied = True
        with torch.no_grad():
            for target_name, delta in self._vllm_pending_deltas.items():
                parts = target_name.split(".")
                mod = self
                try:
                    for p in parts[:-2]:
                        mod = mod[int(p)] if p.isdigit() else getattr(mod, p)
                    proj = getattr(mod, parts[-2])
                    weight = proj.weight
                except AttributeError:
                    continue
                d = delta.to(device=weight.device, dtype=weight.dtype)
                weight.data.add_(d)
        self.set_task(self._vllm_pending_task)
        self._vllm_pending_deltas = None
        self._vllm_pending_task = None

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

    def get_image_features(self, pixel_values, image_grid_thw):
        if self._modality in ("audio", "text"):
            raise ValueError(f"Vision inputs not supported when modality='{self._modality}'")
        pixel_values = pixel_values.type(self.visual.dtype)
        out = self.visual(pixel_values, grid_thw=image_grid_thw)
        if isinstance(out, tuple):
            raw = out[0]
        elif hasattr(out, "last_hidden_state"):
            raw = out.last_hidden_state
        else:
            raw = out
        merged = self.mergers[self._active_task_key](raw)
        merge_sq = self.visual.spatial_merge_size ** 2
        split_sizes = (image_grid_thw.prod(-1) // merge_sq).tolist()
        return list(torch.split(merged, split_sizes))

    def get_audio_features(self, input_features, feature_attention_mask=None):
        if self._modality in ("vision", "text"):
            raise ValueError(f"Audio inputs not supported when modality='{self._modality}'")
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
        attention_mask=None,
        position_ids=None,
        pixel_values=None,
        pixel_values_videos=None,
        image_grid_thw=None,
        video_grid_thw=None,
        input_features=None,
        feature_attention_mask=None,
        inputs_embeds=None,
        output_hidden_states=None,
        **kwargs,
    ):
        if (input_ids is None) ^ (inputs_embeds is not None):
            raise ValueError("You must specify exactly one of input_ids or inputs_embeds")

        if inputs_embeds is None:
            inputs_embeds = self.get_input_embeddings()(input_ids)

        if pixel_values is not None and image_grid_thw is not None:
            feats = self.get_image_features(pixel_values, image_grid_thw)
            embeds = torch.cat(feats, dim=0).to(inputs_embeds.device, inputs_embeds.dtype)
            mask = (input_ids == self.config.image_token_id).unsqueeze(-1).expand_as(inputs_embeds)
            inputs_embeds = inputs_embeds.masked_scatter(mask, embeds)

        if pixel_values_videos is not None and video_grid_thw is not None:
            feats = self.get_image_features(pixel_values_videos, video_grid_thw)
            embeds = torch.cat(feats, dim=0).to(inputs_embeds.device, inputs_embeds.dtype)
            mask = (input_ids == self.config.video_token_id).unsqueeze(-1).expand_as(inputs_embeds)
            inputs_embeds = inputs_embeds.masked_scatter(mask, embeds)

        if input_features is not None:
            aud = self.get_audio_features(input_features, feature_attention_mask)
            aud_flat = aud.reshape(-1, aud.shape[-1]).to(inputs_embeds.device, inputs_embeds.dtype)
            mask = (input_ids == self.config.audio_token_id).unsqueeze(-1).expand_as(inputs_embeds)
            inputs_embeds = inputs_embeds.masked_scatter(mask, aud_flat)

        if position_ids is None:
            seq_len = inputs_embeds.shape[1]
            if attention_mask is not None:
                pos = attention_mask.long().cumsum(-1) - 1
                pos = pos.masked_fill(attention_mask == 0, 0)
            else:
                batch_size = inputs_embeds.shape[0]
                pos = torch.arange(seq_len, device=inputs_embeds.device).unsqueeze(0).expand(batch_size, -1)
            position_ids = pos.unsqueeze(0).expand(3, -1, -1).contiguous()

        out = self.language_model(
            inputs_embeds=inputs_embeds,
            attention_mask=attention_mask,
            position_ids=position_ids,
            output_hidden_states=output_hidden_states,
            **kwargs,
        )

        return BaseModelOutputWithPast(
            last_hidden_state=out[0],
            past_key_values=getattr(out, "past_key_values", None),
            hidden_states=getattr(out, "hidden_states", None),
            attentions=getattr(out, "attentions", None),
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

        base_model = JinaEmbeddingsV5OmniBase.from_pretrained(
            pretrained_model_name_or_path,
            config=config,
            torch_dtype=kwargs.pop("torch_dtype", kwargs.pop("dtype", torch.bfloat16)),
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
            raise ValueError(f"task must be one of {config.task_names}, got '{task}'")
        model.set_adapter(task)
        return model

    @classmethod
    def is_backend_compatible(cls) -> bool:
        # Signals vLLM to use the Transformers fallback path for this model.
        return True

    @classmethod
    def _from_config(cls, config, **kwargs):
        # vLLM's transformers-fallback path calls cls._from_config(config) to
        # get a "meta" skeleton. PeftMixedModel has no such hook; return a bare
        # JinaEmbeddingsV5OmniBase (the underlying PreTrainedModel) so that
        # vLLM can build the module graph, then we load real weights later.
        return JinaEmbeddingsV5OmniBase._from_config(config, **kwargs)

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


def _register_vllm() -> None:
    # When loaded via transformers' `trust_remote_code=True`, only the
    # modeling_*.py referenced in auto_map lands in the transformers_modules
    # cache — sibling vLLM adapter files are NOT. Pull them from HF Hub so
    # vLLM resolves our custom port instead of falling back to the generic
    # transformers backend (which OOMs on multimodal profiling for this arch).
    import importlib.util as _iu
    if _iu.find_spec("vllm") is None:
        return
    try:
        import os
        import sys
        import importlib
        import shutil

        pkg = __package__ or ""
        current_dir = os.path.dirname(os.path.abspath(__file__))
        siblings = ("vllm_qwen3vl_audio", "vllm_jina_v5_omni")

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

        if current_dir not in sys.path:
            sys.path.insert(0, current_dir)
        existing = os.environ.get("PYTHONPATH", "")
        if current_dir not in existing.split(os.pathsep):
            os.environ["PYTHONPATH"] = (
                current_dir if not existing else current_dir + os.pathsep + existing
            )

        if pkg:
            _qwen = importlib.import_module(".vllm_qwen3vl_audio", package=pkg)
            _omni = importlib.import_module(".vllm_jina_v5_omni", package=pkg)
        else:
            _qwen = importlib.import_module("vllm_qwen3vl_audio")
            _omni = importlib.import_module("vllm_jina_v5_omni")

        ModelRegistry = importlib.import_module(
            "vllm.model_executor.models"
        ).ModelRegistry
        # Register the class object directly (not the "module:Class" string).
        # The string form fails to resolve in vLLM worker subprocesses where
        # `vllm_jina_v5_omni` is not yet on sys.path, so vLLM silently falls
        # back to its generic transformers backend, which then reports every
        # adapter-merged key as "missing" at load time. Variant repos avoid
        # this by registering with the class object — mirror that here.
        ModelRegistry.register_model(
            "JinaEmbeddingsV5OmniModel",
            _omni.JinaV5OmniSmallForVLLMEmbedding,
        )
    except Exception as e:
        import warnings
        warnings.warn(
            f"jina-embeddings-v5-omni-small base: vLLM registration failed "
            f"({type(e).__name__}: {e}); embeddings will fall back to "
            f"vLLM's generic transformers backend (OOMs on profile).",
            stacklevel=2,
        )


_register_vllm()
