/// Retrieval pipeline upgrade: typed query parsing, multi-stage retrieval, and
/// rerank hooks as the default MASTERd search path.
///
/// Design:
/// - `QueryPlan` is the canonical parsed form of any user/API query.
/// - `RetrievalStage` defines the execution contract for a single retrieval step.
/// - `RetrievalPipeline` chains stages deterministically and merges candidates.
/// - Reranking is a first-class hook, not an afterthought.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── Query representation ──────────────────────────────────────────────────────

/// Raw user query parsed into a structured plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPlan {
    pub raw: String,
    pub terms: Vec<String>,
    pub filters: HashMap<String, String>,
    /// Hint from the caller on what kind of search to prefer.
    pub intent: QueryIntent,
    pub top_k: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryIntent {
    /// Lexical / keyword search.
    Lexical,
    /// Dense vector similarity search.
    Semantic,
    /// Both lexical and semantic, results merged.
    Hybrid,
}

impl QueryPlan {
    /// Parse a raw query string into a `QueryPlan`.
    ///
    /// Recognizes:
    /// - `key:value` tokens as filters.
    /// - `top:N` as a top-k override.
    /// - `mode:(lexical|semantic|hybrid)` as intent override.
    /// - Remaining tokens as search terms.
    pub fn parse(raw: &str, default_top_k: usize) -> Self {
        let mut terms = Vec::new();
        let mut filters = HashMap::new();
        let mut intent = QueryIntent::Hybrid;
        let mut top_k = default_top_k;

        for token in raw.split_whitespace() {
            if let Some((key, value)) = token.split_once(':') {
                match key {
                    "top" => {
                        if let Ok(n) = value.parse::<usize>() {
                            top_k = n.max(1);
                        }
                    }
                    "mode" => {
                        intent = match value {
                            "lexical" => QueryIntent::Lexical,
                            "semantic" => QueryIntent::Semantic,
                            _ => QueryIntent::Hybrid,
                        };
                    }
                    _ => {
                        filters.insert(key.to_string(), value.to_string());
                    }
                }
            } else {
                terms.push(token.to_string());
            }
        }

        Self {
            raw: raw.to_string(),
            terms,
            filters,
            intent,
            top_k,
        }
    }

    pub fn query_text(&self) -> String {
        self.terms.join(" ")
    }
}

// ── Retrieval candidates ──────────────────────────────────────────────────────

/// A single candidate document returned by a retrieval stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalCandidate {
    pub doc_id: String,
    pub path: String,
    pub content_hash: String,
    /// Score in [0.0, 1.0].  Higher is more relevant.
    pub score: f32,
    /// Which retrieval stage produced this candidate.
    pub source_stage: String,
    pub tags: Vec<String>,
}

/// Merged and deduplicated result set after all retrieval stages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub query: QueryPlan,
    pub candidates: Vec<RetrievalCandidate>,
    pub reranked: bool,
    pub stage_counts: HashMap<String, usize>,
}

impl RetrievalResult {
    pub fn top_k(&self, k: usize) -> &[RetrievalCandidate] {
        let end = k.min(self.candidates.len());
        &self.candidates[..end]
    }
}

// ── Stage contract ────────────────────────────────────────────────────────────

/// A single retrieval stage.  Implementations include lexical, semantic, and graph.
pub trait RetrievalStage: Send + Sync {
    fn name(&self) -> &str;

    fn retrieve(&self, plan: &QueryPlan) -> Result<Vec<RetrievalCandidate>, RetrievalError>;
}

