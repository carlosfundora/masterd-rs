//! LocalIndex: BM25-backed document store with symbol and path lookup.
//!
//! Thread-safety: DashMap for concurrent reads.  Write methods take `&mut self`
//! (callers wrap in `Arc<RwLock<LocalIndex>>` if needed from async contexts).

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::bm25::BM25Okapi;

/// A document stored in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDocument {
    pub doc_id: String,
    /// Optional filesystem path for path-based lookup.
    pub path: Option<String>,
    pub text: String,
    /// Symbol names extracted from the document (function names, headings, etc.).
    #[serde(default)]
    pub symbols: Vec<String>,
    /// Optional content-type tag (e.g. "pdf", "txt", "markdown").
    #[serde(default)]
    pub doc_type: Option<String>,
}

/// A scored search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub doc_id: String,
    pub score: f32,
    /// A short excerpt from the document (first 300 chars).
    pub excerpt: String,
    pub path: Option<String>,
}

/// In-memory document index with BM25 lexical search.
pub struct LocalIndex {
    documents: Vec<IndexedDocument>,
    tokenized_documents: Vec<Vec<String>>,
    /// term → doc_ids
    lexical: BTreeMap<String, BTreeSet<String>>,
    /// symbol name → doc_ids
    symbols: BTreeMap<String, BTreeSet<String>>,
    /// path → doc_id
    paths: BTreeMap<String, String>,
    bm25: Option<BM25Okapi>,
    /// Hot-cache: bounded LRU-style string KV store.
    hot_capacity: usize,
    hot_order: VecDeque<String>,
    hot_values: BTreeMap<String, String>,
}

impl LocalIndex {
    pub fn new(hot_capacity: usize) -> Self {
        Self {
            documents: Vec::new(),
            tokenized_documents: Vec::new(),
            lexical: BTreeMap::new(),
            symbols: BTreeMap::new(),
            paths: BTreeMap::new(),
            bm25: None,
            hot_capacity,
            hot_order: VecDeque::new(),
            hot_values: BTreeMap::new(),
        }
    }

    /// Insert (or replace by doc_id) a document.
    pub fn insert(&mut self, doc: IndexedDocument) {
        self.remove_internal(&doc.doc_id, false);

        let tokens = BM25Okapi::tokenize(&doc.text);
        for token in &tokens {
            self.lexical
                .entry(token.clone())
                .or_default()
                .insert(doc.doc_id.clone());
        }

        if let Some(path) = &doc.path {
            self.paths.insert(path.clone(), doc.doc_id.clone());
        }

        for sym in &doc.symbols {
            self.symbols
                .entry(sym.clone())
                .or_default()
                .insert(doc.doc_id.clone());
        }

        self.tokenized_documents.push(tokens);
        self.documents.push(doc);
        self.rebuild_bm25();
    }

    /// Remove a document by id.
    pub fn remove(&mut self, doc_id: &str) {
        self.remove_internal(doc_id, true);
    }

    fn remove_internal(&mut self, doc_id: &str, rebuild: bool) {
        if let Some(index) = self.documents.iter().position(|d| d.doc_id == doc_id) {
            self.documents.remove(index);
            self.tokenized_documents.remove(index);
        }
        self.documents.retain(|d| d.doc_id != doc_id);
        for ids in self.lexical.values_mut() {
            ids.remove(doc_id);
        }
        for ids in self.symbols.values_mut() {
            ids.remove(doc_id);
        }
        self.paths.retain(|_, id| id != doc_id);
        if rebuild {
            self.rebuild_bm25();
        }
    }

    fn rebuild_bm25(&mut self) {
        if self.tokenized_documents.is_empty() {
            self.bm25 = None;
            return;
        }
        self.bm25 = Some(BM25Okapi::new(
            self.tokenized_documents.clone(),
            None,
            None,
            None,
        ));
    }

