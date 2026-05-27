# Third-Party Notices

MASTERd is Copyright (c) 2026 Carlos Fundora and is licensed under the MIT
License. This file records credits and third-party components known to be used,
vendored, referenced, or packaged by this repository. The license files and
metadata inside each vendored project remain authoritative for that project.

## Required Attributions

- ROCm ATOM engine: credited as the source of runtime/engine inspiration for
  the AMD/ROCm-oriented local execution approach. ROCm and related marks belong
  to Advanced Micro Devices, Inc. MASTERd is not affiliated with or endorsed by
  AMD.
- Liquid AI LFM models: credited for the LFM-family model assets referenced or
  embedded by MASTERd, including LFM2.5 1.2B Thinking, LFM2.5 350M Instruct,
  and LFM2 ColBERT variants. Model weights, tokenizers, names, and usage terms
  remain governed by their upstream Liquid AI model licenses.
- Jina AI models: credited for Jina embedding model usage, including
  `jina-embeddings-v5-omni-small` and the local `jina-code-embed` endpoint name
  used by the embedding configuration. Model usage remains governed by Jina AI's
  upstream model licenses and terms.
- ColBERT / MaxSim reranking: credited for the late-interaction retrieval and
  MaxSim reranking approach used by the indexing pipeline.

## Vendored Source Packages

- Hugging Face Candle, vendored under `vendor/candle`.
  License: MIT OR Apache-2.0. See `vendor/candle/LICENSE-MIT` and
  `vendor/candle/LICENSE-APACHE`.
- Hugging Face Tokenizers, vendored under `vendor/tokenizers`.
  License: Apache-2.0 per `vendor/tokenizers/Cargo.toml`.
- Tauri, vendored under `vendor/tauri`.
  License: Apache-2.0 OR MIT. See `vendor/tauri/LICENSE_APACHE-2.0`,
  `vendor/tauri/LICENSE_MIT`, and `vendor/tauri/LICENSE.spdx`.

## Runtime and Integration Credits

- Meilisearch: referenced as a lexical/search sidecar integration.
- Valkey / Redis-compatible runtime: referenced as a hot-cache sidecar.
- FalkorDB / Falkor module: referenced as a vector graph mirror sidecar.
- LanceDB: referenced for vector snapshot/storage pipeline integration.
- Fluidsynth, TiMidity++, aplaymidi, pmidi, and WildMIDI: supported as optional
  system MIDI playback backends.
- `TimGM6mb.sf2`: bundled soundfont used by the Rust MIDI fallback. Keep the
  upstream soundfont license/notice with any redistributed installer or replace
  the asset with a soundfont whose license is documented in this repository.

## Rust and JavaScript Dependencies

MASTERd also depends on Rust crates listed in `Cargo.lock` and JavaScript
packages listed in `apps/masterd-shell/package-lock.json`, including React,
Next.js, Lucide React, Motion, Tailwind-related packages, Reqwest, Tokio,
Serde, Rusqlite, Redis, CPAL, Midly, OxiSynth, Rodio, and their transitive
dependencies. Their upstream licenses remain authoritative and must be
preserved when redistributing binary or source bundles.

## Trademark Notice

All product names, project names, model names, and trademarks are the property
of their respective owners. Attribution here does not imply endorsement,
sponsorship, or affiliation.
