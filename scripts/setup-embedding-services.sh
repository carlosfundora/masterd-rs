#!/usr/bin/env bash
# setup-embedding-services.sh
#
# Creates isolated Python 3.12 venvs for each MASTERd embedding service and
# installs the required packages from the AMD ROCm PyTorch index.
#
# Services managed:
#   colbert-service  — ColBERT token-matrix reranker (port 11450)
#   jina-service     — Jina embeddings v3 (port 11447)
#   qwen3-service    — Qwen3-Embedding (port 11502)
#
# Prerequisites:
#   - Python 3.12 available
#   - uv is preferred; the script bootstraps it or falls back to venv+pip
#   - AMD ROCm 6.x or 7.x runtime (rocm-smi should show the GPU)
#
# Usage:
#   ./scripts/setup-embedding-services.sh [--service colbert|jina|qwen3|all]
#   ./scripts/setup-embedding-services.sh jina
#
# Environment overrides:
#   ROCM_TORCH_INDEX   — override the primary PyTorch ROCm index URL
#   PYTHON_BIN         — override the python interpreter (default: python3.12, fallback python3)
#   HF_TOKEN           — optional Hugging Face token for gated/private models
#   HF_HOME            — optional local Hugging Face cache root
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVICES_DIR="${ROOT_DIR}/services"

source "${ROOT_DIR}/scripts/lib/install-bootstrap.sh"

# ── AMD ROCm index (must be set before any uv/pip call) ──────────────────
ROCM_TORCH_INDEX="${ROCM_TORCH_INDEX:-https://download.pytorch.org/whl/nightly/rocm6.3}"
ROCM_STABLE_INDEX="${ROCM_STABLE_INDEX:-https://download.pytorch.org/whl/rocm6.2.4}"
ROCM_CONSTRAINTS="${ROOT_DIR}/config/rocm-constraints.txt"

export UV_EXTRA_INDEX_URL="${ROCM_TORCH_INDEX}"
export PIP_EXTRA_INDEX_URL="${ROCM_TORCH_INDEX}"
export PIP_CONSTRAINT="${ROCM_CONSTRAINTS}"
export UV_CONSTRAINT="${ROCM_CONSTRAINTS}"

# Block CUDA; ROCm only.
export CUDA_VISIBLE_DEVICES=""
unset CUDA_HOME 2>/dev/null || true

PYTHON_BIN="${PYTHON_BIN:-python3.12}"
TARGET_SERVICE="${TARGET_SERVICE:-all}"
PREFETCH_MODELS="${PREFETCH_MODELS:-1}"

RED=$'\033[38;5;196m'
GREEN=$'\033[38;5;46m'
CYAN=$'\033[38;5;51m'
YELLOW=$'\033[38;5;226m'
RESET=$'\033[0m'

info()    { printf "%b[setup-embed]%b %s\n" "${CYAN}"  "${RESET}" "$*"; }
success() { printf "%b[setup-embed]%b %s\n" "${GREEN}" "${RESET}" "$*"; }
warn()    { printf "%b[setup-embed]%b %s\n" "${YELLOW}" "${RESET}" "$*"; }
die()     { printf "%b[setup-embed] ERROR:%b %s\n" "${RED}" "${RESET}" "$*" >&2; exit 1; }

ensure_uv() {
  masterd_ensure_uv "${ROOT_DIR}"
  UV_BIN="${MASTERD_UV_BIN}"
}

create_venv() {
  local venv_dir="$1"
  if [[ -n "${UV_BIN}" ]]; then
    "${UV_BIN}" venv --python "${PYTHON_BIN}" "${venv_dir}"
  else
    "${PYTHON_BIN}" -m venv "${venv_dir}"
  fi
}

