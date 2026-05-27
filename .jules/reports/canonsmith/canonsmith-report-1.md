# CanonSmith Operational Report

## Run Mode
Migration

## Summary
Moved `crates/model2vec-rs` to `vendor/model2vec-rs` to enforce the boundary between first-party foundation crates and third-party vendored source dependencies.

## Operational Memory Loaded
Yes. Initialized Journals, Manifests, Plans, Reports.

## Preferences Applied
Enforced `vendor/` as the canonical home for third-party vendored code.

## Concern Map Updates
Recorded `apps/`, `crates/`, `services/`, and `vendor/` base directories.
Recorded duplicated UI contracts (TS vs Rust).

## Roadmap Updates
Target UI contracts for consolidation in a future run.

## Selected Target
`crates/model2vec-rs` -> `vendor/model2vec-rs`

## Work Performed
1. Moved `crates/model2vec-rs` to `vendor/model2vec-rs`.
2. Updated references in `Cargo.toml`, `services/model2vec-service/Cargo.toml`, `README.md`, `apps/masterd-engine-check/src/main.rs`.
3. Validated workspace compilation, formatting, and tests.

## Files Moved
- `crates/model2vec-rs/` -> `vendor/model2vec-rs/`

## Canonical Homes Clarified
`vendor/`: Only vendored third-party code.
`crates/`: Only internal first-party foundation capabilities.

## References Repaired
- `Cargo.toml`
- `services/model2vec-service/Cargo.toml`
- `README.md`
- `apps/masterd-engine-check/src/main.rs`

## Documentation Updated
`README.md` paths pointing to `crates/model2vec-rs` changed to `vendor/model2vec-rs`.

## Incremental Commits and Pushes
N/A (Agent handles submission in the final step).

## Baseline Validation
Unknown/Failed protobuf. Installed protobuf and ALSA system libs. Compilation succeeds.

## Post-Change Validation
`cargo test --workspace` passed successfully.

## Risks and Tradeoffs
Minimal risk. This is a path move and reference updates.

## Rollback Notes
Revert the commit cleanly.

## Recommended Next Runs
Investigate `apps/masterd-shell/contracts/api.ts` vs `crates/masterd-ui-contract` to establish a single canonical source of truth for the frontend UI contract.
