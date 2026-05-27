#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${ROOT_DIR}/dist/installer-bundles"
mkdir -p "${OUT_DIR}"

source "${ROOT_DIR}/scripts/lib/install-bootstrap.sh"

MASTERD_BUILD_JOBS="${MASTERD_BUILD_JOBS:-8}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-${MASTERD_BUILD_JOBS}}"

# ── AMD ROCm defaults ──────────────────────────────────────────────────────
# Python package resolution is scoped inside setup-embedding-services.sh.
# Do not export pip/uv index URLs globally from this installer: non-package
# uv commands and unrelated subprocesses can interpret them as request URLs.
#
# Nightly ROCm 6.3 carries torch+rocm7.2 wheels. Stable ROCm 6.2.4 is the
# fallback. NVIDIA/CUDA wheels are blocked via the constraints file.
#
# NEVER override these vars in subscripts without an explicit justification.
export ROCM_HOME="${ROCM_HOME:-/opt/rocm}"
export HIP_VISIBLE_DEVICES="${HIP_VISIBLE_DEVICES:-0}"

ROCM_TORCH_INDEX_NIGHTLY="${ROCM_TORCH_INDEX_NIGHTLY:-https://download.pytorch.org/whl/nightly/rocm6.3}"
ROCM_TORCH_INDEX_STABLE="${ROCM_TORCH_INDEX_STABLE:-https://download.pytorch.org/whl/rocm6.2.4}"
unset UV_EXTRA_INDEX_URL UV_INDEX_URL UV_CONSTRAINT
unset PIP_EXTRA_INDEX_URL PIP_INDEX_URL PIP_CONSTRAINT

# Block CUDA wheels globally via the project constraints file.
ROCM_CONSTRAINTS="${ROOT_DIR}/config/rocm-constraints.txt"

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

ensure_source_build_tools() {
  masterd_ensure_source_build_tools "${ROOT_DIR}"

  printf "%b║%b  Source build tools ready: %s | %s%b\n" \
    "${RED}" "${GREEN}" "$(rustc --version)" "$(cargo --version)" "${RESET}"
}

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

ensure_source_build_tools
masterd_resolve_python "${ROOT_DIR}"

printf "%b║%b  Ensuring vendored package dependencies are cloned...%b\n" "${RED}" "${CYAN}" "${RESET}"
"${ROOT_DIR}/scripts/clone-vendors.sh"
printf "%b║%b  Vendored packages ready.%b\n" "${RED}" "${GREEN}" "${RESET}"

play_boot_midi

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

if [[ "${MASTERD_SKIP_SEARXNG:-0}" == "1" ]]; then
  printf "%b║%b  Skipping SearXNG setup (MASTERD_SKIP_SEARXNG=1).%b\n" "${RED}" "${YELLOW}" "${RESET}"
else
  printf "%b║%b  Setting up bundled SearXNG web search service...%b\n" "${RED}" "${CYAN}" "${RESET}"
  "${ROOT_DIR}/scripts/setup-searxng-service.sh"
  printf "%b║%b  SearXNG web search service ready.%b\n" "${RED}" "${GREEN}" "${RESET}"
fi

# ── Sidecar binary download ────────────────────────────────────────────────
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64|amd64)
    # AMD Ryzen CPUs report x86_64 and require Meilisearch's linux-amd64 asset.
    NATIVE_EXPECTED_FILE_ARCH="x86-64"
    MEILI_PLATFORM="linux-amd64"
    MEILI_EXPECTED_FILE_ARCH="${NATIVE_EXPECTED_FILE_ARCH}"
    FALKOR_WHEEL_PLATFORM="manylinux_2_17_x86_64"
    ;;
  aarch64|arm64)
    NATIVE_EXPECTED_FILE_ARCH="ARM aarch64"
    MEILI_PLATFORM="linux-aarch64"
    MEILI_EXPECTED_FILE_ARCH="${NATIVE_EXPECTED_FILE_ARCH}"
    FALKOR_WHEEL_PLATFORM="manylinux_2_17_aarch64"
    ;;
  *)
    printf "ERROR: unsupported installer architecture: %s\n" "${ARCH}" >&2
    exit 1
    ;;
esac
BIN_DIR="${ROOT_DIR}/apps/masterd-desktop-tauri/binaries"
MOD_DIR="${ROOT_DIR}/apps/masterd-desktop-tauri/modules"
mkdir -p "${BIN_DIR}" "${MOD_DIR}"

native_binary_matches_arch() {
  local bin="$1"
  local desc
  [[ -f "${bin}" && -s "${bin}" ]] || return 1
  command -v file >/dev/null 2>&1 || masterd_die "file is required to validate downloaded native binaries"
  desc="$(file "${bin}")"
  [[ "${desc}" == *"${NATIVE_EXPECTED_FILE_ARCH}"* ]]
}

validate_meilisearch_binary() {
  local bin="$1"
  local desc
  command -v file >/dev/null 2>&1 || masterd_die "file is required to validate the meilisearch binary architecture"
  desc="$(file "${bin}")"
  if [[ "${desc}" != *"${MEILI_EXPECTED_FILE_ARCH}"* ]]; then
    printf "ERROR: meilisearch binary architecture mismatch for %s: %s\n" "${ARCH}" "${desc}" >&2
    exit 1
  fi
}

