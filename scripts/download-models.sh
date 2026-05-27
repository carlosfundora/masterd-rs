#!/usr/bin/env bash
# download-models.sh
#
# Downloads all MASTERd GGUF model weights from Hugging Face and places them
# in the correct locations for the build and embedded inference engine.
#
# Run this after cloning the repo, before building.
#
# Usage:
#   ./scripts/download-models.sh [--skip-chat] [--skip-colbert] [--force] [--verify-only]
#
# Requirements:
#   curl  (already on most Linux systems)
#   — OR — huggingface-hub Python package:
#     pip install huggingface-hub   (or uv pip install huggingface-hub)
#
# To use a private HuggingFace token (for gated models):
#   export HF_TOKEN=hf_your_token_here
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

RED=$'\033[38;5;196m'
GREEN=$'\033[38;5;46m'
CYAN=$'\033[38;5;51m'
YELLOW=$'\033[38;5;226m'
RESET=$'\033[0m'

info()    { printf "%b[download-models]%b %s\n" "${CYAN}"   "${RESET}" "$*"; }
success() { printf "%b[download-models]%b %s\n" "${GREEN}"  "${RESET}" "$*"; }
warn()    { printf "%b[download-models]%b %s\n" "${YELLOW}" "${RESET}" "$*"; }
die()     { printf "%b[download-models] ERROR:%b %s\n" "${RED}" "${RESET}" "$*" >&2; exit 1; }

SKIP_CHAT="${SKIP_CHAT:-0}"
SKIP_COLBERT="${SKIP_COLBERT:-0}"
HF_TOKEN="${HF_TOKEN:-}"
FORCE="${FORCE:-0}"
VERIFY_ONLY="${VERIFY_ONLY:-0}"

for arg in "$@"; do
  case "${arg}" in
    --skip-chat)    SKIP_CHAT=1 ;;
    --skip-colbert) SKIP_COLBERT=1 ;;
    --force)        FORCE=1 ;;
    --verify-only)  VERIFY_ONLY=1 ;;
    --help|-h)
      echo "Usage: $0 [--skip-chat] [--skip-colbert] [--force] [--verify-only]"
      echo "  --skip-chat     Skip downloading chat models (thinking + instruct)"
      echo "  --skip-colbert  Skip downloading ColBERT reranker model"
      echo "  --force         Re-download files even if they already exist"
      echo "  --verify-only   Only verify local model files; do not download"
      echo ""
      echo "Set HF_TOKEN env var for gated models:"
      echo "  export HF_TOKEN=hf_your_token_here"
      exit 0 ;;
    *)
      die "unknown argument: ${arg}" ;;
  esac
done

is_lfs_pointer() {
  local path="$1"
  [[ -f "${path}" ]] && head -c 128 "${path}" | grep -q "https://git-lfs.github.com/spec/v1"
}

file_size() {
  stat -c%s "$1"
}

verify_file() {
  local path="$1"
  local min_bytes="$2"
  local label="$3"

  if [[ ! -f "${path}" ]]; then
    die "missing ${label}: ${path}"
  fi
  if is_lfs_pointer "${path}"; then
    die "${label} is a Git LFS pointer, not the real model file: ${path}"
  fi
  local size
  size="$(file_size "${path}")"
  if (( size < min_bytes )); then
    die "${label} is too small (${size} bytes, expected at least ${min_bytes}): ${path}"
  fi
}

verify_optional_file() {
  local path="$1"
  local min_bytes="$2"
  local label="$3"

  if [[ -f "${path}" ]]; then
    verify_file "${path}" "${min_bytes}" "${label}"
  fi
}

