//! MASTERd document index — BM25 lexical retrieval, ColBERT MaxSim reranking,
//! and web-result deduplication.
//!
//! Design goals:
//! - Zero network deps at retrieval time (pure in-process)
//! - Snapshot/restore so the index survives app restarts
//! - Rayon parallelism for score_pool on 16+ candidates
//! - Thread-safe via DashMap + Arc — no global locks during reads

pub mod bm25;
pub mod colbert;
pub mod dedup;
pub mod local_index;
pub mod semantic;
pub mod snapshot;

pub use bm25::BM25Okapi;
pub use colbert::{maxsim, rerank};
pub use dedup::DocumentDeduper;
pub use local_index::{IndexedDocument, LocalIndex, SearchResult};
pub use snapshot::IndexSnapshot;

#[cfg(feature = "semantic")]
pub use semantic::SemanticReranker;