    /// BM25 full-text search. Returns top-k results sorted by score descending.
    pub fn search(&self, query: &str, top_k: usize) -> Vec<SearchResult> {
        let Some(bm25) = &self.bm25 else {
            return vec![];
        };
        let ranked = bm25.rank(query, top_k);

        ranked
            .into_iter()
            .filter(|(_, score)| *score > 0.0)
            .map(|(idx, score)| {
                let doc = &self.documents[idx];
                SearchResult {
                    doc_id: doc.doc_id.clone(),
                    score,
                    excerpt: doc.text.chars().take(300).collect(),
                    path: doc.path.clone(),
                }
            })
            .collect()
    }

    /// Lookup docs that define a symbol.
    pub fn lookup_symbol(&self, symbol: &str) -> Vec<String> {
        self.symbols
            .get(symbol)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Lookup doc_id by path.
    pub fn lookup_path(&self, path: &str) -> Option<&str> {
        self.paths.get(path).map(String::as_str)
    }

    /// Document count.
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    // ── Hot cache ──────────────────────────────────────────────────────────────

    pub fn put_hot(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        if !self.hot_values.contains_key(&key) {
            self.hot_order.push_back(key.clone());
        }
        self.hot_values.insert(key.clone(), value.into());
        while self.hot_values.len() > self.hot_capacity {
            if let Some(old) = self.hot_order.pop_front() {
                if old != key {
                    self.hot_values.remove(&old);
                }
            } else {
                break;
            }
        }
    }

    pub fn get_hot(&self, key: &str) -> Option<&str> {
        self.hot_values.get(key).map(String::as_str)
    }

    /// Get all documents (for snapshot / ColBERT reranking prep).
    pub fn documents(&self) -> &[IndexedDocument] {
        &self.documents
    }

    pub fn get_document(&self, doc_id: &str) -> Option<&IndexedDocument> {
        self.documents.iter().find(|d| d.doc_id == doc_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_search() {
        let mut idx = LocalIndex::new(10);
        idx.insert(IndexedDocument {
            doc_id: "doc1".to_string(),
            path: Some("notes/rust.md".to_string()),
            text: "Rust ownership rules prevent data races at compile time".to_string(),
            symbols: vec!["ownership".to_string()],
            doc_type: None,
        });
        idx.insert(IndexedDocument {
            doc_id: "doc2".to_string(),
            path: None,
            text: "Python uses garbage collection for memory management".to_string(),
            symbols: vec![],
            doc_type: None,
        });

        let results = idx.search("rust memory compile", 5);
        assert!(!results.is_empty());
        assert_eq!(results[0].doc_id, "doc1");
    }

    #[test]
    fn rebuilds_cached_bm25_on_remove() {
        let mut idx = LocalIndex::new(4);
        idx.insert(IndexedDocument {
            doc_id: "doc1".to_string(),
            path: None,
            text: "alpha beta gamma".to_string(),
            symbols: vec![],
            doc_type: None,
        });
        idx.insert(IndexedDocument {
            doc_id: "doc2".to_string(),
            path: None,
            text: "beta gamma delta".to_string(),
            symbols: vec![],
            doc_type: None,
        });
        idx.remove("doc1");
        let results = idx.search("alpha", 5);
        assert!(results.is_empty());
        let results = idx.search("delta", 5);
        assert_eq!(results.first().map(|r| r.doc_id.as_str()), Some("doc2"));
    }

    #[test]
    fn symbol_lookup() {
        let mut idx = LocalIndex::new(4);
        idx.insert(IndexedDocument {
            doc_id: "d1".to_string(),
            path: None,
            text: "fn ownership_check() {}".to_string(),
            symbols: vec!["ownership_check".to_string()],
            doc_type: None,
        });
        assert_eq!(idx.lookup_symbol("ownership_check"), vec!["d1"]);
    }

    #[test]
    fn hot_cache_eviction() {
        let mut idx = LocalIndex::new(2);
        idx.put_hot("a", "1");
        idx.put_hot("b", "2");
        idx.put_hot("c", "3");
        assert_eq!(idx.get_hot("a"), None);
        assert_eq!(idx.get_hot("c"), Some("3"));
    }
}
