//! Optional semantic reranking facade.
//!
//! MASTERd keeps this feature policy-free.  The `semantic` feature therefore
//! exposes a no-op reranker that preserves the BM25 result order until a
//! standalone semantic client is introduced.

#[cfg(feature = "semantic")]
use crate::local_index::SearchResult;

#[cfg(feature = "semantic")]
#[derive(Clone, Default)]
pub struct SemanticReranker;

#[cfg(feature = "semantic")]
impl SemanticReranker {
    pub fn from_env() -> Self {
        Self
    }

    pub fn new(_base_url: impl Into<String>, _max_inflight: usize) -> Self {
        Self
    }

    pub async fn rerank_results(
        &self,
        _query: &str,
        results: Vec<SearchResult>,
    ) -> Vec<SearchResult> {
        results
    }
}
