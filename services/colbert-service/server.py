"""ColBERT token-matrix HTTP service for MASTERd (port 11450).

Provides:
  POST /embed   — returns token-level embeddings for a batch of texts
  POST /rerank or /v1/rerank  — ColBERT MaxSim reranking for (query, [docs]) pairs
  GET  /health  — health check
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

app = FastAPI(title="MASTERd ColBERT Service")
_model = None
_tokenizer = None

DEVICE = "cpu"

local_model = resolve_install_path("models/lfm2-colbert-350m")
if os.path.isdir(local_model) and os.path.exists(os.path.join(local_model, "tokenizer.json")):
    DEFAULT_MODEL = local_model
else:
    DEFAULT_MODEL = "colbert-ir/colbertv2.0"

MODEL_NAME = os.getenv("COLBERT_MODEL", DEFAULT_MODEL)


@app.on_event("startup")
async def load_model() -> None:
    global _model, _tokenizer
    from transformers import AutoTokenizer, AutoModel
    
    local_tokenizer_path = resolve_install_path("models/lfm2-colbert-350m")
    if os.path.isdir(local_tokenizer_path) and os.path.exists(os.path.join(local_tokenizer_path, "tokenizer.json")):
        tokenizer_name = local_tokenizer_path
    else:
        tokenizer_name = MODEL_NAME

    _tokenizer = AutoTokenizer.from_pretrained(tokenizer_name)
    _model = AutoModel.from_pretrained(MODEL_NAME).to(DEVICE).eval()


class EmbedRequest(BaseModel):
    texts: list[str]
    max_length: int = 128


class RerankRequest(BaseModel):
    query: str
    documents: list[str]
    top_k: int = 10


@app.get("/health")
async def health() -> dict:
    return {"status": "ok", "device": DEVICE, "model": MODEL_NAME}


@app.post("/embed")
async def embed(req: EmbedRequest) -> dict:
    assert _model is not None, "Model not loaded"
    enc = _tokenizer(
        req.texts,
        padding=True,
        truncation=True,
        max_length=req.max_length,
        return_tensors="pt",
    ).to(DEVICE)
    with torch.no_grad():
        out = _model(**enc)
    vecs = out.last_hidden_state  # (B, T, D)
    return {"embeddings": vecs.cpu().tolist()}


@app.post("/rerank")
@app.post("/v1/rerank")
async def rerank(req: RerankRequest) -> dict:
    assert _model is not None, "Model not loaded"
    enc_q = _tokenizer(
        req.query, return_tensors="pt", truncation=True, max_length=32
    ).to(DEVICE)
    enc_d = _tokenizer(
        req.documents, return_tensors="pt", padding=True, truncation=True, max_length=128
    ).to(DEVICE)
    with torch.no_grad():
        q_vecs = _model(**enc_q).last_hidden_state[0]  # (Tq, D)
        d_vecs = _model(**enc_d).last_hidden_state      # (B, Td, D)
    q_vecs = torch.nn.functional.normalize(q_vecs, dim=-1)
    d_vecs = torch.nn.functional.normalize(d_vecs, dim=-1)
    scores = (q_vecs.unsqueeze(0) @ d_vecs.permute(0, 2, 1)).max(dim=-1).values.sum(dim=-1)
    
    ranked = torch.argsort(scores, descending=True)[: req.top_k].cpu().tolist()
    scores_list = scores[ranked].cpu().tolist()
    
    # Expose both custom array and OpenAI-compatible results list structure for Rust client compatibility
    results = [{"index": idx, "relevance_score": s, "score": s} for idx, s in zip(ranked, scores_list)]
    
    return {
        "ranked_indices": ranked,
        "scores": scores_list,
        "results": results
    }


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=11450)
