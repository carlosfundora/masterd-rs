# Changelog

All notable changes to this project will be documented in this file.

## [0.1.4] - 2026-05-27

### Added
- Exposed Preference Learning, Classification Learning, and Entity Extraction configurations in settings panel and synced with Rust backend config structures.
- Injected authoritarian, cold, and hilariously hostile MASTERd AI persona system instructions into ChatPanel prompts.
- Customized buttons, placeholders, empty states, and log notes in Intake, Rules, and Review Queue views to adopt the overconfident, slightly contemptuous tone.

## [0.1.3] - 2026-05-27

### Added
- Implemented settings validation in Tauri command `settings_save` to enforce safe bounds on generation temperature, safety confidence percent, and BM25 top K.
- Integrated exponential backoff retry and stale lock cleanup to the `AtomicHashIndexService` advisory locking mechanism.

### Changed
- Refactored Tauri desktop service health queries to use non-blocking `reqwest::Client` probes executed concurrently via `tokio::join!`, replacing blocking HTTP calls.
- Refactored `intake_add_files` command to run ingestion inside `tokio::task::spawn_blocking` and return robust error/fallback results.
- Added E0382 safety improvements in the `preferences_draft_policy` command to clone arguments before sending to database worker thread.
- Added checking checks to the `install.sh` python setup helper to verify installation success.
- Added `chrono` formatting support to log entries.

## [0.1.2] - 2026-05-27

### Added
- Aligned Tauri desktop command parameters with the JS bridge casing so invoke payloads map cleanly across intake, actions, rules, audit, and chat commands.
- Added live model health checks for the desktop status panel, including embedded chat model loading state plus ColBERT and Jina HTTP health probes.

## [0.1.1] - 2026-05-27

### Added
- Configured Tauri desktop UI startup lifecycle to automatically spawn supervised Python FastAPI processes for the local embedding and ColBERT reranker services (`colbert-service`, `jina-service`).
- Implemented background preloading of embedded LFM2.5 thinking and instruct models on Tauri setup, eliminating first-message response latency.

## [0.1.0] - 2026-05-26

### Added
- Created `install.sh` top-level installation entrypoint that bootstraps a local Python virtual environment `.venv-bootstrap` for stage-two setup.
- Created `scripts/bootstrap.py` which handles cloning missing or corrupted vendored repositories (`candle`, `tokenizers`, `tauri`, `lopdf`, `iced`, `tesseract-rs`), verifies the Rust toolchain (bootstrapping `rustup` if needed), and runs the Rust orchestrator.
- Implemented `run_installation_flow` inside `apps/masterd-bootstrap` (triggered with the `--install` flag) to orchestrate package management, model downloads, FastAPIs setups, sidecar builds, and full workspace compilation verification.

### Fixed
- Enforced CPU-only PyTorch index (`https://download.pytorch.org/whl/cpu`) and hardcoded `DEVICE = "cpu"` in all embedding services (`colbert-service`, `jina-service`) for CPU-only Ryzen systems.
- Corrected the `tokenizers` crate path in `crates/masterd-chat-engine/Cargo.toml` to point to `../../vendor/tokenizers/tokenizers`.
- Improved prefetch handling in `setup-embedding-services.sh` to gracefully print warnings and proceed instead of throwing blocking exceptions if model weights or tokenizers fail to prefetch on startup.
- Cleaned up the Valkey build source folder (`target/valkey-src`) before compiling to wipe any residues from failed installations.
