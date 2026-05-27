#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SERVICE_DIR="${ROOT_DIR}/services/searxng-service"
SRC_DIR="${SERVICE_DIR}/searxng-src"
VENV_DIR="${SERVICE_DIR}/.venv"
SETTINGS_FILE="${SERVICE_DIR}/settings.yml"
SEARXNG_REF="${SEARXNG_REF:-master}"

source "${ROOT_DIR}/scripts/lib/install-bootstrap.sh"

masterd_ensure_source_build_tools "${ROOT_DIR}"
masterd_resolve_python "${ROOT_DIR}"
masterd_ensure_uv "${ROOT_DIR}"

mkdir -p "${SERVICE_DIR}"

if [[ ! -d "${SRC_DIR}/.git" ]]; then
  rm -rf "${SRC_DIR}"
  git clone --depth 1 --branch "${SEARXNG_REF}" https://github.com/searxng/searxng.git "${SRC_DIR}"
else
  git -C "${SRC_DIR}" fetch --depth 1 origin "${SEARXNG_REF}"
  git -C "${SRC_DIR}" checkout FETCH_HEAD
fi

rm -rf "${VENV_DIR}"
"${MASTERD_UV_BIN}" venv --seed --relocatable --python "${MASTERD_PYTHON_BIN}" "${VENV_DIR}"
"${MASTERD_UV_BIN}" pip install --python "${VENV_DIR}/bin/python" -e "${SRC_DIR}"
"${MASTERD_UV_BIN}" pip install --python "${VENV_DIR}/bin/python" pyyaml

[[ -f "${SETTINGS_FILE}" ]] || {
  printf "[setup-searxng] ERROR: missing settings template at %s\n" "${SETTINGS_FILE}" >&2
  exit 1
}

"${VENV_DIR}/bin/python" - "${SETTINGS_FILE}" <<'PY'
import secrets
import sys
from pathlib import Path

path = Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
placeholder = "MASTERd local development secret; installer rewrites this"
if placeholder in text:
    text = text.replace(placeholder, secrets.token_urlsafe(48))
    path.write_text(text, encoding="utf-8")
PY

printf "[setup-searxng] ready at %s\n" "${SERVICE_DIR}"
