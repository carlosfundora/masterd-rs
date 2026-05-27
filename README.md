# MASTERd — Machine-Assisted Sorting, Tagging, and Extraction of Records and Documents

> **File disarray is a solvable defect. Organize or be organized.**

MASTERd is a fully Rust-first document intelligence platform with a Tauri desktop UI. It ingests, classifies, deduplicates, semantically indexes, and retrieves documents using a multi-stage pipeline backed by embedded LFM2.5 GGUF models — with Ollama as a transparent fallback. Zero cloud dependencies. AMD ROCm–first. Runs entirely on your machine.

---

## Features

- **Tauri desktop app** — Next.js frontend wired to a native Rust backend via typed Tauri commands
- **Multi-stage ingestion pipeline** — hash → hot-cache → dedup → canonical SQLite write → LanceDB vector snapshot → Meilisearch lexical index → ColBERT rerank queue → Falkor graph mirror
- **Embedded GGUF inference** — LFM2.5-1.2B-Thinking and LFM2.5-350M-Instruct bundled as `include_bytes!` assets, loaded via llama.cpp; Ollama fallback when models are unavailable
- **ColBERT MaxSim reranker** — L2-normalized token-matrix reranking (correct cosine similarity, not raw dot product)
- **Python embedding services** — FastAPI servers for ColBERT, Jina v3, and Qwen3-Embedding; setup script enforces AMD ROCm PyTorch index
- **Supervised sidecar processes** — Meilisearch and Valkey managed by `SidecarSupervisor`, with optional FalkorDB graph module
- **AMD ROCm–first** — all Python installs routed through ROCm PyTorch index; CUDA wheels blocked by `config/rocm-constraints.txt`
- **Boot MIDI player** — ambient music on app launch, pure Rust

## Architecture

```
apps/
  masterd-desktop-tauri/   ← Tauri 2.x native host
  masterd-shell/           ← Next.js 14 UI
  masterd-ingest/          ← CLI document ingestion tool
  masterd-bootstrap/       ← Sidecar validation & first-launch setup
  masterd-engine-check/    ← Inference + retrieval integration tests
  masterd-tune/            ← AMD kernel auto-tuner
  masterd-midi-player/     ← Boot music player

crates/
  masterd-chat-engine/     ← Embedded GGUF chat + Ollama fallback
  masterd-embed-engine/    ← Local embedding stack (ColBERT / Jina / Qwen3)
  masterd-index/           ← ColBERT reranker, atomic hash-index dedup
  masterd-pipeline/        ← Typed stage-graph, retrieval pipeline, naming rules
  masterd-core/            ← Shared types, cancellation contract
  masterd-prompt-core/     ← MASTERd personality + avatar prompt registry
  masterd-runtime-tune/    ← Startup-safe AMD kernel profiler
  masterd-sidecars/        ← Sidecar topology validation
  masterd-ui-contract/     ← Typed Tauri event contract

models/
  lfm2.5-1.2b-thinking/   ← LFM2.5-1.2B-Thinking Q8_0 GGUF + tokenizer
  lfm2.5-350m-instruct/   ← LFM2.5-350M-Instruct Q8_0 GGUF + tokenizer
  lfm2-colbert-350m/      ← LFM2-ColBERT-350M Q8_0 GGUF (reranker)
  masterd-identity/       ← MASTERd system personality prompt

services/
  colbert-service/         ← FastAPI ColBERT HTTP server (port 11450)
  jina-service/            ← FastAPI Jina v3 HTTP server (port 11447)
  qwen3-service/           ← FastAPI Qwen3-Embedding HTTP server (port 11502)

vendor/
  candle/                  ← Hugging Face Candle ML framework (vendored)
  tauri/                   ← Tauri framework source (vendored)
  tokenizers/              ← HF Tokenizers (vendored)
```

## Requirements

### Build tools
- **Rust** ≥ 1.85 (nightly, see `rust-toolchain.toml`)
- **Node.js** >= 20 + pnpm
- **Tauri CLI** — `cargo install tauri-cli`
- **curl** or Python `huggingface-hub` — used by `scripts/download-models.sh`

