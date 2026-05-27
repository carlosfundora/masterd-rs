#!/usr/bin/env bash
# Shared helpers for MASTERd source installer scripts.

masterd_log() {
  printf "[masterd-install] %s\n" "$*" >&2
}

masterd_die() {
  printf "[masterd-install] ERROR: %s\n" "$*" >&2
  exit 1
}

masterd_prepend_path() {
  case ":${PATH}:" in
    *":$1:"*) ;;
    *) export PATH="$1:${PATH}" ;;
  esac
}

masterd_init_bootstrap() {
  MASTERD_ROOT_DIR="$1"
  MASTERD_TOOLCHAIN_DIR="${MASTERD_TOOLCHAIN_DIR:-${MASTERD_ROOT_DIR}/target/toolchains}"
  mkdir -p "${MASTERD_TOOLCHAIN_DIR}"
  masterd_prepend_path "${HOME}/.cargo/bin"
  masterd_prepend_path "${HOME}/.local/bin"
}

masterd_detect_arch() {
  local arch
  arch="$(uname -m)"
  case "${arch}" in
    x86_64|amd64)
      MASTERD_NODE_PLATFORM="linux-x64"
      ;;
    aarch64|arm64)
      MASTERD_NODE_PLATFORM="linux-arm64"
      ;;
    *)
      masterd_die "unsupported CPU architecture: ${arch}"
      ;;
  esac
}

masterd_download_atomic() {
  local url="$1"
  local dest="$2"
  local tmp="${dest}.tmp.$$"

  mkdir -p "$(dirname "${dest}")"
  rm -f "${tmp}"
  if ! curl -fL --retry 3 --retry-delay 2 --connect-timeout 30 "${url}" -o "${tmp}"; then
    rm -f "${tmp}"
    return 1
  fi
  mv "${tmp}" "${dest}"
}

masterd_without_python_index_env() {
  env \
    -u UV_EXTRA_INDEX_URL \
    -u UV_INDEX_URL \
    -u UV_CONSTRAINT \
    -u PIP_EXTRA_INDEX_URL \
    -u PIP_INDEX_URL \
    -u PIP_CONSTRAINT \
    "$@"
}

masterd_ensure_uv() {
  masterd_init_bootstrap "$1"
  if command -v uv >/dev/null 2>&1; then
    MASTERD_UV_BIN="$(command -v uv)"
    return 0
  fi

  command -v curl >/dev/null 2>&1 || masterd_die "curl is required to install uv"
  masterd_log "uv not found; installing uv for this user"
  masterd_without_python_index_env curl -LsSf https://astral.sh/uv/install.sh | sh >/dev/null
  masterd_prepend_path "${HOME}/.local/bin"
  masterd_prepend_path "${HOME}/.cargo/bin"

  if ! command -v uv >/dev/null 2>&1; then
    masterd_die "uv is still unavailable after bootstrap"
  fi
  MASTERD_UV_BIN="$(command -v uv)"
}

masterd_resolve_python() {
  masterd_init_bootstrap "$1"
  local requested="${PYTHON_BIN:-}"

  if [[ -n "${requested}" ]] && command -v "${requested}" >/dev/null 2>&1; then
    MASTERD_PYTHON_BIN="$(command -v "${requested}")"
    return 0
  fi
  if [[ -n "${requested}" ]]; then
    masterd_log "requested PYTHON_BIN not found: ${requested}; trying defaults"
  fi

  if command -v python3.12 >/dev/null 2>&1; then
    MASTERD_PYTHON_BIN="$(command -v python3.12)"
    return 0
  fi
  if command -v python3 >/dev/null 2>&1; then
    MASTERD_PYTHON_BIN="$(command -v python3)"
    return 0
  fi

  masterd_ensure_uv "$1"
  masterd_without_python_index_env "${MASTERD_UV_BIN}" python install 3.12
  MASTERD_PYTHON_BIN="$(masterd_without_python_index_env "${MASTERD_UV_BIN}" python find 3.12)"
  [[ -x "${MASTERD_PYTHON_BIN}" ]] || masterd_die "uv could not provide Python 3.12"
}

