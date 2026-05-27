"""Jina v5 Omni Embedding HTTP service for MASTERd (port 11502).

Provides:
  POST /embed or /v1/embeddings — dense semantic embeddings
  GET  /health                  — health check
"""
from __future__ import annotations
import os
import torch
from pathlib import Path
from fastapi import FastAPI
from pydantic import BaseModel

def install_root() -> Path:
    """Resolve the packaged resource root for installed users and repo root for dev."""
    for env_key in ("MASTERD_RESOURCE_DIR", "MASTERD_INSTALL_ROOT"):
        value = os.environ.get(env_key)
        if value:
            return Path(value).expanduser().resolve()
    return Path(__file__).resolve().parents[2]


def resolve_install_path(rel_path: str) -> str:
    return str((install_root() / rel_path).resolve())


def resolve_model_asset(*parts: str) -> Path:
    model_root = Path(os.environ.get("MASTERD_MODELS_DIR", resolve_install_path("models")))
    return (model_root.joinpath(*parts)).expanduser().resolve()

# Set local Hugging Face cache root to keep the installation self-contained
if "HF_HOME" not in os.environ:
    os.environ["HF_HOME"] = resolve_install_path("models/.hf_cache")

app = FastAPI(title="MASTERd Jina v5 Omni Embedding Service")
_model = None
_tokenizer = None

DEVICE = "cpu"

JINA_V5_GGUF_ASSETS = {
    "nano_retrieval": resolve_model_asset("jina-v5-omni-nano-gguf", "retrieval", "model-Q4_K_M.gguf"),
    "nano_text_matching": resolve_model_asset("jina-v5-omni-nano-gguf", "text-matching", "model-Q4_K_M.gguf"),
    "small_retrieval": resolve_model_asset("jina-v5-omni-small-gguf", "retrieval", "model-Q4_K_M.gguf"),
    "small_text_matching": resolve_model_asset("jina-v5-omni-small-gguf", "text-matching", "model-Q4_K_M.gguf"),
}

MODEL_NAME = os.getenv("JINA_V5_MODEL", "")


@app.on_event("startup")
async def load_model() -> None:
    global _model, _tokenizer
    if not MODEL_NAME:
        return
    from transformers import AutoTokenizer, AutoModel
    _tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME, trust_remote_code=True)
    _model = AutoModel.from_pretrained(MODEL_NAME, trust_remote_code=True).to(DEVICE).eval()


class EmbedRequest(BaseModel):
    texts: list[str] = None
    input: list[str] = None
    task: str = "text-matching"
    max_length: int = 512
    model: str = None


@app.get("/health")
async def health() -> dict:
    gguf_assets = {name: str(path) for name, path in JINA_V5_GGUF_ASSETS.items()}
    missing = [name for name, path in JINA_V5_GGUF_ASSETS.items() if not path.is_file()]
    return {
        "status": "ok" if not missing else "degraded",
        "device": DEVICE,
        "model": MODEL_NAME or "gguf-assets-only",
        "install_root": str(install_root()),
        "gguf_assets": gguf_assets,
        "missing_gguf_assets": missing,
    }


@app.post("/embed")
@app.post("/v1/embeddings")
async def embed(req: EmbedRequest) -> dict:
    if _model is None:
        return {
            "error": "Jina v5 transformers runtime is not loaded; packaged GGUF assets are available for the GGUF runtime",
            "gguf_assets": {name: str(path) for name, path in JINA_V5_GGUF_ASSETS.items()},
        }
    texts = req.input if req.input is not None else req.texts
    if texts is None:
        return {"error": "either 'input' or 'texts' is required"}

    with torch.no_grad():
        if hasattr(_model, "encode"):
            vecs = _model.encode(texts, task=req.task, max_length=req.max_length)
            if not isinstance(vecs, torch.Tensor):
                vecs = torch.tensor(vecs)
            vecs = torch.nn.functional.normalize(vecs, dim=-1)
        else:
            enc = _tokenizer(
                texts, padding=True, truncation=True, max_length=req.max_length, return_tensors="pt"
            ).to(DEVICE)
            out = _model(**enc)
            mask = enc["attention_mask"].unsqueeze(-1).float()
            vecs = (out.last_hidden_state * mask).sum(1) / mask.sum(1)
            vecs = torch.nn.functional.normalize(vecs, dim=-1)
    
    embeddings_list = vecs.cpu().tolist()
    data = [{"embedding": e, "index": idx, "object": "embedding"} for idx, e in enumerate(embeddings_list)]
    return {
        "embeddings": embeddings_list,
        "dim": vecs.shape[-1],
        "data": data,
        "model": MODEL_NAME,
        "object": "list"
    }


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=11502)
