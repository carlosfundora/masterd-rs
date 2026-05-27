//! ColBERT MaxSim reranker — ported from rs_colbert/scorer.rs.
//!
//! Late-interaction scoring: for each query token, find the most similar doc token
//! (cosine dot product), then sum these max-similarities across query tokens.
//!
//! `score_pool` parallelizes over ≥16 docs using Rayon.

use rayon::prelude::*;

use crate::local_index::SearchResult;

// ── L2 normalization ─────────────────────────────────────────────────────────

/// L2-normalize each row of a flat token matrix in-place.
/// Rows with near-zero norm are zeroed out rather than producing NaN.
fn normalize_rows(matrix: &mut Vec<f32>, n_tokens: usize, dim: usize) {
    for row in matrix.chunks_mut(dim).take(n_tokens) {
        let norm: f32 = row.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for v in row.iter_mut() { *v /= norm; }
        } else {
            for v in row.iter_mut() { *v = 0.0; }
        }
    }
}

/// Compute MaxSim between a query token matrix and a document token matrix.
///
/// Both matrices **must be L2-normalized** before calling this function.
/// Use `normalize_rows()` or pre-normalize at indexing time.
///
/// `query`: flat f32 slice of `q_tokens × dim`
/// `doc`:   flat f32 slice of `d_tokens × dim`
///
/// Returns the sum of per-query-token max cosine similarities.
pub fn maxsim(
    query: &[f32],
    q_tokens: usize,
    doc: &[f32],
    d_tokens: usize,
    dim: usize,
) -> f32 {
    if dim == 0 || q_tokens == 0 || d_tokens == 0 {
        return f32::NEG_INFINITY;
    }

    query
        .chunks_exact(dim)
        .map(|qv| {
            doc.chunks_exact(dim)
                .map(|dv| qv.iter().zip(dv.iter()).map(|(a, b)| a * b).sum::<f32>())
                .fold(f32::NEG_INFINITY, f32::max)
        })
        .sum()
}

/// A document ready for MaxSim reranking.
pub struct RerankDoc {
    pub doc_id: String,
    /// Flat f32 token matrix: `n_tokens × dim`.
    pub token_matrix: Vec<f32>,
    pub n_tokens: usize,
    pub dim: usize,
}

/// Rerank a set of BM25 candidates using ColBERT MaxSim.
///
/// `query_matrix`: flat f32 slice `q_tokens × dim`.
/// Both the query and doc matrices are L2-normalized before scoring.
/// Returns candidates sorted by MaxSim score (descending).
pub fn rerank(
    query_matrix: &[f32],
    q_tokens: usize,
    dim: usize,
    candidates: Vec<RerankDoc>,
) -> Vec<(String, f32)> {
    if candidates.is_empty() || query_matrix.is_empty() {
        return vec![];
    }

    // Normalize query matrix.
    let mut q_norm = query_matrix.to_vec();
    normalize_rows(&mut q_norm, q_tokens, dim);

    let mut scored: Vec<(String, f32)> = if candidates.len() >= 16 {
        candidates
            .par_iter()
            .map(|doc| {
                let mut d_norm = doc.token_matrix.clone();
                normalize_rows(&mut d_norm, doc.n_tokens, doc.dim);
                let score = maxsim(&q_norm, q_tokens, &d_norm, doc.n_tokens, dim);
                (doc.doc_id.clone(), score)
            })
            .collect()
    } else {
        candidates
            .iter()
            .map(|doc| {
                let mut d_norm = doc.token_matrix.clone();
                normalize_rows(&mut d_norm, doc.n_tokens, doc.dim);
                let score = maxsim(&q_norm, q_tokens, &d_norm, doc.n_tokens, dim);
                (doc.doc_id.clone(), score)
            })
            .collect()
    };

    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored
}

/// Merge BM25 results with ColBERT rerank scores using Reciprocal Rank Fusion.
/// `bm25_results`: (doc_id, bm25_score)
/// `colbert_results`: (doc_id, maxsim_score)
/// `k`: smoothing constant — must be > 0 (conventionally 60.0).
/// Returns doc_ids sorted by fused score.
pub fn rrf_merge(
    bm25_results: &[SearchResult],
    colbert_results: &[(String, f32)],
    k: f32,
) -> Vec<String> {
    assert!(k > 0.0, "rrf_merge: k must be positive (got {k})");
    use std::collections::HashMap;

    let mut scores: HashMap<&str, f64> = HashMap::new();

    for (rank, r) in bm25_results.iter().enumerate() {
        *scores.entry(r.doc_id.as_str()).or_insert(0.0) += 1.0 / (k as f64 + rank as f64 + 1.0);
    }
    for (rank, (id, _)) in colbert_results.iter().enumerate() {
        *scores.entry(id.as_str()).or_insert(0.0) += 1.0 / (k as f64 + rank as f64 + 1.0);
    }

    let mut ranked: Vec<(&str, f64)> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    ranked.into_iter().map(|(id, _)| id.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_rows_unit_vector() {
        let mut m = vec![3.0f32, 4.0]; // norm = 5
        normalize_rows(&mut m, 1, 2);
        assert!((m[0] - 0.6).abs() < 1e-5);
        assert!((m[1] - 0.8).abs() < 1e-5);
    }

    #[test]
    fn normalize_rows_zero_vector_stays_zero() {
        let mut m = vec![0.0f32, 0.0];
        normalize_rows(&mut m, 1, 2);
        assert_eq!(m, vec![0.0, 0.0]);
    }

    #[test]
    fn maxsim_single_token() {
        // Pre-normalize: [1,0] already unit length.
        let q = vec![1.0f32, 0.0];
        let d = vec![1.0f32, 0.0, 0.0, 1.0]; // two doc tokens (each unit length)
        let s = maxsim(&q, 1, &d, 2, 2);
        assert!((s - 1.0).abs() < 1e-5, "expected 1.0, got {s}");
    }

    #[test]
    fn maxsim_two_query_tokens() {
        let dim = 2usize;
        let q = vec![1.0f32, 0.0, 0.0, 1.0]; // q0=[1,0] q1=[0,1] — both unit length
        let d = vec![1.0f32, 0.0, 0.0, 1.0]; // d0=[1,0] d1=[0,1] — both unit length
        let s = maxsim(&q, 2, &d, 2, dim);
        assert!((s - 2.0).abs() < 1e-5, "expected 2.0, got {s}");
    }

    #[test]
    fn rerank_normalizes_before_scoring() {
        // Query token [3,4] (norm=5), doc token [3,4] (norm=5) → cosine = 1.0
        let docs = vec![RerankDoc {
            doc_id: "a".into(),
            token_matrix: vec![3.0, 4.0],
            n_tokens: 1,
            dim: 2,
        }];
        let scored = rerank(&[3.0, 4.0], 1, 2, docs);
        assert!((scored[0].1 - 1.0).abs() < 1e-4, "expected cosine ~1.0, got {}", scored[0].1);
    }

    #[test]
    #[should_panic(expected = "k must be positive")]
    fn rrf_merge_panics_on_zero_k() {
        rrf_merge(&[], &[], 0.0);
    }
}