masterd_install_system_dep() {
  local cmd="$1"
  local pkg_deb="$2"
  local pkg_rpm="$3"
  local pkg_arch="$4"

  if command -v "${cmd}" >/dev/null 2>&1; then
    return 0
  fi

  masterd_log "System dependency '${cmd}' is missing. Attempting auto-install..."
  if command -v apt-get >/dev/null 2>&1; then
    sudo apt-get update -y || true
    sudo apt-get install -y "${pkg_deb}"
  elif command -v dnf >/dev/null 2>&1; then
    sudo dnf install -y "${pkg_rpm}"
  elif command -v yum >/dev/null 2>&1; then
    sudo yum install -y "${pkg_rpm}"
  elif command -v pacman >/dev/null 2>&1; then
    sudo pacman -Sy --noconfirm "${pkg_arch}"
  else
    masterd_die "Cannot install '${cmd}'. No supported package manager found (apt-get, dnf, yum, pacman). Please install '${pkg_deb}' manually."
  fi

  if ! command -v "${cmd}" >/dev/null 2>&1; then
    masterd_die "Failed to install '${cmd}'. Please install it manually."
  fi
}

masterd_ensure_source_build_tools() {
  masterd_init_bootstrap "$1"

  # First, ensure basic tools needed for bootstrap/downloads
  masterd_install_system_dep curl curl curl curl
  masterd_install_system_dep tar tar tar tar
  masterd_install_system_dep xz xz xz xz
  masterd_install_system_dep file file file file
  masterd_install_system_dep make make make make

  if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1 && ! command -v clang >/dev/null 2>&1; then
    masterd_install_system_dep gcc build-essential gcc gcc
  fi

  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    masterd_log "Rust toolchain not found; installing stable Rust via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
    masterd_prepend_path "${HOME}/.cargo/bin"
  fi

  command -v cargo >/dev/null 2>&1 || masterd_die "cargo is unavailable after rustup bootstrap"
  command -v rustc >/dev/null 2>&1 || masterd_die "rustc is unavailable after rustup bootstrap"
  command -v make >/dev/null 2>&1 || masterd_die "make is required to build bundled Valkey"
  command -v tar >/dev/null 2>&1 || masterd_die "tar is required to unpack installer assets"
  command -v xz >/dev/null 2>&1 || masterd_die "xz is required to unpack Node.js toolchains"
  command -v file >/dev/null 2>&1 || masterd_die "file is required to validate downloaded native binaries"

  if ! command -v cc >/dev/null 2>&1 && ! command -v gcc >/dev/null 2>&1 && ! command -v clang >/dev/null 2>&1; then
    masterd_die "a C compiler is required to build bundled Valkey"
  fi
}

masterd_install_node_toolchain() {
  masterd_init_bootstrap "$1"
  masterd_detect_arch

  local node_version="${MASTERD_NODE_VERSION:-v22.22.3}"
  local node_dir="${MASTERD_TOOLCHAIN_DIR}/node-${node_version}-${MASTERD_NODE_PLATFORM}"
  local archive="${MASTERD_TOOLCHAIN_DIR}/node-${node_version}-${MASTERD_NODE_PLATFORM}.tar.xz"
  local url="https://nodejs.org/dist/${node_version}/node-${node_version}-${MASTERD_NODE_PLATFORM}.tar.xz"

  if [[ ! -x "${node_dir}/bin/node" || ! -x "${node_dir}/bin/corepack" ]]; then
    command -v curl >/dev/null 2>&1 || masterd_die "curl is required to install Node.js automatically"
    command -v tar >/dev/null 2>&1 || masterd_die "tar is required to unpack Node.js"
    command -v xz >/dev/null 2>&1 || masterd_die "xz is required to unpack Node.js"
    masterd_log "installing Node.js ${node_version} for ${MASTERD_NODE_PLATFORM}"
    masterd_download_atomic "${url}" "${archive}"
    local tmp="${node_dir}.tmp.$$"
    rm -rf "${tmp}"
    mkdir -p "${tmp}"
    tar -xJf "${archive}" -C "${tmp}" --strip-components=1
    rm -rf "${node_dir}"
    mv "${tmp}" "${node_dir}"
  fi

  masterd_prepend_path "${node_dir}/bin"
}

masterd_ensure_pnpm() {
  masterd_init_bootstrap "$1"
  local pnpm_version="${MASTERD_PNPM_VERSION:-11.1.2}"

  if command -v pnpm >/dev/null 2>&1; then
    return 0
  fi

  if ! command -v corepack >/dev/null 2>&1; then
    masterd_install_node_toolchain "$1"
  fi

  command -v corepack >/dev/null 2>&1 || masterd_die "corepack is unavailable after Node.js bootstrap"
  corepack prepare "pnpm@${pnpm_version}" --activate
  if ! command -v pnpm >/dev/null 2>&1; then
    corepack pnpm --version >/dev/null
  fi
}

masterd_pnpm() {
  if command -v pnpm >/dev/null 2>&1; then
    pnpm "$@"
  else
    corepack pnpm "$@"
  fi
}