validate_valkey_tarball() {
  tar -tzf "$1" >/dev/null
}

validate_falkor_install() {
  native_binary_matches_arch "${FALKOR_SO}" && native_binary_matches_arch "${FALKOR_SERVER}"
}

# Meilisearch v1.8.3 — latest stable
MEILI_VERSION="v1.8.3"
MEILI_BIN="${BIN_DIR}/meilisearch"
if [[ ! -f "${MEILI_BIN}" ]]; then
  printf "%b║%b  Downloading meilisearch %s for %s...%b\n" "${RED}" "${CYAN}" "${MEILI_VERSION}" "${MEILI_PLATFORM}" "${RESET}"
  MEILI_URL="https://github.com/meilisearch/meilisearch/releases/download/${MEILI_VERSION}/meilisearch-${MEILI_PLATFORM}"
  MEILI_TMP="${MEILI_BIN}.tmp.$$"
  rm -f "${MEILI_TMP}"
  masterd_download_atomic "${MEILI_URL}" "${MEILI_TMP}"
  chmod +x "${MEILI_TMP}"
  validate_meilisearch_binary "${MEILI_TMP}"
  mv "${MEILI_TMP}" "${MEILI_BIN}"
  printf "%b║%b  meilisearch downloaded.%b\n" "${RED}" "${GREEN}" "${RESET}"
else
  printf "%b║%b  meilisearch already present, skipping.%b\n" "${RED}" "${YELLOW}" "${RESET}"
fi
validate_meilisearch_binary "${MEILI_BIN}"

# Valkey v7.2.5 — stable release
VALKEY_VERSION="7.2.5"
VALKEY_BIN="${BIN_DIR}/valkey-server"
if [[ ! -f "${VALKEY_BIN}" ]] || ! native_binary_matches_arch "${VALKEY_BIN}"; then
  printf "%b║%b  Building valkey %s from source (no prebuilt binary available)...%b\n" "${RED}" "${CYAN}" "${VALKEY_VERSION}" "${RESET}"
  VALKEY_TMP="${ROOT_DIR}/target/valkey-src"
  /usr/bin/rm -rf "${VALKEY_TMP}"
  mkdir -p "${VALKEY_TMP}"
  VALKEY_TAR="${VALKEY_TMP}/valkey-${VALKEY_VERSION}.tar.gz"
  if [[ ! -f "${VALKEY_TAR}" ]] || ! validate_valkey_tarball "${VALKEY_TAR}"; then
    VALKEY_TAR_TMP="${VALKEY_TAR}.tmp.$$"
    rm -f "${VALKEY_TAR_TMP}"
    masterd_download_atomic "https://github.com/valkey-io/valkey/archive/refs/tags/${VALKEY_VERSION}.tar.gz" "${VALKEY_TAR_TMP}"
    validate_valkey_tarball "${VALKEY_TAR_TMP}"
    mv "${VALKEY_TAR_TMP}" "${VALKEY_TAR}"
  fi
  tar -xzf "${VALKEY_TAR}" -C "${VALKEY_TMP}" --strip-components=1
  (cd "${VALKEY_TMP}" && PATH="/usr/bin:/bin:${PATH}" make RM=/usr/bin/rm -j"${MASTERD_BUILD_JOBS}" 2>&1 | tail -5)
  cp "${VALKEY_TMP}/src/valkey-server" "${VALKEY_BIN}"
  chmod +x "${VALKEY_BIN}"
  printf "%b║%b  valkey-server built and installed.%b\n" "${RED}" "${GREEN}" "${RESET}"
else
  printf "%b║%b  valkey-server already present, skipping.%b\n" "${RED}" "${YELLOW}" "${RESET}"
fi
native_binary_matches_arch "${VALKEY_BIN}" || masterd_die "valkey-server binary failed architecture validation"