install_requirements() {
  local venv_python="$1"
  local reqs_file="$2"

  if [[ -n "${UV_BIN}" ]]; then
    "${UV_BIN}" pip install \
      --python "${venv_python}" \
      --constraint "${ROCM_CONSTRAINTS}" \
      --extra-index-url "${ROCM_TORCH_INDEX}" \
      --extra-index-url "${ROCM_STABLE_INDEX}" \
      -r "${reqs_file}"
  else
    "${venv_python}" -m ensurepip --upgrade >/dev/null 2>&1 || true
    "${venv_python}" -m pip install --upgrade pip setuptools wheel
    "${venv_python}" -m pip install \
      --constraint "${ROCM_CONSTRAINTS}" \
      --extra-index-url "${ROCM_TORCH_INDEX}" \
      --extra-index-url "${ROCM_STABLE_INDEX}" \
      -r "${reqs_file}"
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --service)
      [[ $# -ge 2 ]] || die "--service requires one of: colbert, jina, qwen3, all"
      TARGET_SERVICE="$2"
      shift 2
      ;;
    --no-prefetch)
      PREFETCH_MODELS=0
      shift
      ;;
    --help|-h)
      cat <<'EOF'
Usage:
  ./scripts/setup-embedding-services.sh [--service colbert|jina|qwen3|all] [--no-prefetch]
  ./scripts/setup-embedding-services.sh jina

Environment:
  HF_TOKEN     Optional Hugging Face token for gated/private models.
  HF_HOME      Optional local Hugging Face cache root.
  PYTHON_BIN   Python interpreter, default python3.12.
EOF
      exit 0
      ;;
    colbert|jina|qwen3|all)
      TARGET_SERVICE="$1"
      shift
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

case "${TARGET_SERVICE}" in
  colbert|jina|qwen3|all) ;;
  *) die "unknown service: ${TARGET_SERVICE}. Use colbert, jina, qwen3, or all." ;;
esac

# Validate environment before touching anything.
[[ -f "${ROCM_CONSTRAINTS}" ]]    || die "ROCm constraints file missing: ${ROCM_CONSTRAINTS}"
masterd_resolve_python "${ROOT_DIR}"
PYTHON_BIN="${MASTERD_PYTHON_BIN}"
UV_BIN=""
ensure_uv

PYTHON_VER="$("${PYTHON_BIN}" --version 2>&1)"
info "Using ${PYTHON_VER}"
info "ROCm PyTorch primary index: ${ROCM_TORCH_INDEX}"
info "CUDA_VISIBLE_DEVICES=${CUDA_VISIBLE_DEVICES} (disabled)"
info "Constraints: ${ROCM_CONSTRAINTS}"

# ── Helper: create venv and install packages ──────────────────────────────
setup_venv() {
  local name="$1"
  local venv_dir="${SERVICES_DIR}/${name}/.venv"
  local reqs_file="${SERVICES_DIR}/${name}/requirements.txt"

  info "Setting up ${name} venv at ${venv_dir}..."
  mkdir -p "${SERVICES_DIR}/${name}"

  if [[ ! -d "${venv_dir}" ]]; then
    create_venv "${venv_dir}"
  else
    warn "  venv already exists, skipping creation"
  fi

  if [[ -f "${reqs_file}" ]]; then
    info "  Installing from ${reqs_file} (ROCm index enforced)..."
    install_requirements "${venv_dir}/bin/python" "${reqs_file}"
    success "  ${name} packages installed."
  else
    warn "  No requirements.txt found at ${reqs_file} — skipping package install."
  fi
}

prefetch_hf_model() {
  local service_name="$1"
  local model_env_name="$2"
  local default_model="$3"
  local trust_remote_code="$4"
  local venv_python="${SERVICES_DIR}/${service_name}/.venv/bin/python"
  local model_name="${!model_env_name:-${default_model}}"

  if [[ "${PREFETCH_MODELS}" != "1" ]]; then
    warn "  model prefetch disabled for ${service_name}"
    return 0
  fi

  info "  Prefetching ${model_name} for ${service_name}..."
  MODEL_NAME="${model_name}" TRUST_REMOTE_CODE="${trust_remote_code}" "${venv_python}" - <<'PY'
import os
from transformers import AutoModel, AutoTokenizer

model_name = os.environ["MODEL_NAME"]
trust_remote_code = os.environ.get("TRUST_REMOTE_CODE") == "1"

tokenizer = AutoTokenizer.from_pretrained(model_name, trust_remote_code=trust_remote_code)
model = AutoModel.from_pretrained(model_name, trust_remote_code=trust_remote_code)

print(f"prefetched {model_name}")
print(f"tokenizer={type(tokenizer).__name__} model={type(model).__name__}")
PY
  success "  ${service_name} model cache ready."
}

