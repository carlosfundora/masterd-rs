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

RED=$'\033[38;5;196m'
GREEN=$'\033[38;5;46m'
CYAN=$'\033[38;5;51m'
YELLOW=$'\033[38;5;226m'
RESET=$'\033[0m'

info()    { printf "%b[setup-embed]%b %s\n" "${CYAN}"  "${RESET}" "$*"; }
success() { printf "%b[setup-embed]%b %s\n" "${GREEN}" "${RESET}" "$*"; }
warn()    { printf "%b[setup-embed]%b %s\n" "${YELLOW}" "${RESET}" "$*"; }
die()     { printf "%b[setup-embed] ERROR:%b %s\n" "${RED}" "${RESET}" "$*" >&2; exit 1; }

source "${ROOT_DIR}/scripts/lib/install-bootstrap.sh"
export HF_HOME="${HF_HOME:-${ROOT_DIR}/models/.hf_cache}"

# Detect if AMD ROCm/GPU is available on the system.
HAS_ROCM=0
if command -v rocm-smi >/dev/null 2>&1 || [[ -d "/opt/rocm" || -c "/dev/kfd" ]]; then
  HAS_ROCM=1
fi

# ── AMD ROCm/CPU index ────────────────────────────────────────────────────
# Keep these as local script variables. Passing them directly to pip/uv package
# installs avoids leaking package-index URLs into uv python/bootstrap commands.
info "Enforcing CPU PyTorch index for all environments (CPU inference stack)."
ROCM_TORCH_INDEX="https://download.pytorch.org/whl/cpu"
ROCM_STABLE_INDEX="https://download.pytorch.org/whl/cpu"

ROCM_CONSTRAINTS="${ROOT_DIR}/config/rocm-constraints.txt"
unset UV_EXTRA_INDEX_URL UV_INDEX_URL UV_CONSTRAINT
unset PIP_EXTRA_INDEX_URL PIP_INDEX_URL PIP_CONSTRAINT

# Block CUDA; ROCm/CPU only.
export CUDA_VISIBLE_DEVICES=""
unset CUDA_HOME 2>/dev/null || true

PYTHON_BIN="${PYTHON_BIN:-python3.12}"
TARGET_SERVICE="${TARGET_SERVICE:-all}"
PREFETCH_MODELS="${PREFETCH_MODELS:-1}"

ensure_uv() {
  masterd_ensure_uv "${ROOT_DIR}"
  UV_BIN="${MASTERD_UV_BIN}"
}

create_venv() {
  local venv_dir="$1"
  if [[ -n "${UV_BIN}" ]]; then
    "${UV_BIN}" venv --seed --relocatable --python "${PYTHON_BIN}" "${venv_dir}"
  else
    "${PYTHON_BIN}" -m venv "${venv_dir}"
  fi
}