/// A reranker hook applied after candidates are merged.
pub trait RerankerHook: Send + Sync {
    fn rerank(
        &self,
        query: &str,
        candidates: &mut Vec<RetrievalCandidate>,
        top_k: usize,
    ) -> Result<(), RetrievalError>;
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Chains multiple retrieval stages, merges candidates (dedup by doc_id, keep
/// highest score), applies an optional reranker, and returns the top-K result.
pub struct RetrievalPipeline {
    stages: Vec<Box<dyn RetrievalStage>>,
    reranker: Option<Box<dyn RerankerHook>>,
}

impl RetrievalPipeline {
    pub fn builder() -> RetrievalPipelineBuilder {
        RetrievalPipelineBuilder::default()
    }

    pub fn execute(&self, plan: &QueryPlan) -> Result<RetrievalResult, RetrievalError> {
        let mut all_candidates: Vec<RetrievalCandidate> = Vec::new();
        let mut stage_counts: HashMap<String, usize> = HashMap::new();
        let mut errors: Vec<String> = Vec::new();

        for stage in &self.stages {
            match stage.retrieve(plan) {
                Ok(mut candidates) => {
                    stage_counts.insert(stage.name().to_string(), candidates.len());
                    all_candidates.append(&mut candidates);
                }
                Err(err) => {
                    errors.push(format!("stage '{}': {err}", stage.name()));
                }
            }
        }

        if all_candidates.is_empty() && !errors.is_empty() {
            return Err(RetrievalError::AllStagesFailed(errors.join("; ")));
        }

        // Dedup: keep highest score per doc_id.
        let deduped = dedup_by_score(all_candidates);

        // Sort descending by score.
        let mut ranked: Vec<RetrievalCandidate> = deduped;
        ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
        ranked.truncate(plan.top_k * 4); // pre-trim before reranker to bound cost

        let reranked = if let Some(hook) = &self.reranker {
            hook.rerank(&plan.query_text(), &mut ranked, plan.top_k)?;
            true
        } else {
            ranked.truncate(plan.top_k);
            false
        };

        Ok(RetrievalResult {
            query: plan.clone(),
            candidates: ranked,
            reranked,
            stage_counts,
        })
    }
}

fn dedup_by_score(candidates: Vec<RetrievalCandidate>) -> Vec<RetrievalCandidate> {
    use std::collections::hash_map::Entry;

    let mut best: HashMap<String, RetrievalCandidate> = HashMap::new();
    for candidate in candidates {
        match best.entry(candidate.doc_id.clone()) {
            Entry::Occupied(mut entry) => {
                if candidate.score > entry.get().score {
                    entry.insert(candidate);
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(candidate);
            }
        }
    }
    best.into_values().collect()
}

// ── Builder ───────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct RetrievalPipelineBuilder {
    stages: Vec<Box<dyn RetrievalStage>>,
    reranker: Option<Box<dyn RerankerHook>>,
}

impl RetrievalPipelineBuilder {
    pub fn add_stage(mut self, stage: impl RetrievalStage + 'static) -> Self {
        self.stages.push(Box::new(stage));
        self
    }

    pub fn with_reranker(mut self, reranker: impl RerankerHook + 'static) -> Self {
        self.reranker = Some(Box::new(reranker));
        self
    }

    pub fn build(self) -> Result<RetrievalPipeline, RetrievalError> {
        if self.stages.is_empty() {
            return Err(RetrievalError::ConfigError(
                "retrieval pipeline requires at least one stage".to_string(),
            ));
        }
        Ok(RetrievalPipeline {
            stages: self.stages,
            reranker: self.reranker,
        })
    }
}

// ── Built-in no-op stage (for testing / CPU-only fallback) ────────────────────

/// A no-op retrieval stage that always returns an empty result set.
/// Useful as a placeholder for stages that have not yet been wired.
pub struct NoopRetrievalStage {
    stage_name: String,
}

impl NoopRetrievalStage {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            stage_name: name.into(),
        }
    }
}

impl RetrievalStage for NoopRetrievalStage {
    fn name(&self) -> &str {
        &self.stage_name
    }

