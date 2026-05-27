"""vLLM implementation for jina-embeddings-v5-omni-nano / -small base models.

The base models expose:
  - shared LM + vision + audio weights,
  - a per-task LoRA adapter (in adapters/{task}/adapter_model.safetensors),
  - a per-task PretrainedMerger (vision projection),
  - a per-task audio_projector,
  - per-task extra token embeddings applied to language_model.embed_tokens.

vLLM requires a concrete static model at load time, so we resolve the task from
the environment variable JINA_V5_TASK (default: retrieval). At load_weights time
we read the base safetensors + the selected adapter, merge LoRA into Q/K/V/O and
gate/up/down projections, rename the task-specific mergers/projectors/token
embeddings to their singular form, and stream the resulting state dict into the
existing LlavaEuroBertAudioForVLLMEmbedding weight loader — producing a forward
that is identical to the jinaai/jina-embeddings-v5-omni-nano-{task} variant.

One task per vLLM instance; spawn separate servers for multi-task serving.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Iterable

import torch
from safetensors import safe_open

try:
    # Package import — works when HF dynamic-module-loader places this
    # under transformers_modules.<...>.
    from .vllm_llava_eurobert_audio import LlavaEuroBertAudioForVLLMEmbedding
except ImportError:
    # Top-level import — works when this dir was added to PYTHONPATH
    # (e.g. by vLLM's spawn child during inspect_model_cls).
    from vllm_llava_eurobert_audio import LlavaEuroBertAudioForVLLMEmbedding


_TASK_KEY_MAP = {
    "retrieval": "retrieval",
    "text-matching": "text_matching",
    "clustering": "clustering",
    "classification": "classification",
}
_ATTN_MODULES = {"q_proj", "k_proj", "v_proj", "o_proj"}
_MLP_MODULES = {"gate_proj", "up_proj", "down_proj"}


def _resolve_local_dir(model_path: str) -> Path:
    if os.path.isdir(model_path):
        return Path(model_path)
    from huggingface_hub import snapshot_download
    return Path(snapshot_download(
        repo_id=model_path,
        allow_patterns=["model.safetensors", "config.json", "adapters/*"],
    ))


def _lora_target_key(layer_idx: int, module: str, side: str) -> str:
    parent = "self_attn" if module in _ATTN_MODULES else "mlp"
    return (
        f"base_model.model.language_model.layers.{layer_idx}."
        f"{parent}.{module}.lora_{side}.weight"
    )


def _materialize_task(base_dir: Path, task: str) -> dict[str, torch.Tensor]:
    task_key = _TASK_KEY_MAP[task]
    lora_dir = base_dir / "adapters" / task

    base_cfg = json.loads((base_dir / "config.json").read_text())
    special_tokens: list[int] = base_cfg["special_token_ids"]
    adapter_cfg = json.loads((lora_dir / "adapter_config.json").read_text())
    scale = adapter_cfg["lora_alpha"] / adapter_cfg["r"]

    with safe_open(str(base_dir / "model.safetensors"), framework="pt") as f:
        base = {k: f.get_tensor(k) for k in f.keys()}
    with safe_open(str(lora_dir / "adapter_model.safetensors"), framework="pt") as f:
        adapter = {k: f.get_tensor(k) for k in f.keys()}

    merged: dict[str, torch.Tensor] = {}

    for key, tensor in base.items():
        if key.startswith("language_model.layers."):
            parts = key.split(".")
            # language_model.layers.{i}.{self_attn|mlp}.{module}.weight
            if len(parts) == 6 and parts[-1] == "weight":
                layer_idx = int(parts[2])
                parent = parts[3]
                module = parts[4]
                if (parent == "self_attn" and module in _ATTN_MODULES) or (
                    parent == "mlp" and module in _MLP_MODULES
                ):
                    ak = _lora_target_key(layer_idx, module, "A")
                    bk = _lora_target_key(layer_idx, module, "B")
                    a = adapter.get(ak)
                    b = adapter.get(bk)
                    if a is not None and b is not None:
                        delta = (b.to(torch.float32) @ a.to(torch.float32)) * scale
                        tensor = (tensor.to(torch.float32) + delta).to(tensor.dtype)
            merged[key] = tensor

        elif key == "language_model.embed_tokens.weight":
            tensor = tensor.clone()
            te_key = f"task_token_embeddings.{task_key}"
            te = base.get(te_key)
            if te is not None:
                for i, tid in enumerate(special_tokens):
                    tensor[tid] = te[i].to(tensor.dtype)
            merged[key] = tensor

        elif key.startswith("mergers."):
            prefix = f"mergers.{task_key}."
            if key.startswith(prefix):
                merged["merger." + key[len(prefix):]] = tensor

        elif key.startswith("audio_projectors."):
            prefix = f"audio_projectors.{task_key}."
            if key.startswith(prefix):
                merged["audio_projector." + key[len(prefix):]] = tensor

        elif key.startswith("task_token_embeddings."):
            # Consumed into embed_tokens above.
            pass

        else:
            merged[key] = tensor

    return merged


class JinaV5OmniForVLLMEmbedding(LlavaEuroBertAudioForVLLMEmbedding):
    """vLLM wrapper for the base jina-embeddings-v5-omni-{nano,small}.

    Reads JINA_V5_TASK env var; merges base + adapter[task] + task components at
    load time. Resulting forward equals the jinaai/jina-embeddings-v5-omni-*-{task}
    task variant.
    """

    def __init__(self, *, vllm_config, prefix: str = ""):
        super().__init__(vllm_config=vllm_config, prefix=prefix)
        model = getattr(vllm_config.model_config, "model", None)
        if not isinstance(model, str):
            raise RuntimeError(
                "JinaV5OmniForVLLMEmbedding requires a string model path; got "
                f"{type(model).__name__}"
            )
        self._base_dir = _resolve_local_dir(model)

    def load_weights(self, weights: Iterable[tuple[str, torch.Tensor]]) -> set[str]:
        # Task precedence: config.task (hf_overrides) > env var. No silent
        # fallback — running base+vLLM without picking a task would embed
        # with the wrong adapter.
        task = getattr(self.config, "task", None)
        if task is None:
            task = os.environ.get("JINA_V5_TASK")
        if task is None:
            raise ValueError(
                "JinaV5OmniForVLLMEmbedding requires a task selection. Pass "
                "hf_overrides={'task': X} to LLM(...) or set JINA_V5_TASK=X "
                "in the environment, where X is one of "
                f"{sorted(_TASK_KEY_MAP)}."
            )
        if task not in _TASK_KEY_MAP:
            raise ValueError(
                f"task must be one of {sorted(_TASK_KEY_MAP)}, got '{task}'"
            )
        # The streamed `weights` arg only covers base model.safetensors; we need
        # the adapters too, so we materialize from disk directly and discard the
        # incoming stream.
        for _ in weights:
            pass
        materialized = _materialize_task(self._base_dir, task)
        return super().load_weights(iter(materialized.items()))
