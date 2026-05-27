# CanonSmith Concern Map

## Major Systems
- apps: App entrypoints (masterd-desktop-tauri, masterd-ingest, masterd-shell, masterd-tune)
- crates: Core functionality blocks (masterd-core, masterd-pipeline, masterd-index, etc)
- services: Sidecar and local services (colbert-service, jina-service, model2vec-service)
- vendor: App-local dependencies (candle, iced, lopdf, tauri, tesseract-rs, tokenizers)

## Ambiguous Ownership Zones
- (Resolved in current run) `model2vec-rs` belongs in `vendor/`, not `crates/`, because it is third-party vendored code.

## Duplicate Concern Areas
- UI contracts exist in both `apps/masterd-shell/contracts/api.ts` and `crates/masterd-ui-contract`.
