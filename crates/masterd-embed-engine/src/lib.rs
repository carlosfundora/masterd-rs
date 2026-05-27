use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// Ported from local WIP embedding-service patterns.
pub const COLBERT_WRAPPER_DEFAULT_URL: &str = "http://127.0.0.1:11450";
pub const JINA_DEFAULT_URL: &str = "http://127.0.0.1:11447";
pub const QWEN3_DEFAULT_URL: &str = "http://127.0.0.1:11502";

pub const COLBERT_WRAPPER_MODEL: &str = "colbert-lfm2-305m";
pub const JINA_MODEL: &str = "jina-code-embed";
pub const QWEN3_MODEL: &str = "qwen3-embedding";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeTuning {
    pub max_inflight: usize,
    pub timeout_secs: u64,
    pub batch_size: usize,
    pub direct_vector_dim: usize,
}

impl RuntimeTuning {
    pub fn from_env() -> Self {
        Self {
            max_inflight: std::env::var("MEMORYBANK_EMBED_CONCURRENCY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(4),
            timeout_secs: std::env::var("MASTERD_EMBED_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(60),
            batch_size: std::env::var("MASTERD_EMBED_BATCH_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(16),
            direct_vector_dim: std::env::var("MASTERD_DIRECT_VECTOR_DIM")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(384),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InferenceBackend {
    Direct,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalEmbeddingStack {
    pub backend: InferenceBackend,
    pub colbert_url: String,
    pub jina_url: String,
    pub qwen3_url: String,
    pub colbert_model: String,
    pub jina_model: String,
    pub qwen3_model: String,
    pub tuning: RuntimeTuning,
}

impl LocalEmbeddingStack {
    pub fn from_env() -> Self {
        let backend = match std::env::var("MASTERD_INFERENCE_BACKEND")
            .unwrap_or_else(|_| "direct".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "http" => InferenceBackend::Http,
            _ => InferenceBackend::Direct,
        };
        Self {
            backend,
            colbert_url: std::env::var("MEMORYBANK_COLBERT_WRAPPER_URL")
                .unwrap_or_else(|_| COLBERT_WRAPPER_DEFAULT_URL.to_string()),
            jina_url: std::env::var("MEMORYBANK_JINA_URL")
                .unwrap_or_else(|_| JINA_DEFAULT_URL.to_string()),
            qwen3_url: std::env::var("MEMORYBANK_QWEN3_URL")
                .unwrap_or_else(|_| QWEN3_DEFAULT_URL.to_string()),
            colbert_model: std::env::var("MEMORYBANK_COLBERT_WRAPPER_MODEL")
                .unwrap_or_else(|_| COLBERT_WRAPPER_MODEL.to_string()),
            jina_model: std::env::var("MEMORYBANK_JINA_MODEL")
                .unwrap_or_else(|_| JINA_MODEL.to_string()),
            qwen3_model: std::env::var("MEMORYBANK_QWEN3_MODEL")
                .unwrap_or_else(|_| QWEN3_MODEL.to_string()),
            tuning: RuntimeTuning::from_env(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmbeddedEngine {
    pub cfg: LocalEmbeddingStack,
    client: Client,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbedDataItem {
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbedResponse {
    pub data: Vec<EmbedDataItem>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RerankResult {
    pub index: usize,
    pub relevance_score: Option<f32>,
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RerankResponse {
    pub results: Vec<RerankResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineBench {
    pub operation: String,
    pub elapsed_ms: f64,
    pub estimated_tokens_per_sec: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionProvider {
    Jina,
    Qwen3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionStagePolicy {
    pub provider: ExtractionProvider,
    pub max_retries: usize,
    pub min_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionFallbackPolicy {
    pub stages: Vec<ExtractionStagePolicy>,
    pub global_min_score: f32,
}

impl Default for ExtractionFallbackPolicy {
    fn default() -> Self {
        Self {
            stages: vec![
                ExtractionStagePolicy {
                    provider: ExtractionProvider::Jina,
                    max_retries: 2,
                    min_score: 0.65,
                },
                ExtractionStagePolicy {
                    provider: ExtractionProvider::Qwen3,
                    max_retries: 2,
                    min_score: 0.60,
                },
            ],
            global_min_score: 0.60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionDecision {
    Skipped,
    Failed,
    Selected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionReason {
    pub decision: ExtractionDecision,
    pub provider: ExtractionProvider,
    pub stage_index: usize,
    pub attempt: usize,
    pub code: String,
    pub message: String,
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionSelection {
    pub provider: ExtractionProvider,
    pub score: f32,
    pub embeddings: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionExecutionReport {
    pub selected: Option<ExtractionSelection>,
    pub reasons: Vec<ExtractionReason>,
}

impl EmbeddedEngine {
    pub fn new(cfg: LocalEmbeddingStack) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(cfg.tuning.timeout_secs))
            .build()?;
        Ok(Self { cfg, client })
    }

    pub fn health_check(&self) -> Result<()> {
        if self.cfg.backend == InferenceBackend::Direct {
            return Ok(());
        }
        for url in [
            &self.cfg.colbert_url,
            &self.cfg.jina_url,
            &self.cfg.qwen3_url,
        ] {
            let resp = self
                .client
                .get(format!("{}/health", url.trim_end_matches('/')))
                .send()
                .with_context(|| format!("health check failed for {url}"))?;
            if !resp.status().is_success() {
                anyhow::bail!("health failed for {url}: HTTP {}", resp.status());
            }
        }
        Ok(())
    }

    pub fn embed_jina(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.embed_jina_fast(texts)
    }

    pub fn embed_qwen3(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.embed_generic(&self.cfg.qwen3_url, &self.cfg.qwen3_model, texts)
    }

    pub fn embed_jina_fast(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let report = self.embed_jina_fast_with_report(texts)?;
        report
            .selected
            .map(|selected| selected.embeddings)
            .context("extraction fallback found no valid embedding candidate")
    }

    pub fn embed_jina_fast_with_report(
        &self,
        texts: &[String],
    ) -> Result<ExtractionExecutionReport> {
        if texts.is_empty() {
            return Ok(ExtractionExecutionReport {
                selected: Some(ExtractionSelection {
                    provider: ExtractionProvider::Jina,
                    score: 1.0,
                    embeddings: Vec::new(),
                }),
                reasons: Vec::new(),
            });
        }
        self.execute_extraction_policy(texts, &ExtractionFallbackPolicy::default())
    }

    pub fn embed_qwen3_fast(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.embed_generic_batched(&self.cfg.qwen3_url, &self.cfg.qwen3_model, texts)
    }

    fn embed_generic(
        &self,
        base_url: &str,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        if self.cfg.backend == InferenceBackend::Direct {
            return Ok(Self::embed_direct_texts(
                texts,
                self.cfg.tuning.direct_vector_dim,
            ));
        }
        Self::embed_generic_with_client(&self.client, base_url, model, texts)
    }

    fn embed_generic_with_client(
        client: &Client,
        base_url: &str,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let payload = serde_json::json!({
            "input": texts,
            "model": model,
        });
        let resp = client
            .post(format!("{}/v1/embeddings", base_url.trim_end_matches('/')))
            .json(&payload)
            .send()?;
        if !resp.status().is_success() {
            anyhow::bail!("embed call failed: HTTP {}", resp.status());
        }
        let body: EmbedResponse = resp.json()?;
        Ok(body.data.into_iter().map(|d| d.embedding).collect())
    }

    fn embed_generic_batched(
        &self,
        base_url: &str,
        model: &str,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if self.cfg.backend == InferenceBackend::Direct {
            return self.embed_generic(base_url, model, texts);
        }
        let batch_size = self.cfg.tuning.batch_size.max(1);
        if texts.len() <= batch_size {
            return self.embed_generic(base_url, model, texts);
        }

        let max_inflight = self.cfg.tuning.max_inflight.max(1);
        let batches: Vec<(usize, Vec<String>)> = texts
            .chunks(batch_size)
            .enumerate()
            .map(|(idx, chunk)| (idx, chunk.to_vec()))
            .collect();
        let mut batch_outputs: Vec<Option<Vec<Vec<f32>>>> = vec![None; batches.len()];

        for inflight in batches.chunks(max_inflight) {
            let mut chunk_results = Vec::with_capacity(inflight.len());
            std::thread::scope(|scope| {
                let mut handles = Vec::with_capacity(inflight.len());
                for (batch_idx, batch) in inflight {
                    let client = self.client.clone();
                    let base = base_url.to_string();
                    let model = model.to_string();
                    let payload = batch.clone();
                    let batch_idx = *batch_idx;
                    handles.push(scope.spawn(move || -> Result<(usize, Vec<Vec<f32>>)> {
                        let out =
                            Self::embed_generic_with_client(&client, &base, &model, &payload)?;
                        Ok((batch_idx, out))
                    }));
                }
                for handle in handles {
                    let joined = handle
                        .join()
                        .map_err(|_| anyhow::anyhow!("embed batch worker panicked"));
                    chunk_results.push(joined);
                }
            });

            for joined in chunk_results {
                let (batch_idx, vectors) = joined??;
                batch_outputs[batch_idx] = Some(vectors);
            }
        }

        let mut flattened = Vec::new();
        for output in batch_outputs {
            let vectors = output.context("missing batch output from embed worker")?;
            flattened.extend(vectors);
        }
        Ok(flattened)
    }

    pub fn execute_extraction_policy(
        &self,
        texts: &[String],
        policy: &ExtractionFallbackPolicy,
    ) -> Result<ExtractionExecutionReport> {
        Self::execute_extraction_policy_with_fetch(policy, |provider, _attempt| {
            self.embed_for_provider(provider, texts)
        })
    }

    fn execute_extraction_policy_with_fetch<F>(
        policy: &ExtractionFallbackPolicy,
        mut fetch: F,
    ) -> Result<ExtractionExecutionReport>
    where
        F: FnMut(ExtractionProvider, usize) -> Result<Vec<Vec<f32>>>,
    {
        if policy.stages.is_empty() {
            anyhow::bail!("extraction fallback policy must contain at least one stage");
        }

        #[derive(Debug)]
        struct Candidate {
            provider: ExtractionProvider,
            stage_index: usize,
            attempt: usize,
            score: f32,
            embeddings: Vec<Vec<f32>>,
        }

        let mut reasons = Vec::new();
        let mut candidates = Vec::new();

        for (stage_index, stage) in policy.stages.iter().enumerate() {
            if stage.max_retries == 0 {
                reasons.push(ExtractionReason {
                    decision: ExtractionDecision::Skipped,
                    provider: stage.provider,
                    stage_index,
                    attempt: 0,
                    code: "retry_bound_zero".to_string(),
                    message: "stage skipped because max_retries is zero".to_string(),
                    score: None,
                });
                continue;
            }

            for attempt in 1..=stage.max_retries {
                match fetch(stage.provider, attempt) {
                    Ok(embeddings) => match Self::score_embedding_quality(&embeddings) {
                        Ok(score) => {
                            let threshold = stage.min_score.max(policy.global_min_score);
                            if score < threshold {
                                reasons.push(ExtractionReason {
                                    decision: ExtractionDecision::Skipped,
                                    provider: stage.provider,
                                    stage_index,
                                    attempt,
                                    code: "score_below_threshold".to_string(),
                                    message: format!(
                                        "candidate score {:.4} below threshold {:.4}",
                                        score, threshold
                                    ),
                                    score: Some(score),
                                });
                                continue;
                            }
                            candidates.push(Candidate {
                                provider: stage.provider,
                                stage_index,
                                attempt,
                                score,
                                embeddings,
                            });
                        }
                        Err((code, message)) => reasons.push(ExtractionReason {
                            decision: ExtractionDecision::Skipped,
                            provider: stage.provider,
                            stage_index,
                            attempt,
                            code: code.to_string(),
                            message,
                            score: None,
                        }),
                    },
                    Err(err) => reasons.push(ExtractionReason {
                        decision: ExtractionDecision::Failed,
                        provider: stage.provider,
                        stage_index,
                        attempt,
                        code: "provider_error".to_string(),
                        message: err.to_string(),
                        score: None,
                    }),
                }
            }
        }

        if candidates.is_empty() {
            return Ok(ExtractionExecutionReport {
                selected: None,
                reasons,
            });
        }

        candidates.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.stage_index.cmp(&b.stage_index))
                .then_with(|| a.attempt.cmp(&b.attempt))
                .then_with(|| a.provider.cmp(&b.provider))
        });

        let winner = candidates.remove(0);
        reasons.push(ExtractionReason {
            decision: ExtractionDecision::Selected,
            provider: winner.provider,
            stage_index: winner.stage_index,
            attempt: winner.attempt,
            code: "selected_best_valid_candidate".to_string(),
            message: "candidate selected after deterministic fallback scoring".to_string(),
            score: Some(winner.score),
        });

        Ok(ExtractionExecutionReport {
            selected: Some(ExtractionSelection {
                provider: winner.provider,
                score: winner.score,
                embeddings: winner.embeddings,
            }),
            reasons,
        })
    }

    fn embed_for_provider(&self, provider: ExtractionProvider, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        match provider {
            ExtractionProvider::Jina => {
                self.embed_generic_batched(&self.cfg.jina_url, &self.cfg.jina_model, texts)
            }
            ExtractionProvider::Qwen3 => {
                self.embed_generic_batched(&self.cfg.qwen3_url, &self.cfg.qwen3_model, texts)
            }
        }
    }

    fn score_embedding_quality(embeddings: &[Vec<f32>]) -> std::result::Result<f32, (&'static str, String)> {
        if embeddings.is_empty() {
            return Err(("empty_embeddings", "candidate produced no embeddings".to_string()));
        }
        let dim = embeddings[0].len();
        if dim == 0 {
            return Err(("empty_vector", "candidate produced empty vectors".to_string()));
        }
        if embeddings.iter().any(|row| row.len() != dim) {
            return Err((
                "dimension_mismatch",
                "candidate embeddings have inconsistent dimensions".to_string(),
            ));
        }
        if embeddings
            .iter()
            .flat_map(|row| row.iter())
            .any(|value| !value.is_finite())
        {
            return Err((
                "non_finite_value",
                "candidate embeddings contain non-finite values".to_string(),
            ));
        }

        let avg_norm = embeddings
            .iter()
            .map(|row| row.iter().map(|v| v * v).sum::<f32>().sqrt())
            .sum::<f32>()
            / embeddings.len() as f32;
        if avg_norm == 0.0 {
            return Err(("zero_norm", "candidate embeddings have zero norm".to_string()));
        }
        let norm_component = (1.0 / (1.0 + (avg_norm - 1.0).abs())).clamp(0.0, 1.0);
        let dim_component = (dim as f32 / 1536.0).clamp(0.0, 1.0);
        Ok((norm_component * 0.8 + dim_component * 0.2).clamp(0.0, 1.0))
    }

    pub fn rerank_colbert(&self, query: &str, documents: &[String]) -> Result<RerankResponse> {
        if self.cfg.backend == InferenceBackend::Direct {
            let query_vec =
                Self::embed_direct_texts(&[query.to_string()], self.cfg.tuning.direct_vector_dim)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
            let doc_vecs = Self::embed_direct_texts(documents, self.cfg.tuning.direct_vector_dim);
            let mut results = Vec::with_capacity(doc_vecs.len());
            for (idx, doc_vec) in doc_vecs.iter().enumerate() {
                let score = Self::cosine_similarity(&query_vec, doc_vec);
                results.push(RerankResult {
                    index: idx,
                    relevance_score: Some(score),
                    score: Some(score),
                });
            }
            return Ok(RerankResponse { results });
        }
        let payload = serde_json::json!({
            "model": self.cfg.colbert_model,
            "query": query,
            "documents": documents,
        });
        let base = self.cfg.colbert_url.trim_end_matches('/');
        let mut resp = self
            .client
            .post(format!("{base}/v1/rerank"))
            .json(&payload)
            .send()?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            resp = self
                .client
                .post(format!("{base}/rerank"))
                .json(&payload)
                .send()?;
        }
        if !resp.status().is_success() {
            anyhow::bail!("rerank call failed: HTTP {}", resp.status());
        }
        Ok(resp.json()?)
    }

    pub fn rerank_colbert_topk(
        &self,
        query: &str,
        documents: &[String],
        top_k: usize,
    ) -> Result<Vec<RerankResult>> {
        let mut results = self.rerank_colbert(query, documents)?.results;
        results.sort_by(|a, b| Self::score_of(b).total_cmp(&Self::score_of(a)));
        if top_k == 0 || top_k >= results.len() {
            return Ok(results);
        }
        Ok(results.into_iter().take(top_k).collect())
    }

    pub fn mean_pool_embedding(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
        let first = vectors.first()?;
        if first.is_empty() {
            return None;
        }
        let dim = first.len();
        let mut accum = vec![0.0f32; dim];
        let mut count = 0usize;
        for vec in vectors {
            if vec.len() != dim {
                continue;
            }
            for (idx, value) in vec.iter().enumerate() {
                accum[idx] += *value;
            }
            count += 1;
        }
        if count == 0 {
            return None;
        }
        for value in &mut accum {
            *value /= count as f32;
        }
        Some(accum)
    }

    pub fn token_matrix_hash(vectors: &[Vec<f32>]) -> String {
        let mut hasher = Sha256::new();
        hasher.update((vectors.len() as u64).to_le_bytes());
        for vec in vectors {
            hasher.update((vec.len() as u64).to_le_bytes());
            for value in vec {
                hasher.update(value.to_le_bytes());
            }
        }
        let digest = hasher.finalize();
        format!("{digest:x}")
    }

    pub fn bench_embed_jina(&self, text: &str, token_count_estimate: usize) -> Result<EngineBench> {
        let input = vec![text.to_string()];
        let start = Instant::now();
        let _ = self.embed_jina_fast(&input)?;
        let elapsed = start.elapsed();
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        let tps = if elapsed.as_secs_f64() > 0.0 {
            token_count_estimate as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };
        Ok(EngineBench {
            operation: "jina_embed".to_string(),
            elapsed_ms,
            estimated_tokens_per_sec: tps,
        })
    }

    fn score_of(item: &RerankResult) -> f32 {
        item.score.or(item.relevance_score).unwrap_or(0.0)
    }

    fn embed_direct_texts(texts: &[String], dim: usize) -> Vec<Vec<f32>> {
        texts
            .iter()
            .map(|text| Self::hashed_projection(text.as_bytes(), dim.max(8)))
            .collect()
    }

    fn hashed_projection(seed_bytes: &[u8], dim: usize) -> Vec<f32> {
        let mut hasher = Sha256::new();
        hasher.update(seed_bytes);
        let digest = hasher.finalize();
        let mut state = u64::from_le_bytes([
            digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
        ]);
        let mut vec = Vec::with_capacity(dim);
        for _ in 0..dim {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let sample = (state as f64 / u64::MAX as f64) as f32;
            vec.push((sample * 2.0) - 1.0);
        }
        let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            for value in &mut vec {
                *value /= norm;
            }
        }
        vec
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.is_empty() || b.is_empty() || a.len() != b.len() {
            return 0.0;
        }
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }
}
