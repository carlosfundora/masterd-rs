"""Jina v5 Omni Embedding HTTP service for MASTERd (port 11502).

Provides:
  POST /embed or /v1/embeddings — dense semantic embeddings
  GET  /health                  — health check
"""
from __future__ import annotations
import os
import torch
from fastapi import FastAPI
from pydantic import BaseModel

# Abstract path resolution through an installation resolution layer
def resolve_install_path(rel_path: str) -> str:
    root_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
    return os.path.abspath(os.path.join(root_dir, rel_path))

# Set local Hugging Face cache root to keep the installation self-contained
if "HF_HOME" not in os.environ:
    os.environ["HF_HOME"] = resolve_install_path("models/.hf_cache")

app = FastAPI(title="MASTERd Jina v5 Omni Embedding Service")
_model = None
_tokenizer = None

DEVICE = "cpu"

local_model_small = resolve_install_path("models/jina-v5-omni-small")
local_model_nano = resolve_install_path("models/jina-v5-omni-nano")
if os.path.isdir(local_model_small) and os.path.exists(os.path.join(local_model_small, "config.json")):
    DEFAULT_MODEL = local_model_small
elif os.path.isdir(local_model_nano) and os.path.exists(os.path.join(local_model_nano, "config.json")):
    DEFAULT_MODEL = local_model_nano
else:
    DEFAULT_MODEL = "jinaai/jina-embeddings-v5-omni-nano"

MODEL_NAME = os.getenv("JINA_V5_MODEL", DEFAULT_MODEL)


@app.on_event("startup")
async def load_model() -> None:
    global _model, _tokenizer
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
    return {"status": "ok", "device": DEVICE, "model": MODEL_NAME}


@app.post("/embed")
@app.post("/v1/embeddings")
async def embed(req: EmbedRequest) -> dict:
    assert _model is not None, "Model not loaded"
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