# ── Write service requirements files (idempotent) ────────────────────────
write_colbert_reqs() {
  local dir="${SERVICES_DIR}/colbert-service"
  mkdir -p "${dir}"
  cat > "${dir}/requirements.txt" <<'REQS'
# ColBERT token-matrix reranker service
# Packages installed from ROCm PyTorch index (no CUDA wheels).
#
# Install via:
#   uv pip install -r requirements.txt \
#     --extra-index-url https://download.pytorch.org/whl/nightly/rocm6.3
#
# torch+rocm must be the FIRST extra-index-url hit; PyPI torch would resolve
# the CPU wheel which silently produces wrong results on AMD hardware.

torch
fastapi
uvicorn[standard]
colbert-ai
transformers
sentencepiece
huggingface-hub
safetensors
REQS

  cat > "${dir}/server.py" <<'PY'
"""ColBERT token-matrix HTTP service for MASTERd (port 11450).

Provides:
  POST /embed   — returns token-level embeddings for a batch of texts
  POST /rerank  — ColBERT MaxSim reranking for (query, [docs]) pairs
  GET  /health  — health check
"""
from __future__ import annotations
import os
import torch
from fastapi import FastAPI
from pydantic import BaseModel

app = FastAPI(title="MASTERd ColBERT Service")
_model = None
_tokenizer = None

DEVICE = "cuda" if torch.cuda.is_available() else "cpu"
MODEL_NAME = os.getenv("COLBERT_MODEL", "colbert-ir/colbertv2.0")


@app.on_event("startup")
async def load_model() -> None:
    global _model, _tokenizer
    from transformers import AutoTokenizer, AutoModel
    _tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME)
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
    return {"ranked_indices": ranked, "scores": scores[ranked].cpu().tolist()}


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=11450)
PY
}

write_jina_reqs() {
  local dir="${SERVICES_DIR}/jina-service"
  mkdir -p "${dir}"
  cat > "${dir}/requirements.txt" <<'REQS'
# Jina embeddings service for MASTERd (port 11447)
# Packages installed from ROCm PyTorch index (no CUDA wheels).
torch
fastapi
uvicorn[standard]
transformers
sentence-transformers
einops
sentencepiece
huggingface-hub
safetensors
REQS

  cat > "${dir}/server.py" <<'PY'
"""Jina Embeddings HTTP service for MASTERd (port 11447).

Provides:
  POST /embed   — dense sentence/code embeddings (768-dim)
  GET  /health  — health check
"""
from __future__ import annotations
import os
import torch
from fastapi import FastAPI
from pydantic import BaseModel

app = FastAPI(title="MASTERd Jina Embedding Service")
_model = None
_tokenizer = None

DEVICE = "cuda" if torch.cuda.is_available() else "cpu"
MODEL_NAME = os.getenv("JINA_MODEL", "jinaai/jina-embeddings-v3")
MAX_LENGTH = int(os.getenv("JINA_MAX_LENGTH", "8192"))


@app.on_event("startup")
async def load_model() -> None:
    global _model, _tokenizer
    from transformers import AutoTokenizer, AutoModel
    _tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME, trust_remote_code=True)
    _model = AutoModel.from_pretrained(MODEL_NAME, trust_remote_code=True).to(DEVICE).eval()


class EmbedRequest(BaseModel):
    texts: list[str]
    task: str = "text-matching"


@app.get("/health")
async def health() -> dict:
    return {"status": "ok", "device": DEVICE, "model": MODEL_NAME}