### Runtime sidecars (downloaded by build script)
- **Meilisearch** v1.8.3 — lexical search engine
- **Valkey** v7.2.5 — hot-cache and dedup store
- **FalkorDB** (optional) — graph relationship queries

### Python embedding services (AMD ROCm)
- **Python 3.12**
- **uv** — `curl -LsSf https://astral.sh/uv/install.sh | sh`
- **AMD ROCm 6.x or 7.x** runtime

## Quick start

```bash
# 1. Clone the repo
git clone https://github.com/carlosfundora/masterd-rs
cd masterd-rs

# 2. Provide a Hugging Face token for gated Liquid AI model repos
#    (GGUF files are not stored in git — too large)
export HF_TOKEN=hf_your_token_here  # required for gated Liquid AI repos

# Optional: pre-download model weights, tokenizers, and chat templates now
./scripts/download-models.sh

# Optional: verify existing local model files without downloading
./scripts/download-models.sh --verify-only

# 3. Bootstrap: validates sidecar config and creates first-launch directories
cargo run -p masterd-bootstrap

# 4. Build the full installer bundle
#    Runs model install/verification first, then sidecars, frontend, and Tauri.
./scripts/build-installer-bundles.sh

# — OR — run the desktop app directly in dev mode:
cd apps/masterd-desktop-tauri
cargo tauri dev
```

## Ingest documents

```bash
cargo run -p masterd-ingest -- --root /path/to/your/documents
```

Pipeline stages (configurable in `config/pipeline.toml`):
1. Rapid SHA-256 hash
2. Valkey hot-cache write (offline fallback if Valkey unavailable)
3. Rigorous dedup gate
4. Canonical SQLite write (`data/masterd.db`)
5. LanceDB vector snapshot queue
6. ColBERT rerank queue
7. Meilisearch lexical queue
8. Jina omni multimodal queue (optional)
9. Falkor graph mirror queue

## Python embedding services (AMD ROCm)

The embedding services run as separate HTTP processes. The main installer sets them up by default, including Jina model prefetch. All installs are routed through the AMD ROCm PyTorch index — no CUDA wheels are permitted.

> [!NOTE]
> Whenever the Tauri desktop UI is launched, it automatically starts the embedding services (ColBERT, Jina, Qwen3) as supervised processes and preloads the embedded LFM2.5 thinking and instruct models.
> 
> You can also start the services manually for CLI tools or development:

```bash
# Set up all three service venvs (Python 3.12 + ROCm torch)
./scripts/setup-embedding-services.sh all

# Skip embedding-service setup during installer builds only when needed:
MASTERD_SKIP_EMBEDDING_SERVICES=1 ./scripts/build-installer-bundles.sh

# Start a service manually
services/colbert-service/.venv/bin/python services/colbert-service/server.py
services/jina-service/.venv/bin/python    services/jina-service/server.py
services/qwen3-service/.venv/bin/python   services/qwen3-service/server.py
```

Service endpoints (when running):
| Service | Port | Role |
|---------|------|------|
| ColBERT | 11450 | Token-matrix reranking |
| Jina v3 | 11447 | Dense code/text embeddings |
| Qwen3-Embedding | 11502 | Dense semantic embeddings |

Switch backend in `config/embedding_engine.toml` or env vars:
```bash
export MASTERD_INFERENCE_BACKEND=http   # use HTTP service endpoints
export MASTERD_INFERENCE_BACKEND=direct # self-contained Rust (default)
```

## Engine validation

```bash
cargo run -p masterd-engine-check -- --chat-url http://127.0.0.1:3000
# Report written to: data/engine_validation.json
```

## AMD kernel auto-tuner

```bash
cargo run -p masterd-tune -- --auto     # startup-safe tune
cargo run -p masterd-tune -- --retune   # full retune
```

AMD profiles live in `config/amd_profiles/`. Kernel manifest at `config/kernel_manifest.toml`.

## Ollama fallback

MASTERd automatically falls back to Ollama when embedded models fail to load:

1. Tries to load embedded GGUF model from `assets/models/`
2. On any failure, calls `http://127.0.0.1:11434` (configurable in Settings)
3. Uses `resolve_model()` — picks the configured model name or the first available Ollama model
4. Same `ChatToken` streaming interface; model badge shows `ollama/<model>`

