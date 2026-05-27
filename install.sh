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

# Ensure we have python3
PYTHON_BIN=""
if command -v python3.12 >/dev/null 2>&1; then
  PYTHON_BIN="python3.12"
elif command -v python3 >/dev/null 2>&1; then
  PYTHON_BIN="python3"
elif command -v python >/dev/null 2>&1; then
  PYTHON_BIN="python"
else
  die "Python is required but not found. Please install Python 3.12+ and try again."
fi

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
    "${UV_BIN}" venv --seed --relocatable --python "$(command -v ${PYTHON_BIN})" "${BOOTSTRAP_VENV}"
  else
    info "Using standard python venv module..."
    "$(command -v ${PYTHON_BIN})" -m venv "${BOOTSTRAP_VENV}"
  fi
fi

# Ensure huggingface-hub is present inside bootstrap venv
VENV_PYTHON="${BOOTSTRAP_VENV}/bin/python"

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
    env -u UV_EXTRA_INDEX_URL -u UV_INDEX_URL -u UV_CONSTRAINT "${UV_BIN}" pip install --python "${VENV_PYTHON}" huggingface-hub
    if [ $? -ne 0 ]; then
      die "Failed to install huggingface-hub with uv"
    fi
  else
    "${VENV_PYTHON}" -m ensurepip --upgrade >/dev/null 2>&1 || true
    env -u PIP_EXTRA_INDEX_URL -u PIP_INDEX_URL -u PIP_CONSTRAINT "${VENV_PYTHON}" -m pip install --upgrade pip setuptools wheel
    if [ $? -ne 0 ]; then
      die "Failed to upgrade pip/setuptools/wheel"
    fi
    env -u PIP_EXTRA_INDEX_URL -u PIP_INDEX_URL -u PIP_CONSTRAINT "${VENV_PYTHON}" -m pip install huggingface-hub
    if [ $? -ne 0 ]; then
      die "Failed to install huggingface-hub with pip"
    fi
  fi
fi

# Invoke the Python bootstrap script
info "Executing second stage Python bootstrap..."
exec "${BOOTSTRAP_VENV}/bin/python" "${ROOT_DIR}/scripts/bootstrap.py"