@app.post("/embed")
async def embed(req: EmbedRequest) -> dict:
    assert _model is not None, "Model not loaded"
    if hasattr(_model, "encode"):
        with torch.no_grad():
            vecs = _model.encode(req.texts, task=req.task, max_length=MAX_LENGTH)
        if not isinstance(vecs, torch.Tensor):
            vecs = torch.tensor(vecs)
        vecs = torch.nn.functional.normalize(vecs, dim=-1)
    else:
        enc = _tokenizer(
            req.texts, padding=True, truncation=True, max_length=MAX_LENGTH, return_tensors="pt"
        ).to(DEVICE)
        with torch.no_grad():
            out = _model(**enc)
        mask = enc["attention_mask"].unsqueeze(-1).float()
        vecs = (out.last_hidden_state * mask).sum(1) / mask.sum(1)
        vecs = torch.nn.functional.normalize(vecs, dim=-1)
    return {"embeddings": vecs.cpu().tolist(), "dim": vecs.shape[-1]}


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=11447)
PY
}

write_qwen3_reqs() {
  local dir="${SERVICES_DIR}/qwen3-service"
  mkdir -p "${dir}"
  cat > "${dir}/requirements.txt" <<'REQS'
# Qwen3-Embedding service for MASTERd (port 11502)
# Packages installed from ROCm PyTorch index (no CUDA wheels).
torch
fastapi
uvicorn[standard]
transformers
sentencepiece
huggingface-hub
safetensors
REQS

  cat > "${dir}/server.py" <<'PY'
"""Qwen3-Embedding HTTP service for MASTERd (port 11502).

Provides:
  POST /embed   — dense semantic embeddings (1024-dim)
  GET  /health  — health check
"""
from __future__ import annotations
import os
import torch
from fastapi import FastAPI
from pydantic import BaseModel

app = FastAPI(title="MASTERd Qwen3 Embedding Service")
_model = None
_tokenizer = None

DEVICE = "cuda" if torch.cuda.is_available() else "cpu"
MODEL_NAME = os.getenv("QWEN3_MODEL", "Qwen/Qwen3-Embedding")


@app.on_event("startup")
async def load_model() -> None:
    global _model, _tokenizer
    from transformers import AutoTokenizer, AutoModel
    _tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME)
    _model = AutoModel.from_pretrained(MODEL_NAME).to(DEVICE).eval()


class EmbedRequest(BaseModel):
    texts: list[str]
    max_length: int = 512


@app.get("/health")
async def health() -> dict:
    return {"status": "ok", "device": DEVICE, "model": MODEL_NAME}


@app.post("/embed")
async def embed(req: EmbedRequest) -> dict:
    assert _model is not None, "Model not loaded"
    enc = _tokenizer(
        req.texts, padding=True, truncation=True, max_length=req.max_length, return_tensors="pt"
    ).to(DEVICE)
    with torch.no_grad():
        out = _model(**enc)
    mask = enc["attention_mask"].unsqueeze(-1).float()
    vecs = (out.last_hidden_state * mask).sum(1) / mask.sum(1)
    vecs = torch.nn.functional.normalize(vecs, dim=-1)
    return {"embeddings": vecs.cpu().tolist(), "dim": vecs.shape[-1]}


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=11502)
PY
}

# ── Main dispatch ─────────────────────────────────────────────────────────
case "${TARGET_SERVICE}" in
  colbert|all)
    write_colbert_reqs
    setup_venv "colbert-service"
    ;;&
  jina|all)
    write_jina_reqs
    setup_venv "jina-service"
    prefetch_hf_model "jina-service" "JINA_MODEL" "jinaai/jina-embeddings-v3" "1"
    ;;&
  qwen3|all)
    write_qwen3_reqs
    setup_venv "qwen3-service"
    ;;&
esac

success "Embedding service environments ready."
info ""
info "To start services individually:"
info "  ${SERVICES_DIR}/colbert-service/.venv/bin/python ${SERVICES_DIR}/colbert-service/server.py"
info "  ${SERVICES_DIR}/jina-service/.venv/bin/python    ${SERVICES_DIR}/jina-service/server.py"
info "  ${SERVICES_DIR}/qwen3-service/.venv/bin/python   ${SERVICES_DIR}/qwen3-service/server.py"
info ""
info "ROCm enforcement:"
info "  Index   : ${ROCM_TORCH_INDEX}"
info "  Blocked : config/rocm-constraints.txt"
info "  CUDA    : DISABLED (CUDA_VISIBLE_DEVICES='')"
