#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SHELL_DIR="${ROOT_DIR}/apps/masterd-shell"

source "${ROOT_DIR}/scripts/lib/install-bootstrap.sh"
masterd_ensure_pnpm "${ROOT_DIR}"

run_install() {
  if [[ -f "${SHELL_DIR}/pnpm-lock.yaml" ]]; then
    masterd_pnpm --dir "${SHELL_DIR}" install --frozen-lockfile
  else
    masterd_pnpm --dir "${SHELL_DIR}" install
  fi
}

if [[ ! -x "${SHELL_DIR}/node_modules/.bin/next" ]]; then
  if ! run_install; then
    masterd_pnpm --dir "${SHELL_DIR}" approve-builds --all
    run_install
  fi
fi

masterd_pnpm --dir "${SHELL_DIR}" run "$@"
