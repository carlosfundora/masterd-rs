#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SHELL_DIR="${ROOT_DIR}/apps/masterd-shell"

if ! command -v pnpm >/dev/null 2>&1; then
  echo "pnpm is required to build MASTERd shell. Install pnpm or enable Corepack." >&2
  exit 127
fi

if [[ ! -x "${SHELL_DIR}/node_modules/.bin/next" ]]; then
  if [[ -f "${SHELL_DIR}/pnpm-lock.yaml" ]]; then
    pnpm --dir "${SHELL_DIR}" install --frozen-lockfile
  else
    pnpm --dir "${SHELL_DIR}" install
  fi
fi

exec pnpm --dir "${SHELL_DIR}" run "$@"
