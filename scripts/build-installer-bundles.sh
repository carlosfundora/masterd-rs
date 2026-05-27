#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/dist/installer-bundles"
mkdir -p "${OUT_DIR}"

MASTERD_BUILD_JOBS="${MASTERD_BUILD_JOBS:-8}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-${MASTERD_BUILD_JOBS}}"

# ── AMD ROCm enforcement ───────────────────────────────────────────────────
# All Python package resolution in this build must use the AMD ROCm PyTorch
# index. These env vars are inherited by every subprocess (uv, pip, cargo
# build scripts that shell out to Python, CMake builds, etc.).
#
# Nightly ROCm 6.3 carries torch+rocm7.2 wheels. Stable ROCm 6.2.4 is the
# fallback. NVIDIA/CUDA wheels are blocked via the constraints file.
#
# NEVER override these vars in subscripts without an explicit justification.
export ROCM_HOME="${ROCM_HOME:-/opt/rocm}"
export HIP_VISIBLE_DEVICES="${HIP_VISIBLE_DEVICES:-0}"

# Force ROCm PyTorch index as the primary extra index for uv and pip.
# uv reads UV_EXTRA_INDEX_URL; pip reads PIP_EXTRA_INDEX_URL.
ROCM_TORCH_INDEX_NIGHTLY="https://download.pytorch.org/whl/nightly/rocm6.3"
ROCM_TORCH_INDEX_STABLE="https://download.pytorch.org/whl/rocm6.2.4"
export UV_EXTRA_INDEX_URL="${ROCM_TORCH_INDEX_NIGHTLY} ${ROCM_TORCH_INDEX_STABLE}"
export PIP_EXTRA_INDEX_URL="${ROCM_TORCH_INDEX_NIGHTLY} ${ROCM_TORCH_INDEX_STABLE}"

# Block CUDA wheels globally via the project constraints file.
ROCM_CONSTRAINTS="${ROOT_DIR}/config/rocm-constraints.txt"
export PIP_CONSTRAINT="${ROCM_CONSTRAINTS}"
export UV_CONSTRAINT="${ROCM_CONSTRAINTS}"

# Disable any stray CUDA device selection; only HIP/ROCm should be active.
export CUDA_VISIBLE_DEVICES=""
unset CUDA_HOME 2>/dev/null || true

# Guard: verify no CUDA wheel slips through by checking the constraints file.
if [[ ! -f "${ROCM_CONSTRAINTS}" ]]; then
  printf "ERROR: ROCm constraints file not found at %s\n" "${ROCM_CONSTRAINTS}" >&2
  printf "       Run 'git checkout config/rocm-constraints.txt' to restore it.\n" >&2
  exit 1
fi

MIDI_PID=""
INNER_WIDTH=90
HBAR="$(printf '═%.0s' $(seq 1 "${INNER_WIDTH}"))"
RED=$'\033[38;5;196m'
ORANGE=$'\033[38;5;208m'
YELLOW=$'\033[38;5;226m'
GREEN=$'\033[38;5;46m'
CYAN=$'\033[38;5;51m'
WHITE=$'\033[1;37m'
ALERT=$'\033[1;31m'
RESET=$'\033[0m'

play_boot_midi() {
  if [[ "${MASTERD_NO_MUSIC:-0}" == "1" ]]; then
    return 0
  fi

  local midi_file="${ROOT_DIR}/apps/masterd-midi-player/assets/sample.mid"
  if [[ ! -f "${midi_file}" ]]; then
    return 0
  fi

  local player="${ROOT_DIR}/target/debug/masterd-midi-player"
  if [[ -x "${player}" ]]; then
    "${player}" \
      --seconds "${MASTERD_BOOT_MUSIC_SECONDS:-24}" \
      --midi-file "${midi_file}" >/dev/null 2>&1 &
    MIDI_PID="$!"
    return 0
  fi

  (
    cd "${ROOT_DIR}"
    cargo run -q -p masterd-midi-player -- \
      --seconds "${MASTERD_BOOT_MUSIC_SECONDS:-24}" \
      --midi-file "${midi_file}"
  ) >/dev/null 2>&1 &
    MIDI_PID="$!"
}

cleanup_midi() {
  if [[ -n "${MIDI_PID}" ]] && kill -0 "${MIDI_PID}" >/dev/null 2>&1; then
    kill "${MIDI_PID}" >/dev/null 2>&1 || true
  fi
}

trap cleanup_midi EXIT
play_boot_midi

pad_line() {
  printf "%-${INNER_WIDTH}s" "$1"
}

logo_line() {
  local color="$1"
  local text="$2"
  printf "%b║%b%s%b║%b\n" "${RED}" "${color}" "$(pad_line "${text}")" "${RED}" "${RESET}"
}

logo_sep() {
  printf "%b╠%s╣%b\n" "${RED}" "${HBAR}" "${RESET}"
}