Configure in the desktop app → Settings → **Ollama Fallback Engine**.

## Boot music

```bash
cargo run -q -p masterd-midi-player -- --seconds 8
# Disable: export MASTERD_NO_MUSIC=1
```

## Configuration

| File | Purpose |
|------|---------|
| `config/pipeline.toml` | Stage order, vector authority, cache engine |
| `config/embedding_engine.toml` | Model URLs, batch size, backend mode |
| `config/sidecars.toml` | Sidecar process topology |
| `config/kernel_manifest.toml` | AMD kernel pack registry |
| `config/rocm-constraints.txt` | Blocks CUDA wheels in all Python installs |
| `uv.toml` | ROCm PyTorch index configuration for uv |

## License and author

MASTERd is licensed under the MIT License.

**Author:** Carlos Fundora <sentseven@gmail.com>

Third-party credits and attribution notes are in `THIRD_PARTY_NOTICES.md`.

## What was set up

- Rust workspace with foundation crates:
  - `crates/masterd-core` (shared capability model)
  - `crates/masterd-prompt-core` (MASTERd personality + avatar prompt registry)
  - `crates/masterd-pipeline` (hash→cache→dedup→index pipeline interfaces)
  - `crates/masterd-sidecars` (sidecar topology + validation)
  - `apps/masterd-bootstrap` (validates sidecar config and bootstrap assumptions)
  - `apps/masterd-desktop-tauri` (desktop shell stub for upcoming Tauri UI wiring)
- Sidecar topology config at `config/sidecars.toml`
- Pipeline architecture config at `config/pipeline.toml`
- Vendor helper scripts in `scripts/`

## Vendor repos cloned

App-local source dependencies live under `vendor/`:

- `candle`
- `tokenizers`
- `tauri`
- `lopdf`
- `tesseract-rs`
- `iced`

## Embedding model (critical)

You can ship a **single installer** that includes everything, but not all of these should be one in-process binary:

- **Meilisearch**: run as supervised sidecar process
- **Valkey**: run as supervised sidecar process
- **Falkor module**: load into Valkey/Redis sidecar (`--loadmodule`)
- **LanceDB**: in-process Rust crate integration (not a daemon)

This repo enforces that model via `masterd-sidecars::validate_foundation()`.

## Personality + prompt port scope

- MASTERd personality source is consolidated in:
  - `models/masterd-identity/masterd_personality_prompt.txt`
- Rust prompt registry loader:
  - `crates/masterd-prompt-core`

## Quick start

```bash
cd /home/local/ai/projects/MASTERd
cargo run -p masterd-bootstrap
```

## Ingest pipeline run

```bash
cd /home/local/ai/projects/MASTERd
cargo run -p masterd-ingest -- --root /path/to/files
```

Pipeline order implemented (deterministic runtime; configurable via `config/pipeline.toml` `[runtime].stage_order`):
1. Rapid hash
2. Valkey hot-cache write (with offline fallback)
3. Rigorous dedup gate
4. Canonical SQLite write (`data/masterd.db`)
5. Lance snapshot queue (`data/lancedb_snapshots.jsonl`)
6. ColBERT CPU rerank queue (`data/colbert_rerank_queue.jsonl`)
7. Meilisearch lexical queue (`data/meilisearch_queue.jsonl`)
8. Optional Jina omni multimodal queue (`data/jina_omni_queue.jsonl`)
9. Falkor mirror queue (`data/falkor_queue.jsonl`)

## Embedded 3-model local inference setup (ported)

Copied from your atom-rs/gfxatom runtime pattern:

- ColBERT wrapper: `http://127.0.0.1:11450` (`colbert-lfm2-305m`)
- Qwen3 embeddings: `http://127.0.0.1:11502` (`qwen3-embedding`)
- Jina embeddings: `http://127.0.0.1:11447` (`jina-code-embed`)

Config file: `config/embedding_engine.toml`  
Env overrides supported: `MEMORYBANK_COLBERT_WRAPPER_URL`, `MEMORYBANK_QWEN3_URL`, `MEMORYBANK_JINA_URL`, `MEMORYBANK_EMBED_CONCURRENCY`.

