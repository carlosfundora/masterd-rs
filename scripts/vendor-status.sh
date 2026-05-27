#!/usr/bin/env bash
set -euo pipefail

VENDOR_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/vendor"
for repo in candle tokenizers tauri lopdf iced tesseract-rs; do
  if [[ -d "$VENDOR_ROOT/$repo/.git" ]]; then
    printf "%-14s %s\n" "$repo" "$(git -C "$VENDOR_ROOT/$repo" rev-parse --short HEAD)"
  else
    printf "%-14s %s\n" "$repo" "MISSING"
  fi
done
