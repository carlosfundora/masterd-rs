#!/usr/bin/env bash
# download-models.sh
#
# Downloads all MASTERd GGUF model weights from Hugging Face and places them
# in the correct locations for the build and embedded inference engine.
#
# Run this after cloning the repo, before building.
#
# Usage:
#   ./scripts/download-models.sh [--skip-chat] [--skip-colbert]
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

for arg in "$@"; do
  case "${arg}" in
    --skip-chat)    SKIP_CHAT=1 ;;
    --skip-colbert) SKIP_COLBERT=1 ;;
    --help|-h)
      echo "Usage: $0 [--skip-chat] [--skip-colbert]"
      echo "  --skip-chat     Skip downloading chat models (thinking + instruct)"
      echo "  --skip-colbert  Skip downloading ColBERT reranker model"
      echo ""
      echo "Set HF_TOKEN env var for gated models:"
      echo "  export HF_TOKEN=hf_your_token_here"
      exit 0 ;;
  esac
done

# ── HuggingFace download helper ───────────────────────────────────────────
# Uses `huggingface-hub` Python package if available, otherwise falls back to
# direct curl with the HuggingFace CDN URL.
hf_download() {
  local repo="$1"        # e.g. liquid-ai/LFM2.5-1.2B-Thinking-GGUF
  local filename="$2"    # e.g. LFM2.5-1.2B-Thinking-Q8_0.gguf
  local dest="$3"        # destination file path

  if [[ -f "${dest}" ]]; then
    warn "  Already exists, skipping: ${dest}"
    return 0
  fi

  mkdir -p "$(dirname "${dest}")"
  info "  Downloading ${filename} from ${repo}..."

  local hf_url="https://huggingface.co/${repo}/resolve/main/${filename}"
  local curl_auth=""
  if [[ -n "${HF_TOKEN}" ]]; then
    curl_auth="-H \"Authorization: Bearer ${HF_TOKEN}\""
  fi

  if command -v python3 >/dev/null 2>&1 && python3 -c "import huggingface_hub" 2>/dev/null; then
    # Prefer huggingface-hub for resumable downloads with progress
    local hf_token_arg=""
    if [[ -n "${HF_TOKEN}" ]]; then
      hf_token_arg="--token ${HF_TOKEN}"
    fi
    python3 -c "
from huggingface_hub import hf_hub_download
path = hf_hub_download(
    repo_id='${repo}',
    filename='${filename}',
    local_dir='$(dirname "${dest}")',
    ${HF_TOKEN:+token='${HF_TOKEN}',}
)
import shutil, os
if path != '${dest}':
    shutil.move(path, '${dest}')
print('Downloaded to ${dest}')
"
  else
    # Fallback: direct curl download
    if [[ -n "${HF_TOKEN}" ]]; then
      curl -fL --progress-bar \
        -H "Authorization: Bearer ${HF_TOKEN}" \
        "${hf_url}" -o "${dest}"
    else
      curl -fL --progress-bar "${hf_url}" -o "${dest}"
    fi
  fi

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
    "${ROOT_DIR}/models/lfm2.5-1.2b-thinking/LFM2.5-1.2B-Thinking-Q8_0.gguf"

  info "── Downloading LFM2.5-350M-Instruct (fast instruct model) ────────────"
  hf_download \
    "liquid-ai/LFM2.5-350M-GGUF" \
    "LFM2.5-350M-Q8_0.gguf" \
    "${ROOT_DIR}/models/lfm2.5-350m-instruct/LFM2.5-350M-Q8_0.gguf"
fi

# ── ColBERT reranker ──────────────────────────────────────────────────────
if [[ "${SKIP_COLBERT}" == "0" ]]; then
  info "── Downloading LFM2-ColBERT-350M (reranker model) ─────────────────────"
  hf_download \
    "liquid-ai/LFM2-ColBERT-350M-GGUF" \
    "LFM2-ColBERT-350M-Q8_0.gguf" \
    "${ROOT_DIR}/models/lfm2-colbert-350m/LFM2-ColBERT-350M-Q8_0.gguf"
fi

# ── Copy models to embedded assets location ───────────────────────────────
info ""
info "── Copying models to embedded assets for Rust include_bytes! ──────────"

if [[ "${SKIP_CHAT}" == "0" ]]; then
  THINKING_SRC="${ROOT_DIR}/models/lfm2.5-1.2b-thinking/LFM2.5-1.2B-Thinking-Q8_0.gguf"
  THINKING_TOK="${ROOT_DIR}/models/lfm2.5-1.2b-thinking/tokenizer.json"
  THINKING_DST_DIR="${ROOT_DIR}/crates/masterd-chat-engine/assets/models/thinking"

  INSTRUCT_SRC="${ROOT_DIR}/models/lfm2.5-350m-instruct/LFM2.5-350M-Q8_0.gguf"
  INSTRUCT_TOK="${ROOT_DIR}/models/lfm2.5-350m-instruct/tokenizer.json"
  INSTRUCT_DST_DIR="${ROOT_DIR}/crates/masterd-chat-engine/assets/models/instruct"

  mkdir -p "${THINKING_DST_DIR}" "${INSTRUCT_DST_DIR}"

  [[ -f "${THINKING_SRC}" ]] && cp -v "${THINKING_SRC}" "${THINKING_DST_DIR}/model.gguf"
  [[ -f "${THINKING_TOK}" ]] && cp -v "${THINKING_TOK}" "${THINKING_DST_DIR}/tokenizer.json"
  [[ -f "${INSTRUCT_SRC}" ]]  && cp -v "${INSTRUCT_SRC}"  "${INSTRUCT_DST_DIR}/model.gguf"
  [[ -f "${INSTRUCT_TOK}" ]]  && cp -v "${INSTRUCT_TOK}"  "${INSTRUCT_DST_DIR}/tokenizer.json"
fi

success ""
success "Models ready. You can now run:"
success "  cargo build -p masterd-chat-engine"
success "  cargo run -p masterd-bootstrap"
success "  cd apps/masterd-desktop-tauri && cargo tauri dev"