    fn retrieve(&self, _plan: &QueryPlan) -> Result<Vec<RetrievalCandidate>, RetrievalError> {
        Ok(Vec::new())
    }
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum RetrievalError {
    StageError(String, String),
    AllStagesFailed(String),
    RerankerError(String),
    ConfigError(String),
}

impl std::fmt::Display for RetrievalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RetrievalError::StageError(stage, msg) => {
                write!(f, "retrieval stage '{stage}' failed: {msg}")
            }
            RetrievalError::AllStagesFailed(msg) => {
                write!(f, "all retrieval stages failed: {msg}")
            }
            RetrievalError::RerankerError(msg) => write!(f, "reranker failed: {msg}"),
            RetrievalError::ConfigError(msg) => write!(f, "retrieval config error: {msg}"),
        }
    }
}

impl std::error::Error for RetrievalError {}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedStage {
        name: String,
        results: Vec<RetrievalCandidate>,
    }

    impl RetrievalStage for FixedStage {
        fn name(&self) -> &str {
            &self.name
        }

        fn retrieve(&self, _plan: &QueryPlan) -> Result<Vec<RetrievalCandidate>, RetrievalError> {
            Ok(self.results.clone())
        }
    }

    fn candidate(doc_id: &str, score: f32, stage: &str) -> RetrievalCandidate {
        RetrievalCandidate {
            doc_id: doc_id.to_string(),
            path: format!("/docs/{doc_id}.pdf"),
            content_hash: "aabbccdd".to_string(),
            score,
            source_stage: stage.to_string(),
            tags: vec![],
        }
    }

    #[test]
    fn query_plan_parses_terms_and_filters() {
        let plan = QueryPlan::parse("hello world tag:important top:5 mode:semantic", 10);
        assert_eq!(plan.terms, vec!["hello", "world"]);
        assert_eq!(
            plan.filters.get("tag").map(|s| s.as_str()),
            Some("important")
        );
        assert_eq!(plan.top_k, 5);
        assert_eq!(plan.intent, QueryIntent::Semantic);
    }

    #[test]
    fn pipeline_deduplicates_and_keeps_highest_score() {
        let stage_a = FixedStage {
            name: "lexical".to_string(),
            results: vec![
                candidate("doc1", 0.8, "lexical"),
                candidate("doc2", 0.6, "lexical"),
            ],
        };
        let stage_b = FixedStage {
            name: "semantic".to_string(),
            results: vec![
                candidate("doc1", 0.9, "semantic"), // higher score for doc1
                candidate("doc3", 0.7, "semantic"),
            ],
        };
        let pipeline = RetrievalPipeline::builder()
            .add_stage(stage_a)
            .add_stage(stage_b)
            .build()
            .unwrap();

        let plan = QueryPlan::parse("test query", 10);
        let result = pipeline.execute(&plan).unwrap();

        // doc1 should appear once with the higher semantic score.
        let doc1 = result.candidates.iter().find(|c| c.doc_id == "doc1");
        assert!(doc1.is_some());
        assert!((doc1.unwrap().score - 0.9).abs() < 0.001);
        assert_eq!(result.candidates.len(), 3);
    }

    #[test]
    fn pipeline_respects_top_k() {
        let stage = FixedStage {
            name: "test".to_string(),
            results: (0..20)
                .map(|i| candidate(&format!("doc{i}"), i as f32 / 20.0, "test"))
                .collect(),
        };
        let pipeline = RetrievalPipeline::builder()
            .add_stage(stage)
            .build()
            .unwrap();
        let plan = QueryPlan::parse("test", 5);
        let result = pipeline.execute(&plan).unwrap();
        assert!(result.candidates.len() <= 5);
    }

    #[test]
    fn noop_stage_returns_empty() {
        let pipeline = RetrievalPipeline::builder()
            .add_stage(NoopRetrievalStage::new("empty"))
            .build()
            .unwrap();
        let plan = QueryPlan::parse("anything", 10);
        let result = pipeline.execute(&plan).unwrap();
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn builder_rejects_empty_stages() {
        let err = RetrievalPipeline::builder().build();
        assert!(err.is_err());
    }
}
