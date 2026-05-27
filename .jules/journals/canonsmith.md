## 2024-05-27T19:40:00 - Migrate model2vec-rs to vendor

### Run Mode
migration

### What I Learned
The codebase separates internal first-party libraries (`crates/`) from external third-party vendored dependencies (`vendor/`). However, `model2vec-rs` was erroneously placed in `crates/`.

### What I Organized
Planned the movement of `crates/model2vec-rs` to `vendor/model2vec-rs` to comply with structural boundaries.

### Canonicality Decisions
`vendor/` is the strict canonical home for third-party source code.
`crates/` is reserved exclusively for the internal foundation codebase.

### References Repaired
Planned updates for `Cargo.toml`, service configs, app code, and docs.

### Risks Found
UI Contracts are defined in two places (`api.ts` vs `masterd-ui-contract`).

### Useful Notes for Next Run
Look into unifying the frontend and backend UI event/contract typing using a generated schema approach or single source of truth.

### Recommended Next Targets
Consolidate UI contracts.
