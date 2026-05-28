#!/usr/bin/env bash
# install.sh
#
# MASTERd clean recursive installation entrypoint.
# Step 1: Bootstraps a lightweight python virtual environment (.venv-bootstrap).
# Step 2: Runs scripts/bootstrap.py inside the environment to set up Rust and clone vendors.
# Step 3: Passes control to the Rust bootstrap orchestrator for full environment build.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BOOTSTRAP_VENV="${ROOT_DIR}/.venv-bootstrap"
MODEL_DOWNLOAD_SCRIPT="${ROOT_DIR}/scripts/download-models.sh"

# The main installer delegates model transfer to scripts/download-models.sh.
# Keep the required release Jina v5 GGUF repos visible here so install-time
# drift is caught before the recursive bootstrap starts.
REQUIRED_JINA_V5_GGUF_REPOS=(
  "jinaai/jina-embeddings-v5-omni-nano-retrieval-GGUF"
  "jinaai/jina-embeddings-v5-omni-nano-text-matching-GGUF"
  "jinaai/jina-embeddings-v5-omni-small-retrieval-GGUF"
  "jinaai/jina-embeddings-v5-omni-small-text-matching-GGUF"
)

# Terminal colors
RED=$'\033[38;5;196m'
GREEN=$'\033[38;5;46m'
CYAN=$'\033[38;5;51m'
YELLOW=$'\033[38;5;226m'
RESET=$'\033[0m'

info()    { printf "%b[install.sh]%b %s\n" "${CYAN}"  "${RESET}" "$*"; }
success() { printf "%b[install.sh]%b %s\n" "${GREEN}" "${RESET}" "$*"; }
warn()    { printf "%b[install.sh]%b %s\n" "${YELLOW}" "${RESET}" "$*"; }
die()     { printf "%b[install.sh] ERROR:%b %s\n" "${RED}" "${RESET}" "$*" >&2; exit 1; }

info "Starting recursive installation for MASTERd..."

if [[ ! -x "${MODEL_DOWNLOAD_SCRIPT}" ]]; then
  die "Model download script is missing or not executable: ${MODEL_DOWNLOAD_SCRIPT}"
fi
for repo in "${REQUIRED_JINA_V5_GGUF_REPOS[@]}"; do
  if ! grep -Fq "${repo}" "${MODEL_DOWNLOAD_SCRIPT}"; then
    die "Model download script is missing required Jina v5 GGUF repo: ${repo}"
  fi
done
info "Verified Jina v5 GGUF model download URLs in scripts/download-models.sh"

# Ensure we have python3
PYTHON_BIN=""
if command -v python3.12 >/dev/null 2>&1; then
  PYTHON_BIN="python3.12"
elif command -v python3 >/dev/null 2>&1; then
  PYTHON_BIN="python3"
elif command -v python >/dev/null 2>&1; then
  PYTHON_BIN="python"
else
  die "Python is required but not found. Please install Python 3.10+ and try again."
fi

PYTHON_BIN_PATH="$(command -v "${PYTHON_BIN}")"
"${PYTHON_BIN_PATH}" - <<'PY' || die "Selected Python interpreter must be Python 3.10 or newer"
import sys
if sys.version_info < (3, 10):
    raise SystemExit(1)
print(f"using Python {sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")
PY

# Ensure clean venv if corrupted
if [[ -d "${BOOTSTRAP_VENV}" && ! -x "${BOOTSTRAP_VENV}/bin/python" ]]; then
  warn "Bootstrap venv exists but is corrupted. Cleaning up..."
  /usr/bin/rm -rf "${BOOTSTRAP_VENV}"
fi

# Create bootstrap venv if not present
if [[ ! -d "${BOOTSTRAP_VENV}" ]]; then
  info "Creating bootstrap venv with ${PYTHON_BIN}..."
  
  # Try to find uv
  UV_BIN=""
  if command -v uv >/dev/null 2>&1; then
    UV_BIN="$(command -v uv)"
  elif [[ -x "${HOME}/.local/bin/uv" ]]; then
    UV_BIN="${HOME}/.local/bin/uv"
  fi

  if [[ -n "${UV_BIN}" ]]; then
    info "Using uv to create relocatable venv..."
    if ! "${UV_BIN}" venv --seed --relocatable --python "${PYTHON_BIN_PATH}" "${BOOTSTRAP_VENV}"; then
      die "Failed to create bootstrap venv with uv"
    fi
  else
    info "Using standard python venv module..."
    if ! "${PYTHON_BIN_PATH}" -m venv "${BOOTSTRAP_VENV}"; then
      die "Failed to create bootstrap venv with python venv"
    fi
  fi
fi

# Ensure huggingface-hub is present inside bootstrap venv
VENV_PYTHON="${BOOTSTRAP_VENV}/bin/python"
if [[ ! -x "${VENV_PYTHON}" ]]; then
  die "Bootstrap venv Python is missing or not executable: ${VENV_PYTHON}"
fi
"${VENV_PYTHON}" - <<'PY' || die "Bootstrap venv Python is not runnable or is too old"
import sys
if sys.version_info < (3, 10):
    raise SystemExit(1)
PY

if ! "${VENV_PYTHON}" -c "import huggingface_hub" >/dev/null 2>&1; then
  info "Installing huggingface-hub in bootstrap venv..."
  # Try using uv first if available
  UV_BIN=""
  if command -v uv >/dev/null 2>&1; then
    UV_BIN="$(command -v uv)"
  elif [[ -x "${HOME}/.local/bin/uv" ]]; then
    UV_BIN="${HOME}/.local/bin/uv"
  fi

  if [[ -n "${UV_BIN}" ]]; then
    if ! env -u UV_EXTRA_INDEX_URL -u UV_INDEX_URL -u UV_CONSTRAINT "${UV_BIN}" pip install --python "${VENV_PYTHON}" huggingface-hub; then
      die "Failed to install huggingface-hub with uv"
    fi
  else
    if ! "${VENV_PYTHON}" -m ensurepip --upgrade >/dev/null 2>&1; then
      warn "ensurepip failed or is unavailable; continuing with existing pip if present"
    fi
    if ! env -u PIP_EXTRA_INDEX_URL -u PIP_INDEX_URL -u PIP_CONSTRAINT "${VENV_PYTHON}" -m pip install --upgrade pip setuptools wheel; then
      die "Failed to upgrade pip/setuptools/wheel"
    fi
    if ! env -u PIP_EXTRA_INDEX_URL -u PIP_INDEX_URL -u PIP_CONSTRAINT "${VENV_PYTHON}" -m pip install huggingface-hub; then
      die "Failed to install huggingface-hub with pip"
    fi
  fi
fi

info "Setting up Rust build acceleration tools (sccache, mold)..."
if command -v cargo >/dev/null 2>&1; then
  if ! command -v sccache >/dev/null 2>&1; then
    info "Installing sccache..."
    cargo install sccache || warn "Failed to install sccache"
  fi
else
  warn "Cargo not found, skipping sccache installation."
fi

if ! command -v mold >/dev/null 2>&1; then
  info "Attempting to install mold..."
  if command -v apt-get >/dev/null 2>&1; then
    sudo apt-get install -y mold || warn "Failed to install mold via apt-get. You may need to enter your sudo password."
  else
    warn "apt-get not found, please install mold manually."
  fi
fi

# Invoke the Python bootstrap script
info "Executing second stage Python bootstrap..."
exec "${BOOTSTRAP_VENV}/bin/python" "${ROOT_DIR}/scripts/bootstrap.py"