# ── HuggingFace download helper ───────────────────────────────────────────
# Uses `huggingface-hub` Python package if available, otherwise falls back to
# direct curl with the HuggingFace CDN URL.
hf_download() {
  local repo="$1"        # e.g. liquid-ai/LFM2.5-1.2B-Thinking-GGUF
  local filename="$2"    # e.g. LFM2.5-1.2B-Thinking-Q8_0.gguf
  local dest="$3"        # destination file path
  local min_bytes="$4"   # basic sanity check to reject HTML/errors/pointers
  local label="$5"

  if [[ -f "${dest}" && "${FORCE}" != "1" ]]; then
    verify_file "${dest}" "${min_bytes}" "${label}"
    warn "  Already exists and verified, skipping: ${dest}"
    return 0
  fi

  if [[ "${VERIFY_ONLY}" == "1" ]]; then
    verify_file "${dest}" "${min_bytes}" "${label}"
    return 0
  fi

  mkdir -p "$(dirname "${dest}")"
  info "  Downloading ${filename} from ${repo}..."

  local hf_url="https://huggingface.co/${repo}/resolve/main/${filename}"
  local tmp="${dest}.tmp"
  /usr/bin/rm -f "${tmp}"

  if command -v python3 >/dev/null 2>&1 && python3 -c "import huggingface_hub" 2>/dev/null; then
    # Prefer huggingface-hub for resumable downloads with progress
    HF_REPO="${repo}" HF_FILE="${filename}" HF_DEST="${dest}" HF_TOKEN="${HF_TOKEN}" python3 - <<'PY'
import os
import shutil
import sys
from pathlib import Path
from huggingface_hub import hf_hub_download

repo = os.environ["HF_REPO"]
filename = os.environ["HF_FILE"]
dest = Path(os.environ["HF_DEST"])
token = os.environ.get("HF_TOKEN") or None

path = hf_hub_download(
    repo_id=repo,
    filename=filename,
    local_dir=str(dest.parent),
    token=token,
)
if Path(path) != dest:
    shutil.copy2(path, dest)
print(f"Downloaded to {dest}")
PY
  else
    # Fallback: direct curl download
    if [[ -n "${HF_TOKEN}" ]]; then
      curl -fL --progress-bar --retry 3 --retry-delay 2 \
        -H "Authorization: Bearer ${HF_TOKEN}" \
        "${hf_url}" -o "${tmp}"
    else
      curl -fL --progress-bar --retry 3 --retry-delay 2 "${hf_url}" -o "${tmp}"
    fi
    mv "${tmp}" "${dest}"
  fi

  verify_file "${dest}" "${min_bytes}" "${label}"
  success "  ${filename} → ${dest}"
}

info "MASTERd model downloader"
info "Root: ${ROOT_DIR}"
[[ -n "${HF_TOKEN}" ]] && info "HF_TOKEN: set" || warn "HF_TOKEN: not set (may fail for gated models)"
echo ""

# ── Chat models ───────────────────────────────────────────────────────────
if [[ "${SKIP_CHAT}" == "0" ]]; then
  info "── Downloading LFM2.5-1.2B-Thinking (thinking model) ─────────────────"
  hf_download \
    "liquid-ai/LFM2.5-1.2B-Thinking-GGUF" \
    "LFM2.5-1.2B-Thinking-Q8_0.gguf" \
    "${ROOT_DIR}/models/lfm2.5-1.2b-thinking/LFM2.5-1.2B-Thinking-Q8_0.gguf" \
    1000000000 \
    "LFM2.5-1.2B-Thinking GGUF"
  hf_download \
    "liquid-ai/LFM2.5-1.2B-Thinking-GGUF" \
    "tokenizer.json" \
    "${ROOT_DIR}/models/lfm2.5-1.2b-thinking/tokenizer.json" \
    1000000 \
    "LFM2.5-1.2B-Thinking tokenizer"
  hf_download \
    "liquid-ai/LFM2.5-1.2B-Thinking-GGUF" \
    "tokenizer.chat_template" \
    "${ROOT_DIR}/models/lfm2.5-1.2b-thinking/tokenizer.chat_template" \
    100 \
    "LFM2.5-1.2B-Thinking chat template"

  info "── Downloading LFM2.5-350M-Instruct (fast instruct model) ────────────"
  hf_download \
    "liquid-ai/LFM2.5-350M-GGUF" \
    "LFM2.5-350M-Q8_0.gguf" \
    "${ROOT_DIR}/models/lfm2.5-350m-instruct/LFM2.5-350M-Q8_0.gguf" \
    300000000 \
    "LFM2.5-350M-Instruct GGUF"
  hf_download \
    "liquid-ai/LFM2.5-350M-GGUF" \
    "tokenizer.json" \
    "${ROOT_DIR}/models/lfm2.5-350m-instruct/tokenizer.json" \
    1000000 \
    "LFM2.5-350M-Instruct tokenizer"
  hf_download \
    "liquid-ai/LFM2.5-350M-GGUF" \
    "tokenizer.chat_template" \
    "${ROOT_DIR}/models/lfm2.5-350m-instruct/tokenizer.chat_template" \
    100 \
    "LFM2.5-350M-Instruct chat template"
fi

# ── ColBERT reranker ──────────────────────────────────────────────────────
if [[ "${SKIP_COLBERT}" == "0" ]]; then
  info "── Downloading LFM2-ColBERT-350M (reranker model) ─────────────────────"
  hf_download \
    "liquid-ai/LFM2-ColBERT-350M-GGUF" \
    "LFM2-ColBERT-350M-Q8_0.gguf" \
    "${ROOT_DIR}/models/lfm2-colbert-350m/LFM2-ColBERT-350M-Q8_0.gguf" \
    300000000 \
    "LFM2-ColBERT-350M GGUF"

  # Tokenizer lives in the base (non-GGUF) repo which is NOT gated.
  # A copy is also committed to models/lfm2-colbert-350m/tokenizer.json
  # as a fallback so fresh clones don't fail before first download.
  hf_download \
    "LiquidAI/LFM2-ColBERT-350M" \
    "tokenizer.json" \
    "${ROOT_DIR}/models/lfm2-colbert-350m/tokenizer.json" \
    1000000 \
    "LFM2-ColBERT-350M tokenizer"
