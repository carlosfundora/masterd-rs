//! RAG context builder: local BM25 search + SearXNG web search.
//!
//! Local retrieval uses `masterd-index::LocalIndex` (BM25 Okapi).
//! Web results are deduplicated with `masterd-index::DocumentDeduper`.
//! Results from both are merged into a `[CONTEXT]` block injected into the prompt.

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use masterd_index::{DocumentDeduper, LocalIndex, SearchResult};

use crate::{SearchMode, WebSearchBackend};

/// A single web search result returned by SearXNG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Builds a `[CONTEXT]` block from local docs and/or the web.
///
/// The `index` handle is optional: when `None`, local retrieval is skipped.
/// Use `RagContextBuilder::with_index()` at engine-init to wire the live index.
pub struct RagContextBuilder {
    index: Option<Arc<RwLock<LocalIndex>>>,
}

impl RagContextBuilder {
    /// Create a builder without a local index (web-only mode).
    pub fn new() -> Self {
        Self { index: None }
    }

    /// Attach a shared local index so BM25 retrieval is active.
    pub fn with_index(index: Arc<RwLock<LocalIndex>>) -> Self {
        Self { index: Some(index) }
    }

    /// Set the index after construction (useful for deferred wiring).
    pub fn set_index(&mut self, index: Arc<RwLock<LocalIndex>>) {
        self.index = Some(index);
    }

    /// Returns `(context_block, web_citations)`.
    pub async fn build(
        &self,
        query: &str,
        mode: SearchMode,
        web: &WebSearchBackend,
    ) -> Result<(String, Vec<WebResult>)> {
        let mut sections: Vec<String> = vec![];
        let mut citations: Vec<WebResult> = vec![];

        // ── Local BM25 retrieval ─────────────────────────────────────────────
        if matches!(mode, SearchMode::LocalDocuments | SearchMode::Both) {
            let local = self.search_local(query).await;
            if !local.is_empty() {
                let mut block = String::from("[INDEXED DOCUMENTS]\n");
                for (i, r) in local.iter().enumerate() {
                    let path_hint = r
                        .path
                        .as_deref()
                        .map(|p| format!(" ({p})"))
                        .unwrap_or_default();
                    block.push_str(&format!(
                        "[{}] doc:{}{} — score:{:.2}\n{}\n",
                        i + 1,
                        r.doc_id,
                        path_hint,
                        r.score,
                        r.excerpt
                    ));
                }
                sections.push(block);
            }
        }

        // ── Web search ───────────────────────────────────────────────────────
        if matches!(mode, SearchMode::WebSearch | SearchMode::Both) {
            let raw_results = web.search(query, 12).await.unwrap_or_default();
            // Deduplicate before injecting into context
            let deduped = deduplicate_web_results(raw_results);

            if !deduped.is_empty() {
                let mut block = String::from("[WEB RESULTS]\n");
                let offset = citations.len();
                for (i, r) in deduped.iter().enumerate() {
                    block.push_str(&format!(
                        "[{}] {} — {}\n{}\n",
                        offset + i + 1,
                        r.title,
                        r.url,
                        r.snippet
                    ));
                }
                sections.push(block);
                citations.extend(deduped);
            }
        }

        let context_block = if sections.is_empty() {
            String::new()
        } else {
            format!("[CONTEXT]\n{}", sections.join("\n"))
        };

        Ok((context_block, citations))
    }

    /// BM25 retrieval from the local index. Returns top-8 results.
    async fn search_local(&self, query: &str) -> Vec<SearchResult> {
        let Some(index) = &self.index else {
            return vec![];
        };
        let idx = index.read().await;
        if idx.is_empty() {
            return vec![];
        }
        idx.search(query, 8)
    }
}

/// Deduplicate web results using `DocumentDeduper` (URL, title, content hash).
fn deduplicate_web_results(results: Vec<WebResult>) -> Vec<WebResult> {
    let mut deduper = DocumentDeduper::new();
    deduper.filter(results, |r| {
        (r.url.as_str(), r.title.as_str(), r.snippet.as_str())
    })
}

impl Default for RagContextBuilder {
    fn default() -> Self {
        Self::new()
    }
}
