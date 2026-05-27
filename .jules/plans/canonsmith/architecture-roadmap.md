# CanonSmith Architecture Roadmap

## Current Structural Goals
- Evaluate `vendor/` and `crates/` organization to ensure vendor crates are properly canonicalized under `vendor/`.
- Maintain clean boundary between first-party functionality and third-party code.

## Completed Migrations
- (Pending completion of current run) Move `crates/model2vec-rs` to `vendor/model2vec-rs`.

## Active Migration Candidates
- Move `crates/model2vec-rs` to `vendor/model2vec-rs` to establish a clear boundary for vendored code.
- Check if UI contracts are duplicated or disjointed between Rust and TS.

## Deferred / Risky Migrations
- None yet

## Canonical Homes Established
- `vendor/`: Vendored third-party source dependencies.
- `crates/`: Internal foundation and pipeline capabilities.

## Duplicate Concern Areas
- UI API contracts: `apps/masterd-shell/contracts/api.ts` vs `crates/masterd-ui-contract`. They represent the same boundary, but one is TS and the other is Rust.

## Reference Repair Risks
- Moving `crates/model2vec-rs` requires updating `Cargo.toml` workspace members and `services/model2vec-service/Cargo.toml`.
- Needs testing of scripts that may hardcode paths.

## Recommended Next Runs
- Run a consolidation of UI contracts to a single source of truth if feasible.