# FalkorDB module from official falkordb-bin wheels.
# MASTERd runs Valkey and FalkorDB as separate local DB processes;
# the wheel supplies FalkorDB's compatible server binary and module.
FALKOR_BIN_VERSION="1.4.1"
FALKOR_SO="${MOD_DIR}/falkordb.so"
FALKOR_SERVER="${BIN_DIR}/falkordb-server"
install_falkor_from_wheel() {
  local wheel_dir="${ROOT_DIR}/target/falkordb-bin"
  local metadata_json="${wheel_dir}/falkordb-bin-${FALKOR_BIN_VERSION}.json"
  local wheel_file="${wheel_dir}/falkordb-bin-${FALKOR_BIN_VERSION}-${FALKOR_WHEEL_PLATFORM}.whl"
  local metadata_tmp="${metadata_json}.tmp.$$"
  local wheel_tmp="${wheel_file}.tmp.$$"
  local module_tmp="${FALKOR_SO}.tmp.$$"
  local server_tmp="${FALKOR_SERVER}.tmp.$$"
  local wheel_url

  mkdir -p "${wheel_dir}"
  rm -f "${metadata_tmp}" "${wheel_tmp}" "${module_tmp}" "${server_tmp}"
  masterd_download_atomic "https://pypi.org/pypi/falkordb-bin/${FALKOR_BIN_VERSION}/json" "${metadata_tmp}"
  "${MASTERD_PYTHON_BIN}" -m json.tool "${metadata_tmp}" >/dev/null
  mv "${metadata_tmp}" "${metadata_json}"
  wheel_url="$(FALKOR_WHEEL_PLATFORM="${FALKOR_WHEEL_PLATFORM}" "${MASTERD_PYTHON_BIN}" - "${metadata_json}" <<'PY'
import json
import os
import sys

platform = os.environ["FALKOR_WHEEL_PLATFORM"]
metadata_path = sys.argv[1]
preferred = ["cp312", "cp311", "cp310", "cp313"]
with open(metadata_path, "r", encoding="utf-8") as fh:
    urls = json.load(fh)["urls"]

candidates = [u for u in urls if platform in u["filename"] and u["filename"].endswith(".whl")]
for py_tag in preferred:
    for candidate in candidates:
        if f"-{py_tag}-{py_tag}-" in candidate["filename"]:
            print(candidate["url"])
            raise SystemExit(0)
if candidates:
    print(candidates[0]["url"])
    raise SystemExit(0)
raise SystemExit(f"no falkordb-bin wheel found for {platform}")
PY
)"

  printf "%b║%b  Downloading FalkorDB binary wheel %s for %s...%b\n" "${RED}" "${CYAN}" "${FALKOR_BIN_VERSION}" "${FALKOR_WHEEL_PLATFORM}" "${RESET}"
  masterd_download_atomic "${wheel_url}" "${wheel_tmp}"
  FALKOR_WHEEL="${wheel_tmp}" "${MASTERD_PYTHON_BIN}" - <<'PY'
import os
import zipfile

wheel = os.environ["FALKOR_WHEEL"]
with zipfile.ZipFile(wheel) as zf:
    names = zf.namelist()
    if not any(name.endswith("/falkordb.so") for name in names):
        raise SystemExit("falkordb.so missing from FalkorDB wheel")
    if not any(name.endswith("/redis-server") for name in names):
        raise SystemExit("redis-server missing from FalkorDB wheel")
PY
  mv "${wheel_tmp}" "${wheel_file}"

  FALKOR_WHEEL="${wheel_file}" FALKOR_SO="${module_tmp}" FALKOR_SERVER="${server_tmp}" "${MASTERD_PYTHON_BIN}" - <<'PY'
import os
import stat
import zipfile

wheel = os.environ["FALKOR_WHEEL"]
module_dest = os.environ["FALKOR_SO"]
server_dest = os.environ["FALKOR_SERVER"]
with zipfile.ZipFile(wheel) as zf:
    module_name = next(name for name in zf.namelist() if name.endswith("/falkordb.so"))
    with zf.open(module_name) as src, open(module_dest, "wb") as dst:
        dst.write(src.read())
    server_name = next(name for name in zf.namelist() if name.endswith("/redis-server"))
    with zf.open(server_name) as src, open(server_dest, "wb") as dst:
        dst.write(src.read())
mode = os.stat(module_dest).st_mode
os.chmod(module_dest, mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
mode = os.stat(server_dest).st_mode
os.chmod(server_dest, mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
PY
  mv "${module_tmp}" "${FALKOR_SO}"
  mv "${server_tmp}" "${FALKOR_SERVER}"
}

if [[ ! -f "${FALKOR_SO}" || ! -f "${FALKOR_SERVER}" ]] || ! validate_falkor_install; then
  printf "%b║%b  Installing FalkorDB graph DB...%b\n" "${RED}" "${CYAN}" "${RESET}"
  install_falkor_from_wheel
  printf "%b║%b  FalkorDB graph DB installed.%b\n" "${RED}" "${GREEN}" "${RESET}"
else
  printf "%b║%b  FalkorDB graph DB already present, skipping.%b\n" "${RED}" "${YELLOW}" "${RESET}"
fi
validate_falkor_install || masterd_die "FalkorDB graph DB files failed architecture validation"

# ── model2vec-service ──────────────────────────────────────────────────────
MODEL2VEC_BIN="${BIN_DIR}/model2vec-service"
if [[ ! -f "${MODEL2VEC_BIN}" ]] || ! native_binary_matches_arch "${MODEL2VEC_BIN}"; then
  printf "%b║%b  Compiling model2vec-service from source...%b\n" "${RED}" "${CYAN}" "${RESET}"
  (cd "${ROOT_DIR}/services/model2vec-service" && cargo build --release)
  cp "${ROOT_DIR}/target/release/model2vec-service" "${MODEL2VEC_BIN}"
  printf "%b║%b  model2vec-service built and installed.%b\n" "${RED}" "${GREEN}" "${RESET}"
else
  printf "%b║%b  model2vec-service already present, skipping.%b\n" "${RED}" "${YELLOW}" "${RESET}"
fi
native_binary_matches_arch "${MODEL2VEC_BIN}" || masterd_die "model2vec-service binary failed architecture validation"

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
