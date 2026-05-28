## ⚡ Bolt Learnings - masterd-data N+1 Insert Optimization

- **Issue**: `save_embeddings_and_index` (specifically inside `write_embeddings`) used `tx.execute` inside an iteration loop over `chunks`.
- **Learning**: Re-parsing and re-preparing a SQLite statement inside a high-iteration loop introduces measurable compilation overhead. Moving `tx.prepare` outside the loop and using `stmt.execute` improved insert performance for 100-chunk batches from ~315 µs to ~82 µs (a >3.5x speedup) based on Criterion benchmarks.
- **Verification**: Changes were localized to `crates/masterd-data/src/lib.rs`. Checked for side effects related to dropping prepared statements before `tx.commit()`.
