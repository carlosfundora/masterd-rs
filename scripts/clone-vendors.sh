#!/usr/bin/env bash
set -euo pipefail

VENDOR_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/vendor"
mkdir -p "$VENDOR_ROOT"
cd "$VENDOR_ROOT"

clone_if_missing() {
  local name="$1"
  local url="$2"
  if [[ -d "$name" && ! -d "$name/.git" ]]; then
    echo "[clean] removing failed/corrupted vendor install: $name"
    /usr/bin/rm -rf "$name"
  fi

  if [[ -d "$name/.git" ]]; then
    echo "[skip] $name exists"
  else
    echo "[clone] $name"
    git clone --depth 1 "$url" "$name"
  fi
}

clone_if_missing "candle" "https://github.com/huggingface/candle.git"
clone_if_missing "tokenizers" "https://github.com/huggingface/tokenizers.git"
clone_if_missing "tauri" "https://github.com/tauri-apps/tauri.git"
clone_if_missing "lopdf" "https://github.com/J-F-Liu/lopdf.git"
clone_if_missing "iced" "https://github.com/iced-rs/iced.git"

if [[ -d "tesseract-rs" && ! -d "tesseract-rs/.git" ]]; then
  echo "[clean] removing failed/corrupted vendor install: tesseract-rs"
  /usr/bin/rm -rf "tesseract-rs"
fi

if [[ ! -d "tesseract-rs/.git" ]]; then
  echo "[clone] tesseract-rs"
  git clone --depth 1 https://github.com/cafercangundogdu/tesseract-rs.git tesseract-rs \
    || git clone --depth 1 https://github.com/antimatter15/tesseract-rs.git tesseract-rs
else
  echo "[skip] tesseract-rs exists"
fi

echo "Done. Vendored repos:"
for repo in candle tokenizers tauri lopdf iced tesseract-rs; do
  [[ -d "$repo/.git" ]] && echo "- $repo @ $(git -C "$repo" rev-parse --short HEAD)"
done
