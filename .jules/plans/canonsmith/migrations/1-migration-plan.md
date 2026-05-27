# CanonSmith Migration Plan

## Selected Migration
Move `crates/model2vec-rs` to `vendor/model2vec-rs`.

## Why This Migration Matters
The repository currently has a specific `vendor/` directory for vendored source dependencies (candle, tauri, tokenizers, etc). However, `model2vec-rs` is a vendored third-party dependency that is currently placed inside the `crates/` directory. Moving it to `vendor/model2vec-rs` enforces the canonical separation of concerns: `crates/` is for first-party internal foundation crates, while `vendor/` is for third-party vendored code.
This aligns with principle 5: Deterministic file placement by topic, function, system, and subsystem.

## Current Structure
`crates/model2vec-rs`

## Target Structure
`vendor/model2vec-rs`

## Files / Directories to Move
- `crates/model2vec-rs/` -> `vendor/model2vec-rs/`

## Ownership Decision
- `vendor/` is explicitly owned by third-party vendor code.
- `model2vec-rs` is vendored.

## References to Repair
- `Cargo.toml` (workspace members list needs updating)
- `services/model2vec-service/Cargo.toml` (path to model2vec-rs dependency)
- `apps/masterd-engine-check/src/main.rs` (if it refers to the path directly)
- `README.md` references to `crates/model2vec-rs`.
- `crates/model2vec-rs/README.md` (no paths needing repair inside, just the markdown text references)
- `scripts/vendor-status.sh` or similar if they hardcode it.

## Documentation Path Updates
- `README.md` (update documentation references to point to `vendor/model2vec-rs`).

## Validation Plan
1. `cargo check --workspace`
2. `cargo test --workspace`
3. Verify paths in docs.

## Incremental Commit Plan
1. Update concern map and architecture roadmap.
2. Execute move and perform reference updates.
3. Validate.
4. Commit: "CanonSmith: Relocate vendored model2vec-rs to canonical vendor directory"

## Rollback Plan
- Revert the `git mv` and restore Cargo.toml.

## Risk Assessment
Low. The move is isolated to rust package paths. We will update `Cargo.toml` correctly.