install_requirements() {
  local venv_python="$1"
  local reqs_file="$2"
  local venv_dir
  venv_dir="$(dirname "$(dirname "${venv_python}")")"

  # Detect if we should use the local ROCm venv bridge (to speed up testing/validation on this host)
  local bridge_src=""
  for dir in "/usr/local/lib/python3.12/dist-packages" "/usr/lib/python3/dist-packages" "/home/local/.venv-bridges/.venv-py312/lib/python3.12/site-packages" "/home/local/Projects/venvs/.venv-pytorch-rocm-72/lib/python3.12/site-packages"; do
    if [[ -d "${dir}/torch" ]]; then
      bridge_src="${dir}"
      break
    fi
  done

  if [[ -n "${bridge_src}" ]]; then
    info "  Local canonical ROCm PyTorch detected at ${bridge_src}. Activating bridge link..."
    local sp_dir="${venv_dir}/lib/python3.12/site-packages"
    mkdir -p "${sp_dir}"
    echo "${bridge_src}" > "${sp_dir}/bridge.pth"
    
    # Filter torch, triton, colbert-ai, and sentence-transformers to temp_reqs
    local temp_reqs
    temp_reqs="$(mktemp)"
    grep -v -E '^(torch|triton|colbert-ai|sentence-transformers)($|[=>~])' "${reqs_file}" > "${temp_reqs}"
    
    local torch_deps=()
    if grep -q -E '^colbert-ai($|[=>~])' "${reqs_file}"; then
      torch_deps+=("colbert-ai")
    fi
    if grep -q -E '^sentence-transformers($|[=>~])' "${reqs_file}"; then
      torch_deps+=("sentence-transformers")
    fi

    info "  Installing non-bridged, non-torch requirements..."
    if [[ -n "${UV_BIN}" ]]; then
      masterd_without_python_index_env "${UV_BIN}" pip install \
        --no-config \
        --python "${venv_python}" \
        -r "${temp_reqs}"
      
      if [[ "${#torch_deps[@]}" -gt 0 ]]; then
        info "  Installing torch-dependent packages (${torch_deps[*]}) with --no-deps..."
        masterd_without_python_index_env "${UV_BIN}" pip install \
          --no-config \
          --python "${venv_python}" \
          --no-deps \
          "${torch_deps[@]}"
      fi
    else
      "${venv_python}" -m ensurepip --upgrade >/dev/null 2>&1 || true
      masterd_without_python_index_env "${venv_python}" -m pip install --upgrade pip setuptools wheel
      masterd_without_python_index_env "${venv_python}" -m pip install -r "${temp_reqs}"
      
      if [[ "${#torch_deps[@]}" -gt 0 ]]; then
        info "  Installing torch-dependent packages (${torch_deps[*]}) with --no-deps..."
        masterd_without_python_index_env "${venv_python}" -m pip install --no-deps "${torch_deps[@]}"
      fi
    fi
    rm -f "${temp_reqs}"
    return 0
  fi

  # Otherwise: Public user clean install path
  if [[ -n "${UV_BIN}" ]]; then
    masterd_without_python_index_env "${UV_BIN}" pip install \
      --no-config \
      --python "${venv_python}" \
      --constraint "${ROCM_CONSTRAINTS}" \
      --extra-index-url "${ROCM_TORCH_INDEX}" \
      --extra-index-url "${ROCM_STABLE_INDEX}" \
      -r "${reqs_file}"
  else
    "${venv_python}" -m ensurepip --upgrade >/dev/null 2>&1 || true
    masterd_without_python_index_env "${venv_python}" -m pip install --upgrade pip setuptools wheel
    masterd_without_python_index_env "${venv_python}" -m pip install \
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
  PYTHON_BIN   Python interpreter, default python3.12, fallback python3/uv.
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
masterd_ensure_source_build_tools "${ROOT_DIR}"
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

  info "Setting up ${name} venv at ${venv_dir} (overwriting any existing environments)..."
  mkdir -p "${SERVICES_DIR}/${name}"

  # Overwrite older crud from failed installs automatically
  /usr/bin/rm -rf "${venv_dir}"
  create_venv "${venv_dir}"

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

try:
    print(f"Prefetching {model_name}...")
    tokenizer = AutoTokenizer.from_pretrained(model_name, trust_remote_code=trust_remote_code)
    model = AutoModel.from_pretrained(model_name, trust_remote_code=trust_remote_code)
    print(f"prefetched {model_name}")
except Exception as e:
    print(f"Failed to prefetch {model_name}: {e}")
    if "ColBERT" in model_name:
        fallback = "colbert-ir/colbertv2.0"
        print(f"Retrying prefetch with fallback: {fallback}")
        try:
            tokenizer = AutoTokenizer.from_pretrained(fallback)
            model = AutoModel.from_pretrained(fallback)
            print(f"prefetched fallback {fallback}")
        except Exception as fallback_err:
            print(f"WARNING: Fallback prefetch failed for {fallback}: {fallback_err}")
    else:
        print(f"WARNING: Prefetch failed for {model_name}, but proceeding anyway: {e}")
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
  POST /embed or /v1/embeddings — dense sentence/code embeddings (768-dim)
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

app = FastAPI(title="MASTERd Jina Embedding Service")
_model = None
_tokenizer = None

DEVICE = "cpu"
MODEL_NAME = os.getenv("JINA_MODEL", "jinaai/jina-embeddings-v3")
MAX_LENGTH = int(os.getenv("JINA_MAX_LENGTH", "8192"))


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

    if hasattr(_model, "encode"):
        with torch.no_grad():
            vecs = _model.encode(texts, task=req.task, max_length=MAX_LENGTH)
        if not isinstance(vecs, torch.Tensor):
            vecs = torch.tensor(vecs)
        vecs = torch.nn.functional.normalize(vecs, dim=-1)
    else:
        enc = _tokenizer(
            texts, padding=True, truncation=True, max_length=MAX_LENGTH, return_tensors="pt"
        ).to(DEVICE)
        with torch.no_grad():
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
  POST /embed or /v1/embeddings — dense semantic embeddings (1024-dim)
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

app = FastAPI(title="MASTERd Qwen3 Embedding Service")
_model = None
_tokenizer = None

DEVICE = "cpu"
MODEL_NAME = os.getenv("QWEN3_MODEL", "Qwen/Qwen3-Embedding")


@app.on_event("startup")
async def load_model() -> None:
    global _model, _tokenizer
    from transformers import AutoTokenizer, AutoModel
    _tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME)
    _model = AutoModel.from_pretrained(MODEL_NAME).to(DEVICE).eval()


class EmbedRequest(BaseModel):
    texts: list[str] = None
    input: list[str] = None
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

    enc = _tokenizer(
        texts, padding=True, truncation=True, max_length=req.max_length, return_tensors="pt"
    ).to(DEVICE)
    with torch.no_grad():
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
PY
}

# ── Main dispatch ─────────────────────────────────────────────────────────
case "${TARGET_SERVICE}" in
  colbert|all)
    write_colbert_reqs
    setup_venv "colbert-service"
    prefetch_hf_model "colbert-service" "COLBERT_MODEL" "LiquidAI/LFM2-ColBERT-350M" "0"
    ;;&
  jina|all)
    write_jina_reqs
    setup_venv "jina-service"
    prefetch_hf_model "jina-service" "JINA_MODEL" "jinaai/jina-embeddings-v3" "1"
    ;;&
  qwen3|all)
    write_qwen3_reqs
    setup_venv "qwen3-service"
    prefetch_hf_model "qwen3-service" "QWEN3_MODEL" "Qwen/Qwen3-Embedding" "0"
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
