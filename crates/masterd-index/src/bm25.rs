//! BM25 Okapi ranking — ported from rs_retrieval_core / rs_bm25_legacy.
//!
//! Tokenizes on whitespace and non-alphanumerics, computes per-document IDF
//! with an epsilon floor for negative-IDF terms, and ranks by BM25 score.

use std::collections::HashMap;

/// BM25 Okapi scorer.
pub struct BM25Okapi {
    corpus_size: usize,
    avgdl: f64,
    doc_freqs: Vec<HashMap<String, usize>>,
    idf: HashMap<String, f64>,
    doc_len: Vec<usize>,
    k1: f64,
    b: f64,
}

impl BM25Okapi {
    /// Tokenize: lowercase, split on non-alphanumeric (keep `_` and `-`), drop singles.
    pub fn tokenize(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .filter(|w| !w.is_empty() && w.len() > 1)
            .map(|w| w.to_string())
            .collect()
    }

    /// Build from pre-tokenized documents.
    pub fn new(
        corpus: Vec<Vec<String>>,
        k1: Option<f64>,
        b: Option<f64>,
        epsilon: Option<f64>,
    ) -> Self {
        let k1 = k1.unwrap_or(1.5);
        let b = b.unwrap_or(0.75);
        let epsilon = epsilon.unwrap_or(0.25);

        let corpus_size = corpus.len();
        let mut doc_len = Vec::with_capacity(corpus_size);
        let mut doc_freqs = Vec::with_capacity(corpus_size);
        let mut nd: HashMap<String, usize> = HashMap::new();
        let mut total_tokens = 0usize;

        for doc in &corpus {
            doc_len.push(doc.len());
            total_tokens += doc.len();
            let mut freqs: HashMap<String, usize> = HashMap::new();
            for word in doc {
                *freqs.entry(word.clone()).or_insert(0) += 1;
            }
            for word in freqs.keys() {
                *nd.entry(word.clone()).or_insert(0) += 1;
            }
            doc_freqs.push(freqs);
        }

        let avgdl = if corpus_size > 0 {
            total_tokens as f64 / corpus_size as f64
        } else {
            0.0
        };

        let mut idf: HashMap<String, f64> = HashMap::new();
        let mut sum_idf = 0.0;
        let mut negative_words: Vec<String> = Vec::new();

        for (word, freq) in &nd {
            let idf_val =
                ((corpus_size as f64 - *freq as f64 + 0.5) / (*freq as f64 + 0.5) + 1.0).ln();
            idf.insert(word.clone(), idf_val);
            sum_idf += idf_val;
            if idf_val < 0.0 {
                negative_words.push(word.clone());
            }
        }

        let average_idf = if !idf.is_empty() {
            sum_idf / idf.len() as f64
        } else {
            0.0
        };
        let eps_floor = epsilon * average_idf;
        for word in negative_words {
            idf.insert(word, eps_floor);
        }

        Self { corpus_size, avgdl, doc_freqs, idf, doc_len, k1, b }
    }

    /// Score a query against all documents.  Returns (doc_index, score) sorted descending.
    pub fn rank(&self, query: &str, top_k: usize) -> Vec<(usize, f32)> {
        let query_tokens = Self::tokenize(query);
        if query_tokens.is_empty() || self.corpus_size == 0 {
            return vec![];
        }

        let mut scores: Vec<(usize, f32)> = (0..self.corpus_size)
            .map(|i| {
                let score = self.score_doc(i, &query_tokens);
                (i, score as f32)
            })
            .collect();

        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        scores.truncate(top_k);
        scores
    }

    fn score_doc(&self, idx: usize, query_tokens: &[String]) -> f64 {
        let dl = self.doc_len[idx] as f64;
        let norm = 1.0 - self.b + self.b * dl / self.avgdl.max(1.0);

        query_tokens.iter().fold(0.0, |acc, term| {
            let idf = self.idf.get(term).copied().unwrap_or(0.0);
            if idf == 0.0 {
                return acc;
            }
            let tf = self.doc_freqs[idx].get(term).copied().unwrap_or(0) as f64;
            let tf_norm = (tf * (self.k1 + 1.0)) / (tf + self.k1 * norm);
            acc + idf * tf_norm
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ranking() {
        let docs = vec![
            BM25Okapi::tokenize("the quick brown fox jumps over the lazy dog"),
            BM25Okapi::tokenize("a fast red fox leaps quickly"),
            BM25Okapi::tokenize("lazy dogs sleep all day"),
        ];
        let bm25 = BM25Okapi::new(docs, None, None, None);
        let results = bm25.rank("quick fox", 3);
        assert!(!results.is_empty());
        // First result should be one of the fox docs
        assert!(results[0].0 < 2);
    }
}