Backend mode:
- `MASTERD_INFERENCE_BACKEND=direct` (default): self-contained Rust direct calls (no local model HTTP servers required)
- `MASTERD_INFERENCE_BACKEND=http`: use the 3 local endpoint wrappers above

To run ingest + engine verification/benchmark:

```bash
cd /home/local/ai/projects/MASTERd
cargo run -p masterd-ingest -- --root /path/to/files --verify-engine true --benchmark-engine true
```

To validate inference + retrieval + optional thinking chat and write a report:

```bash
cd /home/local/ai/projects/MASTERd
cargo run -p masterd-engine-check -- --chat-url http://127.0.0.1:3000
```

Report output path (default): `data/engine_validation.json`.

## AMD-first installer + auto-tuning

Profiles and kernel manifest:
- `config/amd_profiles/*.toml`
- `config/kernel_manifest.toml`

Run startup-safe tune:

```bash
cd /home/local/ai/projects/MASTERd
cargo run -p masterd-tune -- --auto
```

Run full retune:

```bash
cd /home/local/ai/projects/MASTERd
cargo run -p masterd-tune -- --retune
```

Build installer bundle:

```bash
cd /home/local/ai/projects/MASTERd
./scripts/build-installer-bundles.sh
```

Installer sequence:
1. Launch boot MIDI unless `MASTERD_NO_MUSIC=1`.
2. Run `scripts/download-models.sh` to install model weights, tokenizers, and chat templates unless `MASTERD_SKIP_MODEL_DOWNLOAD=1`.
3. Run `scripts/setup-embedding-services.sh all` unless `MASTERD_SKIP_EMBEDDING_SERVICES=1`.
4. Download/build sidecars, build the Next shell, and package Tauri.

## Boot screen + Rust music

- Installer launch shows the ANSI boot logo and waits for Enter.
- Boot music is played by bundled Rust app: `apps/masterd-midi-player`.
- Disable installer music with `MASTERD_NO_MUSIC=1`.

Run music player directly:

```bash
cd /home/local/ai/projects/MASTERd
cargo run -q -p masterd-midi-player -- --seconds 8
```

---

## Aggressive Rust pipeline surfaces (superior-by-default)

All critical pipeline capabilities are now fully implemented in Rust.

### Stage graph runtime (`crates/masterd-pipeline`)

Typed, deterministic stage-graph with cooperative cancellation and rollback hooks. Configure stage order in `config/pipeline.toml` under `[runtime].stage_order`.

### Telemetry taxonomy (`masterd_pipeline::telemetry`)

Machine-actionable failure classes, per-stage counters, and wall-clock timing:

```
FailureClass: TransientIo | CorruptInput | DependencyUnavailable | ResourceExhausted | PolicyRejected | Cancelled | InternalError
```

Each class carries `is_retryable()` and `is_expected()` predicates for automated triage.

### Naming and routing (`masterd_pipeline::naming`)

Rule-pack loader + deterministic priority resolver. Rule packs live in `config/naming/*.json`.

```bash
# Resolve a file path to route + canonical name (example usage in ingest):
cargo run -p masterd-ingest -- --root /path/to/files
```

### Retrieval pipeline (`masterd_pipeline::retrieval`)

Typed query parser + multi-stage retrieval + dedup-merge + rerank hooks as the default search path.  Query syntax: `terms... key:value top:N mode:(lexical|semantic|hybrid)`.

### UI workflow contract (`crates/masterd-ui-contract`)

Typed Tauri/Iced event contract for the review queue, operator commands, and correction loop.  All events are namespaced under `masterd://` for Tauri routing.

### Cancellation contract (`crates/masterd-core`)

`CancellationSource` / `CancellationToken` — cooperative, reason-carrying cancellation across all long-running pipeline stages.

### Extraction fallback policy (`crates/masterd-embed-engine`)

Centralized multi-provider fallback with bounded retries, quality scoring, and full audit trail via `ExtractionExecutionReport`.

### Atomic hash-index dedup (`apps/masterd-ingest`)

`AtomicHashIndexService` with advisory lock-file, atomic tmp→rename write, and `Drop`-based lock cleanup. Concurrency-safe across threads.