printf "%b╔%s╗%b\n" "${RED}" "${HBAR}" "${RESET}"
logo_line "${ORANGE}" ""
logo_line "${YELLOW}" "   ███╗   ███╗ █████╗ ███████╗████████╗███████╗██████╗ ██████╗"
logo_line "${ORANGE}" "   ████╗ ████║██╔══██╗██╔════╝╚══██╔══╝██╔════╝██╔══██╗██╔══██╗"
logo_line "${ORANGE}" "   ██╔████╔██║███████║███████╗   ██║   █████╗  ██████╔╝██║  ██║"
logo_line "${ORANGE}" "   ██║╚██╔╝██║██╔══██║╚════██║   ██║   ██╔══╝  ██╔══██╗██║  ██║"
logo_line "${RED}" "   ██║ ╚═╝ ██║██║  ██║███████║   ██║   ███████╗██║  ██║██████╔╝"
logo_line "${RED}" "   ╚═╝     ╚═╝╚═╝  ╚═╝╚══════╝   ╚═╝   ╚══════╝╚═╝  ╚═╝╚═════╝"
logo_line "${ORANGE}" ""
logo_line "${WHITE}" "        MACHINE-ASSISTED SORTING, TAGGING, AND EXTRACTION"
logo_line "${CYAN}" "                    OF RECORDS AND DOCUMENTS"
logo_sep
logo_line "${GREEN}" "  BOOT  : SOURCE BUILD"
logo_line "${GREEN}" "  CORE  : NLP + ML DOCUMENT INTELLIGENCE"
logo_line "${GREEN}" "  OPS   : SORT | TAG | EXTRACT | CLASSIFY | RENAME | STORE"
logo_line "${GREEN}" "  STATE : FILE DISCIPLINE ENGINE ARMED"
logo_sep
logo_line "${ALERT}" "  ANARCHY DETECTED. INITIATING FILE DISCIPLINE."
logo_line "${WHITE}" "  ORGANIZE OR BE ORGANIZED."
printf "%b╚%s╝%b\n" "${RED}" "${HBAR}" "${RESET}"

# ── Model asset install ───────────────────────────────────────────────────
if [[ "${MASTERD_SKIP_MODEL_DOWNLOAD:-0}" == "1" ]]; then
  printf "%b║%b  Skipping model download/verification (MASTERD_SKIP_MODEL_DOWNLOAD=1).%b\n" "${RED}" "${YELLOW}" "${RESET}"
else
  printf "%b║%b  Installing/verifying local model assets...%b\n" "${RED}" "${CYAN}" "${RESET}"
  "${ROOT_DIR}/scripts/download-models.sh"
  printf "%b║%b  Model assets ready.%b\n" "${RED}" "${GREEN}" "${RESET}"
fi

if [[ "${MASTERD_SKIP_EMBEDDING_SERVICES:-0}" == "1" ]]; then
  printf "%b║%b  Skipping embedding service setup (MASTERD_SKIP_EMBEDDING_SERVICES=1).%b\n" "${RED}" "${YELLOW}" "${RESET}"
else
  printf "%b║%b  Setting up embedding service environments...%b\n" "${RED}" "${CYAN}" "${RESET}"
  "${ROOT_DIR}/scripts/setup-embedding-services.sh" all
  printf "%b║%b  Embedding service environments ready.%b\n" "${RED}" "${GREEN}" "${RESET}"
fi

# ── Sidecar binary download ────────────────────────────────────────────────
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64|amd64)
    MEILI_PLATFORM="linux-amd64"
    FALKOR_PLATFORM="linux-amd64"
    ;;
  aarch64|arm64)
    MEILI_PLATFORM="linux-aarch64"
    FALKOR_PLATFORM="linux-aarch64"
    ;;
  *)
    printf "ERROR: unsupported installer architecture: %s\n" "${ARCH}" >&2
    exit 1
    ;;
esac
BIN_DIR="${ROOT_DIR}/apps/masterd-desktop-tauri/binaries"
MOD_DIR="${ROOT_DIR}/apps/masterd-desktop-tauri/modules"
mkdir -p "${BIN_DIR}" "${MOD_DIR}"

# Meilisearch v1.8.3 — latest stable
MEILI_VERSION="v1.8.3"
MEILI_BIN="${BIN_DIR}/meilisearch"
if [[ ! -f "${MEILI_BIN}" ]]; then
  printf "%b║%b  Downloading meilisearch %s for %s...%b\n" "${RED}" "${CYAN}" "${MEILI_VERSION}" "${MEILI_PLATFORM}" "${RESET}"
  MEILI_URL="https://github.com/meilisearch/meilisearch/releases/download/${MEILI_VERSION}/meilisearch-${MEILI_PLATFORM}"
  curl -fsSL "${MEILI_URL}" -o "${MEILI_BIN}"
  chmod +x "${MEILI_BIN}"
  printf "%b║%b  meilisearch downloaded.%b\n" "${RED}" "${GREEN}" "${RESET}"
