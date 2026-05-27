# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-05-26

### Added
- Created `install.sh` top-level installation entrypoint that bootstraps a local Python virtual environment `.venv-bootstrap` for stage-two setup.
- Created `scripts/bootstrap.py` which handles cloning missing or corrupted vendored repositories (`candle`, `tokenizers`, `tauri`, `lopdf`, `iced`, `tesseract-rs`), verifies the Rust toolchain (bootstrapping `rustup` if needed), and runs the Rust orchestrator.
- Implemented `run_installation_flow` inside `apps/masterd-bootstrap` (triggered with the `--install` flag) to orchestrate package management, model downloads, FastAPIs setups, sidecar builds, and full workspace compilation verification.

### Fixed
- Enforced CPU-only PyTorch index (`https://download.pytorch.org/whl/cpu`) and hardcoded `DEVICE = "cpu"` in all embedding services (`colbert-service`, `jina-service`, `qwen3-service`) for CPU-only Ryzen systems.
- Corrected the `tokenizers` crate path in `crates/masterd-chat-engine/Cargo.toml` to point to `../../vendor/tokenizers/tokenizers`.
- Improved prefetch handling in `setup-embedding-services.sh` to gracefully print warnings and proceed instead of throwing blocking exceptions if model weights or tokenizers fail to prefetch on startup.
- Cleaned up the Valkey build source folder (`target/valkey-src`) before compiling to wipe any residues from failed installations.