fi

# ── Copy models to embedded assets location ───────────────────────────────
info ""
info "── Copying models to embedded assets for Rust include_bytes! ──────────"

if [[ "${SKIP_CHAT}" == "0" ]]; then
  THINKING_SRC="${ROOT_DIR}/models/lfm2.5-1.2b-thinking/LFM2.5-1.2B-Thinking-Q8_0.gguf"
  THINKING_TOK="${ROOT_DIR}/models/lfm2.5-1.2b-thinking/tokenizer.json"
  THINKING_TEMPLATE="${ROOT_DIR}/models/lfm2.5-1.2b-thinking/tokenizer.chat_template"
  THINKING_DST_DIR="${ROOT_DIR}/crates/masterd-chat-engine/assets/models/thinking"

  INSTRUCT_SRC="${ROOT_DIR}/models/lfm2.5-350m-instruct/LFM2.5-350M-Q8_0.gguf"
  INSTRUCT_TOK="${ROOT_DIR}/models/lfm2.5-350m-instruct/tokenizer.json"
  INSTRUCT_TEMPLATE="${ROOT_DIR}/models/lfm2.5-350m-instruct/tokenizer.chat_template"
  INSTRUCT_DST_DIR="${ROOT_DIR}/crates/masterd-chat-engine/assets/models/instruct"

  mkdir -p "${THINKING_DST_DIR}" "${INSTRUCT_DST_DIR}"

  verify_file "${THINKING_SRC}" 1000000000 "LFM2.5-1.2B-Thinking GGUF"
  verify_file "${THINKING_TOK}" 1000000 "LFM2.5-1.2B-Thinking tokenizer"
  verify_file "${THINKING_TEMPLATE}" 100 "LFM2.5-1.2B-Thinking chat template"
  verify_file "${INSTRUCT_SRC}" 300000000 "LFM2.5-350M-Instruct GGUF"
  verify_file "${INSTRUCT_TOK}" 1000000 "LFM2.5-350M-Instruct tokenizer"
  verify_file "${INSTRUCT_TEMPLATE}" 100 "LFM2.5-350M-Instruct chat template"

  if [[ "${VERIFY_ONLY}" != "1" ]]; then
    cp -v "${THINKING_SRC}" "${THINKING_DST_DIR}/model.gguf"
    cp -v "${THINKING_TOK}" "${THINKING_DST_DIR}/tokenizer.json"
    cp -v "${THINKING_TEMPLATE}" "${THINKING_DST_DIR}/tokenizer.chat_template"
    cp -v "${INSTRUCT_SRC}" "${INSTRUCT_DST_DIR}/model.gguf"
    cp -v "${INSTRUCT_TOK}" "${INSTRUCT_DST_DIR}/tokenizer.json"
    cp -v "${INSTRUCT_TEMPLATE}" "${INSTRUCT_DST_DIR}/tokenizer.chat_template"
  fi

  verify_file "${THINKING_DST_DIR}/model.gguf" 1000000000 "embedded thinking GGUF"
  verify_file "${THINKING_DST_DIR}/tokenizer.json" 1000000 "embedded thinking tokenizer"
  verify_file "${THINKING_DST_DIR}/tokenizer.chat_template" 100 "embedded thinking chat template"
  verify_file "${INSTRUCT_DST_DIR}/model.gguf" 300000000 "embedded instruct GGUF"
  verify_file "${INSTRUCT_DST_DIR}/tokenizer.json" 1000000 "embedded instruct tokenizer"
  verify_file "${INSTRUCT_DST_DIR}/tokenizer.chat_template" 100 "embedded instruct chat template"
fi

if [[ "${SKIP_COLBERT}" == "0" ]]; then
  verify_file \
    "${ROOT_DIR}/models/lfm2-colbert-350m/LFM2-ColBERT-350M-Q8_0.gguf" \
    300000000 \
    "LFM2-ColBERT-350M GGUF"
  verify_file \
    "${ROOT_DIR}/models/lfm2-colbert-350m/tokenizer.json" \
    1000000 \
    "LFM2-ColBERT-350M tokenizer"
fi

success ""
success "Models ready. You can now run:"
success "  cargo build -p masterd-chat-engine"
success "  cargo run -p masterd-bootstrap"
success "  cd apps/masterd-desktop-tauri && cargo tauri dev"