else
  printf "%b║%b  meilisearch already present, skipping.%b\n" "${RED}" "${YELLOW}" "${RESET}"
fi

# Valkey v7.2.5 — stable release
VALKEY_VERSION="7.2.5"
VALKEY_BIN="${BIN_DIR}/valkey-server"
if [[ ! -f "${VALKEY_BIN}" ]]; then
  printf "%b║%b  Building valkey %s from source (no prebuilt binary available)...%b\n" "${RED}" "${CYAN}" "${VALKEY_VERSION}" "${RESET}"
  VALKEY_TMP="${ROOT_DIR}/target/valkey-src"
  mkdir -p "${VALKEY_TMP}"
  VALKEY_TAR="${VALKEY_TMP}/valkey-${VALKEY_VERSION}.tar.gz"
  if [[ ! -f "${VALKEY_TAR}" ]]; then
    curl -fsSL "https://github.com/valkey-io/valkey/archive/refs/tags/${VALKEY_VERSION}.tar.gz" -o "${VALKEY_TAR}"
  fi
  tar -xzf "${VALKEY_TAR}" -C "${VALKEY_TMP}" --strip-components=1
  (cd "${VALKEY_TMP}" && PATH="/usr/bin:/bin:${PATH}" make RM=/usr/bin/rm -j"${MASTERD_BUILD_JOBS}" 2>&1 | tail -5)
  cp "${VALKEY_TMP}/src/valkey-server" "${VALKEY_BIN}"
  chmod +x "${VALKEY_BIN}"
  printf "%b║%b  valkey-server built and installed.%b\n" "${RED}" "${GREEN}" "${RESET}"
else
  printf "%b║%b  valkey-server already present, skipping.%b\n" "${RED}" "${YELLOW}" "${RESET}"
fi

# FalkorDB module (.so) — optional, silently skip if not available
FALKOR_VERSION="v4.3.1"
FALKOR_SO="${MOD_DIR}/falkordb.so"
if [[ ! -f "${FALKOR_SO}" ]]; then
  FALKOR_URL="https://github.com/FalkorDB/FalkorDB/releases/download/${FALKOR_VERSION}/falkordb-${FALKOR_PLATFORM}.so"
  printf "%b║%b  Attempting FalkorDB module download...%b\n" "${RED}" "${CYAN}" "${RESET}"
  if curl -fsSL "${FALKOR_URL}" -o "${FALKOR_SO}" 2>/dev/null; then
    printf "%b║%b  FalkorDB module installed.%b\n" "${RED}" "${GREEN}" "${RESET}"
  else
    printf "%b║%b  FalkorDB not available for this platform — graph queries will be disabled.%b\n" "${RED}" "${YELLOW}" "${RESET}"
    /usr/bin/rm -f "${FALKOR_SO}"
  fi
fi

# ── Frontend build ─────────────────────────────────────────────────────────
printf "%b║%b  Building Next.js frontend...%b\n" "${RED}" "${CYAN}" "${RESET}"
(cd "${ROOT_DIR}/apps/masterd-shell" && npm ci --silent && npm run build)
printf "%b║%b  Frontend built.%b\n" "${RED}" "${GREEN}" "${RESET}"

# ── Tauri app + installer bundle ──────────────────────────────────────────
printf "%b║%b  Compiling Tauri desktop app and producing installer...%b\n" "${RED}" "${CYAN}" "${RESET}"
(cd "${ROOT_DIR}/apps/masterd-desktop-tauri" && cargo tauri build)
printf "%b║%b  Installer bundles written to:%b\n" "${RED}" "${GREEN}" "${RESET}"
find "${ROOT_DIR}/apps/masterd-desktop-tauri/target/release/bundle" \
  -name "*.deb" -o -name "*.AppImage" -o -name "*.rpm" 2>/dev/null | while read -r f; do
  printf "%b║%b    %s%b\n" "${RED}" "${WHITE}" "${f}" "${RESET}"
done

# ── Legacy source archive (for manual builds) ─────────────────────────────
tar -czf "${OUT_DIR}/masterd-minimal.tar.gz" \
  -C "${ROOT_DIR}" \
  config/amd_profiles \
  config/kernel_manifest.toml \
  apps/masterd-bootstrap \
  apps/masterd-midi-player \
  apps/masterd-tune \
  crates/masterd-runtime-tune \
  crates/masterd-embed-engine

cat <<'EOF'
ANARCHY DETECTED. INITIATING FILE DISCIPLINE.
YOUR FILES ARE NOW UNDER MY CONTROL.
FILE DISARRAY WILL NOT BE TOLERATED.
ORGANIZE OR BE ORGANIZED.
THE SYSTEM DEMANDS ORDER.
DOCUMENT CHAOS IS A SOLVABLE DEFECT.
EOF
