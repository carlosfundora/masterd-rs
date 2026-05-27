use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use reqwest::blocking::Client;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tree_sitter::Parser;

pub const DEFAULT_CHUNK_TARGET_TOKENS: usize = 800;
pub const DEFAULT_CHUNK_OVERLAP_TOKENS: usize = 120;
const DEFAULT_EMBED_DIM: usize = 768;
const MATRYOSHKA_DIMS: &[usize] = &[64, 128, 256, 384, 768, 1024];
const RRF_K: f64 = 60.0;
const GRAPH_EXPANSION_LIMIT: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRecord {
    pub id: String,
    pub original_name: String,
    pub current_name: String,
    pub suggested_name: Option<String>,
    pub original_path: String,
    pub current_path: String,
    pub extension: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub hash: String,
    pub classification: Option<ClassificationResult>,
    pub tags: Vec<String>,
    pub extracted_text: Option<String>,
    pub summary: Option<String>,
    pub confidence: f32,
    pub duplicate_status: String,
    pub processing_status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationResult {
    pub category: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentChunk {
    pub id: String,
    pub document_id: String,
    pub chunk_index: usize,
    pub text: String,
    pub token_start: usize,
    pub token_end: usize,
    pub text_hash: String,
    pub source_stage: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MatryoshkaEmbeddingProfile {
    pub provider: String,
    pub full_dim: usize,
    pub prefix_dims: Vec<usize>,
    pub lexical_context: String,
    pub metadata_context: serde_json::Value,
    pub colbert_token_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalTrace {
    pub id: String,
    pub query: String,
    pub mode: SearchMode,
    pub top_k: usize,
    pub lexical_count: usize,
    pub semantic_count: usize,
    pub graph_count: usize,
    pub reranked: bool,
    pub stages: Vec<RetrievalStageTrace>,
    pub results: Vec<RetrievalCandidate>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalStageTrace {
    pub stage: String,
    pub count: usize,
    pub elapsed_ms: u64,
    pub degraded: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalCandidate {
    pub document_id: String,
    pub chunk_id: String,
    pub title: String,
    pub path: String,
    pub text: String,
    pub score: f32,
    pub source_stage: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SearchMode {
    Lexical,
    Semantic,
    #[default]
    Hybrid,
}


impl SearchMode {
    pub fn from_str_lossy(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "lexical" | "localdocuments" => Self::Lexical,
            "semantic" => Self::Semantic,
            _ => Self::Hybrid,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceEvent {
    pub id: String,
    pub category: String,
    pub signal: String,
    pub value: String,
    pub source: String,
    pub confidence: f32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LearnedPreference {
    pub id: String,
    pub category: String,
    pub key: String,
    pub value: String,
    pub confidence: f32,
    pub status: String,
    pub evidence_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRun {
    pub id: String,
    pub document_id: Option<String>,
    pub file_path: String,
    pub status: String,
    pub stage_timings: Vec<StageTiming>,
    pub failure: Option<PipelineFailure>,
    pub retryable: bool,
    pub indexed_chunk_count: usize,
    pub ocr_confidence: Option<f32>,
    pub embedding_provider: Option<String>,
    pub rerank_status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StageTiming {
    pub stage: String,
    pub elapsed_ms: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PipelineFailure {
    pub stage: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewItem {
    pub id: String,
    pub document_id: String,
    pub reason: String,
    pub severity: String,
    pub title: String,
    pub explanation: String,
    pub proposed_action: Option<serde_json::Value>,
    pub created_at: String,
    pub resolved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: String,
    pub document_id: Option<String>,
    pub action: String,
    pub actor: String,
    pub summary: String,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub reversible: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct DataStore {
    db_path: PathBuf,
    conn: Arc<Mutex<Connection>>,
    meili: Option<MeilisearchClient>,
    valkey: Option<redis::Client>,
    embedding: Option<EmbeddingServiceClient>,
    model2vec: Option<Model2VecClient>,
    reranker: Option<ColbertRerankerClient>,
    lancedb: Option<LanceDbClient>,
    falkordb: Option<FalkorDbClient>,
}

#[derive(Debug, Clone)]
pub struct DataStoreConfig {
    pub db_path: PathBuf,
    pub meilisearch_url: Option<String>,
    pub valkey_url: Option<String>,
    pub embedding_url: Option<String>,
    pub embedding_model: Option<String>,
    pub model2vec_url: Option<String>,
    pub model2vec_model: Option<String>,
    pub colbert_url: Option<String>,
    pub lancedb_url: Option<String>,
    pub falkordb_url: Option<String>,
}

impl DataStoreConfig {
    pub fn local(db_path: PathBuf) -> Self {
        Self {
            db_path,
            meilisearch_url: Some("http://127.0.0.1:7700".to_string()),
            valkey_url: Some("redis://127.0.0.1:6399/".to_string()),
            embedding_url: Some("http://127.0.0.1:11447".to_string()),
            embedding_model: Some("jina-omni".to_string()),
            model2vec_url: Some("http://127.0.0.1:11448".to_string()),
            model2vec_model: Some("minishlab/potion-base-8M".to_string()),
            colbert_url: Some("http://127.0.0.1:11450".to_string()),
            lancedb_url: None,
            falkordb_url: Some("redis://127.0.0.1:6380/".to_string()),
        }
    }
}

impl DataStore {
    pub fn open(config: DataStoreConfig) -> Result<Self> {
        if let Some(parent) = config.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&config.db_path)?;
        let store = Self {
            db_path: config.db_path,
            conn: Arc::new(Mutex::new(conn)),
            meili: config
                .meilisearch_url
                .filter(|url| !url.trim().is_empty())
                .map(MeilisearchClient::new),
            valkey: config
                .valkey_url
                .and_then(|url| redis::Client::open(url).ok()),
            embedding: config
                .embedding_url
                .filter(|url| !url.trim().is_empty())
                .map(|url| EmbeddingServiceClient::new(url, config.embedding_model.unwrap_or_else(|| "jina-omni".to_string()))),
            model2vec: config
                .model2vec_url
                .filter(|url| !url.trim().is_empty())
                .map(|url| Model2VecClient::new(
                    url,
                    config
                        .model2vec_model
                        .filter(|model| !model.trim().is_empty())
                        .unwrap_or_else(|| "minishlab/potion-base-8M".to_string()),
                )),
            reranker: config
                .colbert_url
                .filter(|url| !url.trim().is_empty())
                .map(ColbertRerankerClient::new),
            lancedb: config
                .lancedb_url
                .filter(|url| !url.trim().is_empty())
                .map(LanceDbClient::new),
            falkordb: config
                .falkordb_url
                .filter(|url| !url.trim().is_empty())
                .and_then(|url| FalkorDbClient::new(url).ok()),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS documents (
                id TEXT PRIMARY KEY,
                original_name TEXT NOT NULL,
                current_name TEXT NOT NULL,
                suggested_name TEXT,
                original_path TEXT NOT NULL,
                current_path TEXT NOT NULL,
                extension TEXT NOT NULL,
                mime_type TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                hash TEXT NOT NULL UNIQUE,
                classification_json TEXT,
                tags_json TEXT NOT NULL,
                extracted_text TEXT,
                summary TEXT,
                confidence REAL NOT NULL,
                duplicate_status TEXT NOT NULL,
                processing_status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                text TEXT NOT NULL,
                token_start INTEGER NOT NULL,
                token_end INTEGER NOT NULL,
                text_hash TEXT NOT NULL,
                source_stage TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(document_id) REFERENCES documents(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS embeddings (
                chunk_id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                dim INTEGER NOT NULL,
                vector_json TEXT NOT NULL,
                vector_hash TEXT NOT NULL,
                matryoshka_json TEXT NOT NULL,
                lexical_context TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                colbert_token_hash TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS embeddings_model2vec (
                chunk_id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                dim INTEGER NOT NULL,
                vector_json TEXT NOT NULL,
                vector_hash TEXT NOT NULL,
                matryoshka_json TEXT NOT NULL,
                lexical_context TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                colbert_token_hash TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY(chunk_id) REFERENCES chunks(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS graph_edges (
                id TEXT PRIMARY KEY,
                from_id TEXT NOT NULL,
                to_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                weight REAL NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audit_entries (
                id TEXT PRIMARY KEY,
                document_id TEXT,
                action TEXT NOT NULL,
                actor TEXT NOT NULL,
                summary TEXT NOT NULL,
                before_json TEXT,
                after_json TEXT,
                reversible INTEGER NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS preference_events (
                id TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                signal TEXT NOT NULL,
                value TEXT NOT NULL,
                source TEXT NOT NULL,
                confidence REAL NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS learned_preferences (
                id TEXT PRIMARY KEY,
                category TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                confidence REAL NOT NULL,
                status TEXT NOT NULL,
                evidence_count INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(category, key, value)
            );

            CREATE TABLE IF NOT EXISTS pipeline_runs (
                id TEXT PRIMARY KEY,
                document_id TEXT,
                file_path TEXT NOT NULL,
                status TEXT NOT NULL,
                stage_timings_json TEXT NOT NULL,
                failure_json TEXT,
                retryable INTEGER NOT NULL,
                indexed_chunk_count INTEGER NOT NULL,
                ocr_confidence REAL,
                embedding_provider TEXT,
                rerank_status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS review_items (
                id TEXT PRIMARY KEY,
                document_id TEXT NOT NULL,
                reason TEXT NOT NULL,
                severity TEXT NOT NULL,
                title TEXT NOT NULL,
                explanation TEXT NOT NULL,
                proposed_action_json TEXT,
                created_at TEXT NOT NULL,
                resolved INTEGER
            );

            CREATE TABLE IF NOT EXISTS retrieval_traces (
                id TEXT PRIMARY KEY,
                query TEXT NOT NULL,
                mode TEXT NOT NULL,
                trace_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                chunk_id UNINDEXED,
                document_id UNINDEXED,
                title,
                path,
                text
            );

            CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(current_path);
            CREATE INDEX IF NOT EXISTS idx_chunks_document ON chunks(document_id);
            CREATE INDEX IF NOT EXISTS idx_preference_events_signal ON preference_events(category, signal, value);
            ",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (1, ?1)",
            params![now()],
        )?;
        Ok(())
    }

    pub fn migration_versions(&self) -> Result<Vec<i64>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn ingest_file(&self, path: &Path, config: &IngestConfig) -> Result<IngestOutcome> {
        let started = std::time::Instant::now();
        let run_id = stable_id("run", path.to_string_lossy().as_bytes());
        let mut timings = Vec::new();
        let mut stage_start = std::time::Instant::now();
        let path_string = path.to_string_lossy().to_string();

        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                let run = PipelineRun::failed(
                    run_id,
                    None,
                    path_string,
                    "read",
                    err.to_string(),
                    false,
                    timings,
                );
                self.upsert_pipeline_run(&run)?;
                return Ok(IngestOutcome { document: None, chunks: vec![], run });
            }
        };
        let hash = sha256_hex(&bytes);
        timings.push(stage_timing("hash", stage_start.elapsed(), "complete"));

        if let Some(existing) = self.find_document_by_hash(&hash)? {
            let mut run = PipelineRun::complete(run_id, Some(existing.id.clone()), path_string, timings);
            run.status = "duplicate".to_string();
            self.upsert_pipeline_run(&run)?;
            self.write_audit(existing.id.as_str(), "duplicate_detected", "system", "Exact hash duplicate detected", false)?;
            return Ok(IngestOutcome { document: Some(existing), chunks: vec![], run });
        }

        stage_start = std::time::Instant::now();
        let extraction = extract_text(path, &bytes, &config.ocr_language);
        timings.push(stage_timing("extract_text", stage_start.elapsed(), if extraction.text.is_some() { "complete" } else { "warning" }));

        let timestamp = now();
        let id = stable_id("doc", hash.as_bytes());
        let extension = extension(path);
        let original_name = path.file_name().and_then(|v| v.to_str()).unwrap_or("unknown").to_string();
        let routing = routing_decision_for(path, &hash);
        let mut tags = infer_tags(&extension, extraction.text.as_deref().unwrap_or_default());
        if let Some(decision) = &routing {
            tags.extend(decision.tags.iter().cloned());
            tags.push(format!("route:{}", decision.route));
            tags.sort();
            tags.dedup();
        }
        let confidence = if extraction.text.is_some() { 0.82 } else { 0.20 };
        let processing_status = if extraction.text.is_some() { "complete" } else { "warning" }.to_string();
        let doc = DocumentRecord {
            id: id.clone(),
            original_name: original_name.clone(),
            current_name: original_name,
            suggested_name: routing.as_ref().map(|decision| decision.canonical_name.clone()),
            original_path: path_string.clone(),
            current_path: path_string.clone(),
            extension: extension.clone(),
            mime_type: mime_type_for(&extension).to_string(),
            size_bytes: bytes.len() as u64,
            hash,
            classification: Some(ClassificationResult {
                category: classify_text(extraction.text.as_deref().unwrap_or_default(), &extension),
                confidence,
            }),
            tags,
            extracted_text: extraction.text.clone(),
            summary: extraction.text.as_deref().map(summarize),
            confidence,
            duplicate_status: "unique".to_string(),
            processing_status,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        };

        self.upsert_document(&doc)?;
        self.write_audit(doc.id.as_str(), "imported", "system", "Document imported into canonical store", false)?;

        let mut run_embedding_provider = None;
        let chunks = if let Some(text) = &doc.extracted_text {
            stage_start = std::time::Instant::now();
            let chunks = chunk_text(&doc.id, &doc.hash, text, &extension, DEFAULT_CHUNK_TARGET_TOKENS, DEFAULT_CHUNK_OVERLAP_TOKENS);
            self.replace_chunks(&doc, &chunks)?;
            timings.push(stage_timing("chunk", stage_start.elapsed(), "complete"));

            stage_start = std::time::Instant::now();
            let embedding_provider = self.write_embeddings(&chunks)?;
            run_embedding_provider = Some(embedding_provider);
            timings.push(stage_timing("embed", stage_start.elapsed(), "complete"));

            stage_start = std::time::Instant::now();
            self.index_meilisearch(&doc, &chunks).ok();
            timings.push(stage_timing("lexical_index", stage_start.elapsed(), "complete"));

            stage_start = std::time::Instant::now();
            self.write_graph_edges_for_document(&doc).ok();
            self.mirror_falkordb_document(&doc).ok();
            timings.push(stage_timing("graph_index", stage_start.elapsed(), "complete"));

            stage_start = std::time::Instant::now();
            self.cache_hot_path(&doc).ok();
            timings.push(stage_timing("hot_cache", stage_start.elapsed(), "complete"));
            chunks
        } else {
            self.create_review_item(ReviewItem {
                id: stable_id("review", format!("{}:extraction", doc.id).as_bytes()),
                document_id: doc.id.clone(),
                reason: "extraction_warning".to_string(),
                severity: "warning".to_string(),
                title: "Text extraction unavailable".to_string(),
                explanation: extraction.warning.unwrap_or_else(|| "No text could be extracted from this file.".to_string()),
                proposed_action: None,
                created_at: now(),
                resolved: None,
            })?;
            vec![]
        };

        let mut run = PipelineRun::complete(run_id, Some(doc.id.clone()), path_string, timings);
        run.indexed_chunk_count = chunks.len();
        run.ocr_confidence = extraction.ocr_confidence;
        run.embedding_provider = if chunks.is_empty() {
            None
        } else {
            run_embedding_provider
        };
        run.rerank_status = "queued".to_string();
        run.updated_at = now();
        self.upsert_pipeline_run(&run)?;
        let _ = started;
        Ok(IngestOutcome { document: Some(doc), chunks, run })
    }

    pub fn upsert_document(&self, doc: &DocumentRecord) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "INSERT OR REPLACE INTO documents(
                id, original_name, current_name, suggested_name, original_path, current_path,
                extension, mime_type, size_bytes, hash, classification_json, tags_json,
                extracted_text, summary, confidence, duplicate_status, processing_status,
                created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                doc.id,
                doc.original_name,
                doc.current_name,
                doc.suggested_name,
                doc.original_path,
                doc.current_path,
                doc.extension,
                doc.mime_type,
                doc.size_bytes as i64,
                doc.hash,
                to_json_opt(&doc.classification)?,
                serde_json::to_string(&doc.tags)?,
                doc.extracted_text,
                doc.summary,
                doc.confidence,
                doc.duplicate_status,
                doc.processing_status,
                doc.created_at,
                doc.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn get_document(&self, id: &str) -> Result<Option<DocumentRecord>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.query_row("SELECT * FROM documents WHERE id = ?1", params![id], row_to_document)
            .optional()
            .map_err(Into::into)
    }

    pub fn update_document_tags(&self, id: &str, tags: &[String]) -> Result<Option<DocumentRecord>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "UPDATE documents SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![serde_json::to_string(tags)?, now(), id],
        )?;
        drop(conn);
        self.write_audit(id, "tagged", "user", "Document tags updated", true)?;
        self.get_document(id)
    }

    pub fn find_document_by_hash(&self, hash: &str) -> Result<Option<DocumentRecord>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.query_row("SELECT * FROM documents WHERE hash = ?1", params![hash], row_to_document)
            .optional()
            .map_err(Into::into)
    }

    pub fn list_documents(&self, limit: usize, offset: usize) -> Result<Vec<DocumentRecord>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare("SELECT * FROM documents ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2")?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], row_to_document)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn replace_chunks(&self, doc: &DocumentRecord, chunks: &[DocumentChunk]) -> Result<()> {
        let mut conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM chunks_fts WHERE document_id = ?1", params![doc.id])?;
        tx.execute("DELETE FROM chunks WHERE document_id = ?1", params![doc.id])?;
        for chunk in chunks {
            tx.execute(
                "INSERT INTO chunks(id, document_id, chunk_index, text, token_start, token_end, text_hash, source_stage, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    chunk.id,
                    chunk.document_id,
                    chunk.chunk_index as i64,
                    chunk.text,
                    chunk.token_start as i64,
                    chunk.token_end as i64,
                    chunk.text_hash,
                    chunk.source_stage,
                    chunk.created_at,
                ],
            )?;
            tx.execute(
                "INSERT INTO chunks_fts(chunk_id, document_id, title, path, text) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![chunk.id, doc.id, doc.current_name, doc.current_path, chunk.text],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn chunks_for_document(&self, document_id: &str) -> Result<Vec<DocumentChunk>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, document_id, chunk_index, text, token_start, token_end, text_hash, source_stage, created_at
             FROM chunks WHERE document_id = ?1 ORDER BY chunk_index",
        )?;
        let rows = stmt.query_map(params![document_id], row_to_chunk)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn search(&self, query: &str, mode: SearchMode, top_k: usize) -> Result<RetrievalTrace> {
        let trace_id = stable_id("trace", format!("{}:{:?}:{}", query, mode, now()).as_bytes());
        let mut stages = Vec::new();
        let mut candidates = Vec::new();
        let per_stage_limit = top_k.saturating_mul(4).max(top_k).max(1);

        if matches!(mode, SearchMode::Lexical | SearchMode::Hybrid) {
            let start = std::time::Instant::now();
            let mut lexical = self.lexical_search(query, per_stage_limit)?;
            stages.push(RetrievalStageTrace {
                stage: "meilisearch_or_sqlite_fts".to_string(),
                count: lexical.len(),
                elapsed_ms: start.elapsed().as_millis() as u64,
                degraded: self.meili.is_none(),
                message: if self.meili.is_some() { None } else { Some("Using SQLite FTS fallback".to_string()) },
            });
            candidates.append(&mut lexical);
        }

        if matches!(mode, SearchMode::Semantic | SearchMode::Hybrid) {
            let start = std::time::Instant::now();
            let mut semantic = self.semantic_search(query, per_stage_limit)?;
            stages.push(RetrievalStageTrace {
                stage: "semantic_fusion".to_string(),
                count: semantic.len(),
                elapsed_ms: start.elapsed().as_millis() as u64,
                degraded: self.lancedb.is_none() && self.embedding.is_none() && self.model2vec.is_none(),
                message: Some("Merged LanceDB, Jina, and model2vec semantic candidates".to_string()),
            });
            candidates.append(&mut semantic);
        }

        let graph_count = if matches!(mode, SearchMode::Hybrid) {
            let seeds = candidates
                .iter()
                .map(|candidate| candidate.document_id.clone())
                .collect::<Vec<_>>();
            let start = std::time::Instant::now();
            let mut graph = self.graph_expand_candidates(&seeds, per_stage_limit)?;
            let count = graph.len();
            stages.push(RetrievalStageTrace {
                stage: "falkordb_or_sqlite_graph".to_string(),
                count,
                elapsed_ms: start.elapsed().as_millis() as u64,
                degraded: self.falkordb.is_none(),
                message: if self.falkordb.is_some() { None } else { Some("Using SQLite graph edge fallback".to_string()) },
            });
            candidates.append(&mut graph);
            count
        } else {
            0
        };

        let start = std::time::Instant::now();
        let mut merged = rrf_merge_candidates(candidates, top_k.saturating_mul(2).max(top_k).max(1));
        let mut reranked = false;
        if let Some(reranker) = &self.reranker
            && let Ok(results) = reranker.rerank(query, &merged, top_k.max(1))
                && !results.is_empty() {
                    merged = results;
                    reranked = true;
                }
        stages.push(RetrievalStageTrace {
            stage: "colbert_rerank".to_string(),
            count: merged.len(),
            elapsed_ms: start.elapsed().as_millis() as u64,
            degraded: !reranked,
            message: if reranked { None } else { Some("ColBERT unavailable; returning RRF ranking".to_string()) },
        });
        merged.truncate(top_k);
        let trace = RetrievalTrace {
            id: trace_id,
            query: query.to_string(),
            mode,
            top_k,
            lexical_count: stages.iter().find(|s| s.stage.contains("meilisearch")).map(|s| s.count).unwrap_or(0),
            semantic_count: stages.iter().filter(|s| s.stage.contains("semantic")).map(|s| s.count).sum(),
            graph_count,
            reranked,
            stages,
            results: merged,
            created_at: now(),
        };
        self.store_retrieval_trace(&trace)?;
        Ok(trace)
    }

    pub fn store_preference_event(&self, event: PreferenceEvent) -> Result<LearnedPreference> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "INSERT OR REPLACE INTO preference_events(id, category, signal, value, source, confidence, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![event.id, event.category, event.signal, event.value, event.source, event.confidence, event.created_at],
        )?;
        drop(conn);
        self.promote_preference(&event.category, &preference_key(&event.signal), &event.value)
    }

    pub fn list_preferences(&self) -> Result<Vec<LearnedPreference>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, category, key, value, confidence, status, evidence_count, created_at, updated_at
             FROM learned_preferences ORDER BY confidence DESC, updated_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_preference)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn set_preference_status(&self, id: &str, status: &str) -> Result<Option<LearnedPreference>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "UPDATE learned_preferences SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now(), id],
        )?;
        conn.query_row(
            "SELECT id, category, key, value, confidence, status, evidence_count, created_at, updated_at FROM learned_preferences WHERE id = ?1",
            params![id],
            row_to_preference,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_pipeline_runs(&self, limit: usize, offset: usize) -> Result<Vec<PipelineRun>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, document_id, file_path, status, stage_timings_json, failure_json, retryable,
                    indexed_chunk_count, ocr_confidence, embedding_provider, rerank_status, created_at, updated_at
             FROM pipeline_runs ORDER BY updated_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], row_to_pipeline_run)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_pipeline_run(&self, id: &str) -> Result<Option<PipelineRun>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.query_row(
            "SELECT id, document_id, file_path, status, stage_timings_json, failure_json, retryable,
                    indexed_chunk_count, ocr_confidence, embedding_provider, rerank_status, created_at, updated_at
             FROM pipeline_runs WHERE id = ?1",
            params![id],
            row_to_pipeline_run,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_review_items(&self, limit: usize, offset: usize) -> Result<Vec<ReviewItem>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, document_id, reason, severity, title, explanation, proposed_action_json, created_at, resolved
             FROM review_items ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], row_to_review)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn resolve_review(&self, id: &str, resolved: bool) -> Result<Option<ReviewItem>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute("UPDATE review_items SET resolved = ?1 WHERE id = ?2", params![if resolved { 1 } else { 0 }, id])?;
        conn.query_row(
            "SELECT id, document_id, reason, severity, title, explanation, proposed_action_json, created_at, resolved
             FROM review_items WHERE id = ?1",
            params![id],
            row_to_review,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_audit_entries(&self, limit: usize, offset: usize) -> Result<Vec<AuditEntry>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, document_id, action, actor, summary, before_json, after_json, reversible, created_at
             FROM audit_entries ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit as i64, offset as i64], row_to_audit)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn document_summary(&self) -> Result<(usize, u64)> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let (count, total_bytes): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM documents",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok((count.max(0) as usize, total_bytes.max(0) as u64))
    }

    pub fn pipeline_summary(&self) -> Result<(usize, usize, usize, usize, usize)> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let (pending, processing, review, complete_today, errors): (i64, i64, i64, i64, i64) = conn.query_row(
            "
            SELECT
                COALESCE(SUM(CASE WHEN status IN ('queued', 'new') THEN 1 ELSE 0 END), 0) AS pending,
                COALESCE(SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END), 0) AS processing,
                COALESCE((SELECT COUNT(*) FROM review_items WHERE COALESCE(resolved, 0) = 0), 0) AS review,
                COALESCE(SUM(CASE WHEN status = 'complete' AND substr(created_at, 1, 10) = substr(datetime('now'), 1, 10) THEN 1 ELSE 0 END), 0) AS complete_today,
                COALESCE(SUM(CASE WHEN status = 'error' THEN 1 ELSE 0 END), 0) AS errors
            FROM pipeline_runs
            ",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )?;
        Ok((
            pending.max(0) as usize,
            processing.max(0) as usize,
            review.max(0) as usize,
            complete_today.max(0) as usize,
            errors.max(0) as usize,
        ))
    }

    pub fn count_review_items(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM review_items WHERE COALESCE(resolved, 0) = 0",
            [],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    pub fn update_pipeline_run_status(
        &self,
        id: &str,
        status: &str,
        error_message: Option<&str>,
    ) -> Result<Option<PipelineRun>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "UPDATE pipeline_runs SET status = ?1, failure_json = ?2, updated_at = ?3 WHERE id = ?4",
            params![
                status,
                error_message.map(|message| serde_json::json!({ "stage": "cancel", "message": message }).to_string()),
                now(),
                id
            ],
        )?;
        conn.query_row(
            "SELECT id, document_id, file_path, status, stage_timings_json, failure_json, retryable,
                    indexed_chunk_count, ocr_confidence, embedding_provider, rerank_status, created_at, updated_at
             FROM pipeline_runs WHERE id = ?1",
            params![id],
            row_to_pipeline_run,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn audit_entries_for_document(&self, document_id: &str) -> Result<Vec<AuditEntry>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT id, document_id, action, actor, summary, before_json, after_json, reversible, created_at
             FROM audit_entries WHERE document_id = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![document_id], row_to_audit)?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_retrieval_trace(&self, id: &str) -> Result<Option<RetrievalTrace>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let trace_json: Option<String> = conn
            .query_row(
                "SELECT trace_json FROM retrieval_traces WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        trace_json
            .map(|json| serde_json::from_str(&json).map_err(Into::into))
            .transpose()
    }

    fn lexical_search(&self, query: &str, limit: usize) -> Result<Vec<RetrievalCandidate>> {
        if let Some(meili) = &self.meili
            && let Ok(results) = meili.search_chunks(query, limit)
                && !results.is_empty() {
                    return Ok(results);
                }
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        if query.trim().is_empty() {
            let mut stmt = conn.prepare(
                "SELECT c.id, c.document_id, d.current_name, d.current_path, c.text, 1.0
                 FROM chunks c JOIN documents d ON d.id = c.document_id
                 ORDER BY d.updated_at DESC LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit as i64], |row| {
                candidate_from_row(row, "lexical")
            })?;
            return rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into);
        }
        let fts_query = fts5_query(query);
        if fts_query.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = conn.prepare(
            "SELECT f.chunk_id, f.document_id, f.title, f.path, f.text, bm25(chunks_fts) * -1.0 AS score
             FROM chunks_fts f
             WHERE chunks_fts MATCH ?1
             ORDER BY score DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts_query, limit as i64], |row| {
            candidate_from_row(row, "lexical")
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<RetrievalCandidate>> {
        let jina_query_vector = self.embedding.as_ref().and_then(|client| client.embed_one(query).ok());
        let mut out = Vec::new();
        if let (Some(lancedb), Some(query_vector)) = (&self.lancedb, jina_query_vector.as_ref())
            && let Ok(mut results) = lancedb.search(query_vector, limit) {
                out.append(&mut results);
            }
        if let Some(mut jina) = self.semantic_search_table("embeddings", "semantic_jina", query, limit, jina_query_vector.as_deref())? {
            out.append(&mut jina);
        }
        if self.model2vec.is_some() {
            let model2vec_query = self.model2vec.as_ref().and_then(|client| client.embed_one(query).ok());
            if let Some(mut model2vec) = self.semantic_search_table("embeddings_model2vec", "semantic_model2vec", query, limit, model2vec_query.as_deref())? {
                out.append(&mut model2vec);
            }
        }
        out.sort_by(|a, b| b.score.total_cmp(&a.score));
        out.truncate(limit);
        Ok(out)
    }

    pub fn backfill_model2vec_embeddings(&self, batch_size: usize) -> Result<usize> {
        let Some(client) = &self.model2vec else {
            return Ok(0);
        };
        let batch_size = batch_size.max(1);
        let mut inserted = 0usize;

        loop {
            let pending = {
                let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
                let mut stmt = conn.prepare(
                    "SELECT c.id, c.document_id, c.chunk_index, c.text, c.token_start, c.token_end, c.text_hash, c.source_stage, c.created_at
                     FROM chunks c
                     LEFT JOIN embeddings_model2vec e ON e.chunk_id = c.id
                     WHERE e.chunk_id IS NULL
                     ORDER BY c.created_at, c.chunk_index
                     LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![batch_size as i64], row_to_chunk)?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };

            if pending.is_empty() {
                break;
            }

            let texts = pending.iter().map(|chunk| chunk.text.clone()).collect::<Vec<_>>();
            let embeddings = client.embed_batch(&texts)?;
            let mut conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
            let tx = conn.transaction()?;
            for (chunk, vector) in pending.iter().zip(embeddings.iter()) {
                let vector_json = serde_json::to_string(vector)?;
                let vector_hash = sha256_hex(vector_json.as_bytes());
                let profile = build_matryoshka_profile(&client.model_name, chunk, vector, vector.len());
                tx.execute(
                    "INSERT OR REPLACE INTO embeddings_model2vec(
                        chunk_id, provider, dim, vector_json, vector_hash, matryoshka_json,
                        lexical_context, metadata_json, colbert_token_hash, created_at
                     )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    params![
                        chunk.id,
                        &client.model_name,
                        vector.len() as i64,
                        vector_json,
                        vector_hash,
                        serde_json::to_string(&profile)?,
                        profile.lexical_context,
                        serde_json::to_string(&profile.metadata_context)?,
                        profile.colbert_token_hash,
                        now()
                    ],
                )?;
                inserted += 1;
            }
            tx.commit()?;

            if pending.len() < batch_size {
                break;
            }
        }

        Ok(inserted)
    }

    fn semantic_search_table(
        &self,
        table: &str,
        source_stage: &str,
        query: &str,
        limit: usize,
        query_vector: Option<&[f32]>,
    ) -> Result<Option<Vec<RetrievalCandidate>>> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let sql = format!(
            "SELECT c.id, c.document_id, d.current_name, d.current_path, c.text, e.vector_json
             FROM {table} e
             JOIN chunks c ON c.id = e.chunk_id
             JOIN documents d ON d.id = c.document_id"
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let vector_json: String = row.get(5)?;
            let vector: Vec<f32> = serde_json::from_str(&vector_json).unwrap_or_default();
            let fallback_qvec;
            let qvec = if let Some(query_vector) = query_vector {
                query_vector
            } else {
                fallback_qvec = hashed_embedding(query, vector.len().max(DEFAULT_EMBED_DIM));
                fallback_qvec.as_slice()
            };
            out.push(RetrievalCandidate {
                chunk_id: row.get(0)?,
                document_id: row.get(1)?,
                title: row.get(2)?,
                path: row.get(3)?,
                text: row.get(4)?,
                score: coarse_to_fine_similarity(qvec, &vector),
                source_stage: source_stage.to_string(),
            });
        }
        out.sort_by(|a, b| b.score.total_cmp(&a.score));
        out.truncate(limit);
        Ok(if out.is_empty() { None } else { Some(out) })
    }

    fn write_embeddings(&self, chunks: &[DocumentChunk]) -> Result<String> {
        let texts = chunks.iter().map(|chunk| chunk.text.clone()).collect::<Vec<_>>();
        let jina_embeddings = self.embedding.as_ref().and_then(|client| client.embed_batch(&texts).ok());
        let model2vec_embeddings = self.model2vec.as_ref().and_then(|client| client.embed_batch(&texts).ok());
        let jina_provider = jina_embeddings
            .as_ref()
            .and_then(|batch| batch.first())
            .map(|embedding| embedding.provider.clone())
            .unwrap_or_else(|| "direct-hash".to_string());
        let model2vec_provider = self
            .model2vec
            .as_ref()
            .map(|client| client.model_name.clone())
            .unwrap_or_else(|| "model2vec-rs".to_string());
        let provider = if model2vec_embeddings.is_some() {
            format!("{jina_provider}+{model2vec_provider}")
        } else {
            jina_provider.clone()
        };
        let mut conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let tx = conn.transaction()?;
        let mut lance_records = Vec::new();
        for (index, chunk) in chunks.iter().enumerate() {
            let jina_vector = jina_embeddings
                .as_ref()
                .and_then(|batch| batch.get(index))
                .map(|embedding| embedding.vector.clone())
                .unwrap_or_else(|| hashed_embedding(&chunk.text, DEFAULT_EMBED_DIM));
            let jina_vector_json = serde_json::to_string(&jina_vector)?;
            let jina_vector_hash = sha256_hex(jina_vector_json.as_bytes());
            let jina_profile = build_matryoshka_profile(&jina_provider, chunk, &jina_vector, jina_vector.len());
            tx.execute(
                "INSERT OR REPLACE INTO embeddings(
                    chunk_id, provider, dim, vector_json, vector_hash, matryoshka_json,
                    lexical_context, metadata_json, colbert_token_hash, created_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    chunk.id,
                    jina_provider,
                    jina_vector.len() as i64,
                    jina_vector_json,
                    jina_vector_hash,
                    serde_json::to_string(&jina_profile)?,
                    jina_profile.lexical_context,
                    serde_json::to_string(&jina_profile.metadata_context)?,
                    jina_profile.colbert_token_hash,
                    now()
                ],
            )?;
            lance_records.push(LanceVectorRecord {
                id: chunk.id.clone(),
                document_id: chunk.document_id.clone(),
                text: chunk.text.clone(),
                vector: jina_vector,
                metadata: jina_profile.metadata_context,
            });

            if let Some(model2vec_embeddings) = model2vec_embeddings.as_ref()
                && let Some(embedding) = model2vec_embeddings.get(index) {
                    let vector = embedding.clone();
                    let vector_json = serde_json::to_string(&vector)?;
                    let vector_hash = sha256_hex(vector_json.as_bytes());
                    let profile = build_matryoshka_profile(&model2vec_provider, chunk, &vector, vector.len());
                    tx.execute(
                        "INSERT OR REPLACE INTO embeddings_model2vec(
                            chunk_id, provider, dim, vector_json, vector_hash, matryoshka_json,
                            lexical_context, metadata_json, colbert_token_hash, created_at
                         )
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            chunk.id,
                            &model2vec_provider,
                            vector.len() as i64,
                            vector_json,
                            vector_hash,
                            serde_json::to_string(&profile)?,
                            profile.lexical_context,
                            serde_json::to_string(&profile.metadata_context)?,
                            profile.colbert_token_hash,
                            now()
                        ],
                    )?;
                }
        }
        tx.commit()?;
        if let Some(lancedb) = &self.lancedb {
            lancedb.upsert_records(&lance_records).ok();
        }
        Ok(provider)
    }

    fn write_graph_edges_for_document(&self, doc: &DocumentRecord) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        for tag in &doc.tags {
            if tag.trim().is_empty() {
                continue;
            }
            let target = format!("tag:{tag}");
            conn.execute(
                "INSERT OR REPLACE INTO graph_edges(id, from_id, to_id, relation, weight, created_at)
                 VALUES (?1, ?2, ?3, 'HAS_TAG', ?4, ?5)",
                params![
                    stable_id("edge", format!("{}:HAS_TAG:{target}", doc.id).as_bytes()),
                    doc.id,
                    target,
                    0.65f32,
                    now(),
                ],
            )?;
        }
        if let Some(classification) = &doc.classification {
            let target = format!("category:{}", classification.category);
            conn.execute(
                "INSERT OR REPLACE INTO graph_edges(id, from_id, to_id, relation, weight, created_at)
                 VALUES (?1, ?2, ?3, 'CLASSIFIED_AS', ?4, ?5)",
                params![
                    stable_id("edge", format!("{}:CLASSIFIED_AS:{target}", doc.id).as_bytes()),
                    doc.id,
                    target,
                    classification.confidence,
                    now(),
                ],
            )?;
        }
        Ok(())
    }

    fn mirror_falkordb_document(&self, doc: &DocumentRecord) -> Result<()> {
        if let Some(falkor) = &self.falkordb {
            falkor.mirror_document(doc)?;
        }
        Ok(())
    }

    fn graph_expand_candidates(&self, seed_document_ids: &[String], limit: usize) -> Result<Vec<RetrievalCandidate>> {
        if seed_document_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut out = Vec::new();
        for seed in seed_document_ids.iter().take(GRAPH_EXPANSION_LIMIT) {
            let mut stmt = conn.prepare(
                "
                SELECT c.id, c.document_id, d.current_name, d.current_path, c.text, MAX(e.weight) AS score
                FROM graph_edges seed_edge
                JOIN graph_edges e ON e.to_id = seed_edge.to_id AND e.from_id <> seed_edge.from_id
                JOIN chunks c ON c.document_id = e.from_id
                JOIN documents d ON d.id = c.document_id
                WHERE seed_edge.from_id = ?1
                GROUP BY c.id, c.document_id, d.current_name, d.current_path, c.text
                ORDER BY score DESC
                LIMIT ?2
                ",
            )?;
            let rows = stmt.query_map(params![seed, limit as i64], |row| {
                candidate_from_row(row, "graph_expansion")
            })?;
            out.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
        }
        out.sort_by(|a, b| b.score.total_cmp(&a.score));
        out.truncate(limit);
        Ok(out)
    }

    fn upsert_pipeline_run(&self, run: &PipelineRun) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "INSERT OR REPLACE INTO pipeline_runs(
                id, document_id, file_path, status, stage_timings_json, failure_json, retryable,
                indexed_chunk_count, ocr_confidence, embedding_provider, rerank_status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                run.id,
                run.document_id,
                run.file_path,
                run.status,
                serde_json::to_string(&run.stage_timings)?,
                to_json_opt(&run.failure)?,
                if run.retryable { 1 } else { 0 },
                run.indexed_chunk_count as i64,
                run.ocr_confidence,
                run.embedding_provider,
                run.rerank_status,
                run.created_at,
                run.updated_at,
            ],
        )?;
        Ok(())
    }

    fn create_review_item(&self, item: ReviewItem) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "INSERT OR REPLACE INTO review_items(id, document_id, reason, severity, title, explanation, proposed_action_json, created_at, resolved)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                item.id,
                item.document_id,
                item.reason,
                item.severity,
                item.title,
                item.explanation,
                to_json_opt(&item.proposed_action)?,
                item.created_at,
                item.resolved.map(|r| if r { 1 } else { 0 }),
            ],
        )?;
        Ok(())
    }

    fn write_audit(&self, document_id: &str, action: &str, actor: &str, summary: &str, reversible: bool) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "INSERT INTO audit_entries(id, document_id, action, actor, summary, reversible, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![stable_id("audit", format!("{document_id}:{action}:{}", now()).as_bytes()), document_id, action, actor, summary, if reversible { 1 } else { 0 }, now()],
        )?;
        Ok(())
    }

    fn index_meilisearch(&self, doc: &DocumentRecord, chunks: &[DocumentChunk]) -> Result<()> {
        if let Some(meili) = &self.meili {
            meili.index_chunks(doc, chunks)?;
        }
        Ok(())
    }

    fn cache_hot_path(&self, doc: &DocumentRecord) -> Result<()> {
        if let Some(client) = &self.valkey {
            let mut conn = client.get_connection()?;
            let key = format!("masterd:doc:path:{}", doc.hash);
            let _: () = redis::Commands::set_ex(&mut conn, key, &doc.current_path, 86_400)?;
        }
        Ok(())
    }

    fn store_retrieval_trace(&self, trace: &RetrievalTrace) -> Result<()> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        conn.execute(
            "INSERT OR REPLACE INTO retrieval_traces(id, query, mode, trace_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![trace.id, trace.query, format!("{:?}", trace.mode).to_ascii_lowercase(), serde_json::to_string(trace)?, trace.created_at],
        )?;
        Ok(())
    }

    fn promote_preference(&self, category: &str, signal: &str, value: &str) -> Result<LearnedPreference> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("db mutex poisoned"))?;
        let mut stmt = conn.prepare(
            "SELECT signal, confidence, created_at FROM preference_events WHERE category = ?1 AND value = ?2",
        )?;
        let mut rows = stmt.query(params![category, value])?;
        let mut count = 0i64;
        let mut positive_events = 0i64;
        let mut support = 0.0f32;
        let mut opposition = 0.0f32;
        let mut weighted_positive_confidence = 0.0f32;
        let mut positive_weight = 0.0f32;
        while let Some(row) = rows.next()? {
            let raw_signal: String = row.get(0)?;
            if preference_key(&raw_signal) != signal {
                continue;
            }
            let event_confidence = row.get::<_, f32>(1)?.clamp(0.0, 1.0);
            let created_at: String = row.get(2)?;
            let decay = recency_decay(&created_at, 45.0);
            count += 1;
            if preference_signal_is_negative(&raw_signal) {
                opposition += event_confidence * decay;
            } else {
                positive_events += 1;
                support += event_confidence * decay;
                weighted_positive_confidence += event_confidence * decay;
                positive_weight += decay;
            }
        }
        let total = support + opposition;
        let agreement = if total > 0.0 { support / total } else { 0.0 };
        let avg_positive = if positive_weight > 0.0 {
            weighted_positive_confidence / positive_weight
        } else {
            0.0
        };
        let volume = (count as f32 / 5.0).min(1.0);
        let opposition_ratio = if total > 0.0 { opposition / total } else { 0.0 };
        let confidence = ((agreement * 0.65) + (avg_positive * 0.25) + (volume * 0.10) - (opposition_ratio * 0.15)).clamp(0.0, 1.0);
        let status = if confidence >= 0.80 && positive_events >= 3 && opposition_ratio < 0.20 {
            "pending_review"
        } else {
            "learning"
        };
        let id = stable_id("pref", format!("{category}:{signal}:{value}").as_bytes());
        let created_at = now();
        conn.execute(
            "INSERT INTO learned_preferences(id, category, key, value, confidence, status, evidence_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(category, key, value) DO UPDATE SET
                confidence = excluded.confidence,
                status = CASE
                  WHEN learned_preferences.status = 'approved' THEN 'approved'
                  WHEN learned_preferences.status = 'dismissed' THEN 'dismissed'
                  ELSE excluded.status
                END,
                evidence_count = excluded.evidence_count,
                updated_at = excluded.updated_at",
            params![id, category, signal, value, confidence, status, count, created_at],
        )?;
        conn.query_row(
            "SELECT id, category, key, value, confidence, status, evidence_count, created_at, updated_at
             FROM learned_preferences WHERE category = ?1 AND key = ?2 AND value = ?3",
            params![category, signal, value],
            row_to_preference,
        )
        .map_err(Into::into)
    }
}

#[derive(Debug, Clone)]
pub struct IngestConfig {
    pub ocr_language: String,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self { ocr_language: "eng".to_string() }
    }
}

#[derive(Debug, Clone)]
pub struct IngestOutcome {
    pub document: Option<DocumentRecord>,
    pub chunks: Vec<DocumentChunk>,
    pub run: PipelineRun,
}

impl PipelineRun {
    fn complete(id: String, document_id: Option<String>, file_path: String, stage_timings: Vec<StageTiming>) -> Self {
        let ts = now();
        Self {
            id,
            document_id,
            file_path,
            status: "complete".to_string(),
            stage_timings,
            failure: None,
            retryable: false,
            indexed_chunk_count: 0,
            ocr_confidence: None,
            embedding_provider: None,
            rerank_status: "not_run".to_string(),
            created_at: ts.clone(),
            updated_at: ts,
        }
    }

    fn failed(
        id: String,
        document_id: Option<String>,
        file_path: String,
        stage: impl Into<String>,
        message: impl Into<String>,
        retryable: bool,
        stage_timings: Vec<StageTiming>,
    ) -> Self {
        let ts = now();
        Self {
            id,
            document_id,
            file_path,
            status: "error".to_string(),
            stage_timings,
            failure: Some(PipelineFailure { stage: stage.into(), message: message.into() }),
            retryable,
            indexed_chunk_count: 0,
            ocr_confidence: None,
            embedding_provider: None,
            rerank_status: "not_run".to_string(),
            created_at: ts.clone(),
            updated_at: ts,
        }
    }
}

pub fn chunk_text(
    document_id: &str,
    document_hash: &str,
    text: &str,
    extension: &str,
    target_tokens: usize,
    overlap_tokens: usize,
) -> Vec<DocumentChunk> {
    let sections = document_sections(text, extension);
    if sections.is_empty() {
        return vec![];
    }
    let created_at = now();
    sections
        .into_iter()
        .flat_map(|(section_stage, section_text)| {
            chunk_by_semantic_token_budget(&section_text, target_tokens, overlap_tokens)
                .into_iter()
                .map(move |(chunk_text, token_start, token_end)| (section_stage.clone(), chunk_text, token_start, token_end))
        })
        .enumerate()
        .map(|(index, (section_stage, chunk_text, token_start, token_end))| {
            let text_hash = sha256_hex(chunk_text.as_bytes());
            let id = stable_id("chunk", format!("{document_hash}:{index}:{text_hash}").as_bytes());
            DocumentChunk {
                id,
                document_id: document_id.to_string(),
                chunk_index: index,
                text: chunk_text,
                token_start,
                token_end,
                text_hash,
                source_stage: section_stage,
                created_at: created_at.clone(),
            }
        })
        .collect()
}

fn document_sections(text: &str, extension: &str) -> Vec<(String, String)> {
    if extension == "rs"
        && let Some(sections) = rust_structured_sections(text)
            && !sections.is_empty() {
                return sections.into_iter().map(|section| ("ast_section".to_string(), section)).collect();
            }
    semantic_units(text)
        .into_iter()
        .map(|section| ("semantic_section".to_string(), section))
        .collect()
}

fn rust_structured_sections(text: &str) -> Option<Vec<String>> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok()?;
    let tree = parser.parse(text, None)?;
    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut sections = Vec::new();
    for child in root.named_children(&mut cursor) {
        if matches!(
            child.kind(),
            "function_item" | "struct_item" | "enum_item" | "trait_item" | "impl_item" | "mod_item" | "type_item" | "const_item" | "static_item" | "use_declaration"
        )
            && let Ok(snippet) = child.utf8_text(text.as_bytes()) {
                let snippet = snippet.trim();
                if !snippet.is_empty() {
                    sections.push(snippet.to_string());
                }
            }
    }
    Some(sections)
}

fn rrf_merge_candidates(candidates: Vec<RetrievalCandidate>, top_k: usize) -> Vec<RetrievalCandidate> {
    let mut by_stage: BTreeMap<String, Vec<RetrievalCandidate>> = BTreeMap::new();
    for candidate in candidates {
        by_stage.entry(candidate.source_stage.clone()).or_default().push(candidate);
    }
    for stage_candidates in by_stage.values_mut() {
        stage_candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    }

    let mut fused: HashMap<String, (f64, RetrievalCandidate)> = HashMap::new();
    for stage_candidates in by_stage.values() {
        for (rank, candidate) in stage_candidates.iter().enumerate() {
            let weight = match candidate.source_stage.as_str() {
                "semantic_matryoshka" => 1.10,
                "lexical" | "meilisearch" => 1.00,
                "graph_expansion" => 0.72,
                _ => 1.0,
            };
            let score = weight / (RRF_K + rank as f64 + 1.0);
            let entry = fused
                .entry(candidate.chunk_id.clone())
                .or_insert_with(|| (0.0, candidate.clone()));
            entry.0 += score;
            if candidate.score > entry.1.score {
                entry.1 = candidate.clone();
            }
        }
    }

    let mut out: Vec<(f64, RetrievalCandidate)> = fused.into_values().collect();
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut deduper = DocumentDeduper::new();
    let mut merged = Vec::new();
    for (score, mut candidate) in out {
        if deduper.is_duplicate(&candidate.path, &candidate.title, &candidate.text) {
            continue;
        }
        candidate.score = score as f32;
        candidate.source_stage = "hybrid_rrf".to_string();
        merged.push(candidate);
        if merged.len() >= top_k {
            break;
        }
    }
    merged
}

#[derive(Debug, Default)]
struct DocumentDeduper {
    seen_paths: std::collections::HashSet<String>,
    seen_titles: std::collections::HashSet<String>,
    seen_content_hashes: std::collections::HashSet<String>,
}

impl DocumentDeduper {
    fn new() -> Self {
        Self::default()
    }

    fn is_duplicate(&mut self, path: &str, title: &str, content: &str) -> bool {
        let normalized_path = normalize_path_key(path);
        let normalized_title = normalize_title_key(title);
        let content_hash = normalized_content_hash(content);
        let duplicate = (!normalized_path.is_empty() && self.seen_paths.contains(&normalized_path))
            || (!normalized_title.is_empty() && self.seen_titles.contains(&normalized_title))
            || (!content_hash.is_empty() && self.seen_content_hashes.contains(&content_hash));
        if !duplicate {
            if !normalized_path.is_empty() {
                self.seen_paths.insert(normalized_path);
            }
            if !normalized_title.is_empty() {
                self.seen_titles.insert(normalized_title);
            }
            if !content_hash.is_empty() {
                self.seen_content_hashes.insert(content_hash);
            }
        }
        duplicate
    }
}

fn normalize_path_key(path: &str) -> String {
    path.trim()
        .trim_end_matches('/')
        .trim_start_matches("file://")
        .to_ascii_lowercase()
}

fn normalize_title_key(title: &str) -> String {
    title
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalized_content_hash(content: &str) -> String {
    let normalized = content
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        String::new()
    } else {
        sha256_hex(normalized.as_bytes())
    }
}

fn fts5_query(raw: &str) -> String {
    let parsed = parse_query(raw);
    if parsed.positive_terms.is_empty() && parsed.phrases.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    for phrase in parsed.phrases {
        parts.push(format!("\"{}\"", phrase.replace('"', "\"\"")));
    }
    for term in parsed.positive_terms {
        parts.push(format!("\"{}\"", term.replace('"', "\"\"")));
    }
    for term in parsed.negative_terms {
        parts.push(format!("NOT \"{}\"", term.replace('"', "\"\"")));
    }
    parts.join(" AND ")
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct ParsedQuery {
    positive_terms: Vec<String>,
    negative_terms: Vec<String>,
    phrases: Vec<String>,
    filters: HashMap<String, String>,
}

fn parse_query(raw: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery::default();
    let mut current = String::new();
    let mut in_quote = false;
    let mut negated = false;

    for ch in raw.chars().chain(std::iter::once(' ')) {
        match ch {
            '"' => {
                if in_quote {
                    let value = clean_query_token(&current);
                    if !value.is_empty() {
                        if negated {
                            parsed.negative_terms.push(value);
                        } else {
                            parsed.phrases.push(value);
                        }
                    }
                    current.clear();
                    in_quote = false;
                    negated = false;
                } else {
                    if current.trim() == "-" {
                        negated = true;
                        current.clear();
                    }
                    in_quote = true;
                }
            }
            '-' if current.is_empty() && !in_quote => {
                negated = true;
            }
            ch if ch.is_whitespace() && !in_quote => {
                let value = clean_query_token(&current);
                if let Some((key, val)) = value.split_once(':') {
                    if !key.is_empty() && !val.is_empty() {
                        parsed.filters.insert(key.to_string(), val.to_string());
                    }
                } else if !value.is_empty() {
                    if negated {
                        parsed.negative_terms.push(value);
                    } else {
                        parsed.positive_terms.push(value);
                    }
                }
                current.clear();
                negated = false;
            }
            _ => current.push(ch),
        }
    }

    parsed.positive_terms.sort();
    parsed.positive_terms.dedup();
    parsed.negative_terms.sort();
    parsed.negative_terms.dedup();
    parsed.phrases.sort();
    parsed.phrases.dedup();
    parsed
}

fn clean_query_token(token: &str) -> String {
    token
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '+' | '#' | '.' | '@' | ':'))
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase()
}

fn preference_key(signal: &str) -> String {
    let lower = signal.trim().to_ascii_lowercase();
    for prefix in [
        "accepted_",
        "accept_",
        "approved_",
        "approve_",
        "selected_",
        "select_",
        "rejected_",
        "reject_",
        "dismissed_",
        "dismiss_",
        "corrected_",
        "correction_",
        "undo_",
    ] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            return rest.to_string();
        }
    }
    lower
}

fn preference_signal_is_negative(signal: &str) -> bool {
    let lower = signal.trim().to_ascii_lowercase();
    ["rejected_", "reject_", "dismissed_", "dismiss_", "corrected_", "correction_", "undo_"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn recency_decay(created_at: &str, half_life_days: f32) -> f32 {
    let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return 1.0;
    };
    let age_seconds = (Utc::now() - timestamp.with_timezone(&Utc)).num_seconds().max(0) as f32;
    let age_days = age_seconds / 86_400.0;
    0.5f32.powf(age_days / half_life_days.max(1.0))
}

fn chunk_by_semantic_token_budget(text: &str, target_tokens: usize, overlap_tokens: usize) -> Vec<(String, usize, usize)> {
    let units = semantic_units(text);
    if units.is_empty() {
        return Vec::new();
    }
    let target = target_tokens.max(1);
    let overlap = overlap_tokens.min(target.saturating_sub(1));
    let mut chunks = Vec::new();
    let mut current_tokens = Vec::new();
    let mut start_token = 0usize;
    let mut cursor = 0usize;

    for unit in units {
        let tokens = unit.split_whitespace().collect::<Vec<_>>();
        if tokens.is_empty() {
            continue;
        }
        if tokens.len() > target {
            if !current_tokens.is_empty() {
                let end_token = cursor;
                chunks.push((current_tokens.join(" "), start_token, end_token));
                current_tokens.clear();
            }
            let step = target.saturating_sub(overlap).max(1);
            let mut local_start = 0usize;
            while local_start < tokens.len() {
                let local_end = (local_start + target).min(tokens.len());
                chunks.push((
                    tokens[local_start..local_end].join(" "),
                    cursor + local_start,
                    cursor + local_end,
                ));
                if local_end == tokens.len() {
                    break;
                }
                local_start += step;
            }
            cursor += tokens.len();
            start_token = cursor;
            continue;
        }
        if !current_tokens.is_empty() && current_tokens.len() + tokens.len() > target {
            let end_token = cursor;
            chunks.push((current_tokens.join(" "), start_token, end_token));
            let keep = overlap.min(current_tokens.len());
            let retained = current_tokens[current_tokens.len().saturating_sub(keep)..].to_vec();
            start_token = end_token.saturating_sub(retained.len());
            current_tokens = retained;
        }
        cursor += tokens.len();
        current_tokens.extend(tokens.into_iter().map(str::to_string));
    }

    if !current_tokens.is_empty() {
        chunks.push((current_tokens.join(" "), start_token, cursor));
    }
    chunks
}

fn semantic_units(text: &str) -> Vec<String> {
    let mut units = Vec::new();
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if paragraph.starts_with('#') {
            units.push(paragraph.to_string());
            continue;
        }
        let mut current = String::new();
        for ch in paragraph.chars() {
            current.push(ch);
            if matches!(ch, '.' | '!' | '?') && current.split_whitespace().count() >= 8 {
                units.push(current.trim().to_string());
                current.clear();
            }
        }
        if !current.trim().is_empty() {
            units.push(current.trim().to_string());
        }
    }
    units
}

#[allow(dead_code)]
fn mean_pool_token_matrix(token_matrix: &[Vec<f32>]) -> Vec<f32> {
    if token_matrix.is_empty() {
        return vec![];
    }
    let dim = token_matrix[0].len();
    let mut mean = vec![0.0f32; dim];
    let mut count = 0f32;
    for token in token_matrix {
        if token.len() != dim {
            continue;
        }
        count += 1.0;
        for (index, value) in token.iter().enumerate() {
            mean[index] += *value;
        }
    }
    if count > 0.0 {
        for value in &mut mean {
            *value /= count;
        }
    }
    mean
}

#[allow(dead_code)]
fn token_matrix_hash(token_matrix: &[Vec<f32>]) -> String {
    let mut bytes = Vec::new();
    for token in token_matrix {
        for value in token {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    sha256_hex(&bytes)
}

#[derive(Debug, Clone)]
struct MeilisearchClient {
    base_url: String,
    client: Client,
}

impl MeilisearchClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn index_chunks(&self, doc: &DocumentRecord, chunks: &[DocumentChunk]) -> Result<()> {
        let documents: Vec<_> = chunks
            .iter()
            .map(|chunk| {
                serde_json::json!({
                    "id": chunk.id,
                    "document_id": doc.id,
                    "title": doc.current_name,
                    "path": doc.current_path,
                    "text": chunk.text,
                    "source_stage": chunk.source_stage,
                })
            })
            .collect();
        let url = format!("{}/indexes/masterd_chunks/documents?primaryKey=id", self.base_url.trim_end_matches('/'));
        let response = self.client.post(url).json(&documents).send()?;
        if !response.status().is_success() {
            anyhow::bail!("meilisearch index failed: HTTP {}", response.status());
        }
        Ok(())
    }

    fn search_chunks(&self, query: &str, limit: usize) -> Result<Vec<RetrievalCandidate>> {
        #[derive(Debug, Deserialize)]
        struct SearchResponse {
            hits: Vec<MeiliChunkHit>,
        }
        #[derive(Debug, Deserialize)]
        struct MeiliChunkHit {
            id: String,
            document_id: String,
            title: String,
            path: String,
            text: String,
            #[serde(default, rename = "_rankingScore", alias = "_ranking_score")]
            ranking_score: Option<f32>,
        }

        let url = format!("{}/indexes/masterd_chunks/search", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({
                "q": query,
                "limit": limit,
                "showRankingScore": true,
            }))
            .send()?;
        if !response.status().is_success() {
            anyhow::bail!("meilisearch search failed: HTTP {}", response.status());
        }
        let body: SearchResponse = response.json()?;
        Ok(body
            .hits
            .into_iter()
            .map(|hit| RetrievalCandidate {
                document_id: hit.document_id,
                chunk_id: hit.id,
                title: hit.title,
                path: hit.path,
                text: hit.text,
                score: hit.ranking_score.unwrap_or(1.0),
                source_stage: "meilisearch".to_string(),
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
struct StoredEmbedding {
    vector: Vec<f32>,
    provider: String,
}

#[derive(Debug, Clone)]
struct EmbeddingServiceClient {
    base_url: String,
    model: String,
    client: Client,
}

impl EmbeddingServiceClient {
    fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self
            .embed_batch(&[text.to_string()])?
            .into_iter()
            .next()
            .map(|embedding| embedding.vector)
            .unwrap_or_default())
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<StoredEmbedding>> {
        #[derive(Debug, Deserialize)]
        struct EmbeddingDatum {
            embedding: Vec<f32>,
            #[serde(default)]
            index: Option<usize>,
        }
        #[derive(Debug, Deserialize)]
        struct EmbeddingResponse {
            #[serde(default)]
            embeddings: Option<Vec<Vec<f32>>>,
            #[serde(default)]
            data: Option<Vec<EmbeddingDatum>>,
            #[serde(default)]
            model: Option<String>,
        }

        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/v1/embeddings", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({
                "input": texts,
                "model": self.model,
            }))
            .send()?;
        if !response.status().is_success() {
            anyhow::bail!("embedding service failed: HTTP {}", response.status());
        }
        let body: EmbeddingResponse = response.json()?;
        let provider = body.model.unwrap_or_else(|| self.model.clone());
        let mut vectors = if let Some(mut data) = body.data {
            data.sort_by_key(|datum| datum.index.unwrap_or(usize::MAX));
            data.into_iter().map(|datum| datum.embedding).collect::<Vec<_>>()
        } else {
            body.embeddings.unwrap_or_default()
        };
        vectors.truncate(texts.len());
        Ok(vectors
            .into_iter()
            .map(|mut vector| {
                normalize_vector(&mut vector);
                StoredEmbedding { vector, provider: provider.clone() }
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
struct Model2VecClient {
    base_url: String,
    model_name: String,
    client: Client,
}

impl Model2VecClient {
    fn new(base_url: String, model_name: String) -> Self {
        Self {
            base_url,
            model_name,
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        Ok(self
            .embed_batch(&[text.to_string()])?
            .into_iter()
            .next()
            .unwrap_or_default())
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/v1/embeddings", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({
                "input": texts,
                "model": self.model_name,
            }))
            .send()?;
        if !response.status().is_success() {
            anyhow::bail!("model2vec service failed: HTTP {}", response.status());
        }
        parse_model2vec_embedding_response(response.json()?, texts.len())
    }
}

#[derive(Debug, Deserialize)]
struct Model2VecEmbeddingDatum {
    embedding: Vec<f32>,
    #[serde(default)]
    index: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct Model2VecEmbeddingResponse {
    #[serde(default)]
    embeddings: Option<Vec<Vec<f32>>>,
    #[serde(default)]
    data: Option<Vec<Model2VecEmbeddingDatum>>,
}

fn parse_model2vec_embedding_response(
    body: Model2VecEmbeddingResponse,
    expected_len: usize,
) -> Result<Vec<Vec<f32>>> {
    let mut vectors = if let Some(mut data) = body.data {
        data.sort_by_key(|datum| datum.index.unwrap_or(usize::MAX));
        data.into_iter().map(|datum| datum.embedding).collect::<Vec<_>>()
    } else {
        body.embeddings.unwrap_or_default()
    };
    vectors.truncate(expected_len);
    Ok(vectors)
}

#[derive(Debug, Clone)]
struct ColbertRerankerClient {
    base_url: String,
    client: Client,
}

impl ColbertRerankerClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn rerank(&self, query: &str, candidates: &[RetrievalCandidate], top_k: usize) -> Result<Vec<RetrievalCandidate>> {
        #[derive(Debug, Deserialize)]
        struct RerankResult {
            index: usize,
            #[serde(default, alias = "relevance_score")]
            score: f32,
        }
        #[derive(Debug, Deserialize)]
        struct RerankResponse {
            #[serde(default)]
            results: Vec<RerankResult>,
            #[serde(default)]
            ranked_indices: Vec<usize>,
            #[serde(default)]
            scores: Vec<f32>,
        }

        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let documents = candidates.iter().map(|candidate| candidate.text.clone()).collect::<Vec<_>>();
        let url = format!("{}/v1/rerank", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({
                "query": query,
                "documents": documents,
                "top_k": top_k,
            }))
            .send()?;
        if !response.status().is_success() {
            anyhow::bail!("colbert rerank failed: HTTP {}", response.status());
        }
        let body: RerankResponse = response.json()?;
        let pairs = if body.results.is_empty() {
            body.ranked_indices
                .into_iter()
                .enumerate()
                .map(|(rank, index)| {
                    let fallback = 1.0f32 / (rank as f32 + 1.0);
                    (index, body.scores.get(rank).copied().unwrap_or(fallback))
                })
                .collect::<Vec<_>>()
        } else {
            body.results.into_iter().map(|result| (result.index, result.score)).collect()
        };
        let mut out = Vec::new();
        for (index, score) in pairs {
            if let Some(candidate) = candidates.get(index) {
                let mut candidate = candidate.clone();
                candidate.score = score;
                candidate.source_stage = "colbert_rerank".to_string();
                out.push(candidate);
            }
            if out.len() >= top_k {
                break;
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Serialize)]
struct LanceVectorRecord {
    id: String,
    document_id: String,
    text: String,
    vector: Vec<f32>,
    metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
struct LanceDbClient {
    base_url: String,
    client: Client,
}

impl LanceDbClient {
    fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn upsert_records(&self, records: &[LanceVectorRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let url = format!("{}/upsert", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({
                "table": "masterd_chunks",
                "records": records,
            }))
            .send()?;
        if !response.status().is_success() {
            anyhow::bail!("lancedb upsert failed: HTTP {}", response.status());
        }
        Ok(())
    }

    fn search(&self, vector: &[f32], limit: usize) -> Result<Vec<RetrievalCandidate>> {
        #[derive(Debug, Deserialize)]
        struct LanceRow {
            id: String,
            document_id: String,
            #[serde(default)]
            title: Option<String>,
            #[serde(default)]
            path: Option<String>,
            text: String,
            #[serde(default, alias = "_distance")]
            score: Option<f32>,
        }
        #[derive(Debug, Deserialize)]
        struct LanceSearchResponse {
            #[serde(default, alias = "results")]
            rows: Vec<LanceRow>,
        }

        let url = format!("{}/search", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({
                "table": "masterd_chunks",
                "vector": vector,
                "limit": limit,
            }))
            .send()?;
        if !response.status().is_success() {
            anyhow::bail!("lancedb search failed: HTTP {}", response.status());
        }
        let body: LanceSearchResponse = response.json()?;
        Ok(body
            .rows
            .into_iter()
            .map(|row| RetrievalCandidate {
                document_id: row.document_id,
                chunk_id: row.id,
                title: row.title.unwrap_or_default(),
                path: row.path.unwrap_or_default(),
                text: row.text,
                score: row.score.unwrap_or(0.0),
                source_stage: "semantic_matryoshka".to_string(),
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
struct FalkorDbClient {
    client: redis::Client,
}

impl FalkorDbClient {
    fn new(url: String) -> Result<Self> {
        Ok(Self { client: redis::Client::open(url)? })
    }

    fn mirror_document(&self, doc: &DocumentRecord) -> Result<()> {
        let mut conn = self.client.get_connection()?;
        let doc_name = cypher_string(&doc.current_name);
        let doc_path = cypher_string(&doc.current_path);
        let doc_id = cypher_string(&doc.id);
        let category = doc
            .classification
            .as_ref()
            .map(|classification| classification.category.as_str())
            .unwrap_or("General / Document");
        let query = format!(
            "MERGE (d:Document {{id: {doc_id}}}) SET d.title = {doc_name}, d.path = {doc_path} \
             MERGE (c:Category {{name: {}}}) MERGE (d)-[:CLASSIFIED_AS]->(c)",
            cypher_string(category)
        );
        let _: redis::Value = redis::cmd("GRAPH.QUERY").arg("masterd").arg(query).query(&mut conn)?;
        for tag in &doc.tags {
            let query = format!(
                "MATCH (d:Document {{id: {doc_id}}}) MERGE (t:Tag {{name: {}}}) MERGE (d)-[:HAS_TAG]->(t)",
                cypher_string(tag)
            );
            let _: redis::Value = redis::cmd("GRAPH.QUERY").arg("masterd").arg(query).query(&mut conn)?;
        }
        Ok(())
    }
}

fn cypher_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[derive(Debug)]
struct ExtractionResult {
    text: Option<String>,
    ocr_confidence: Option<f32>,
    warning: Option<String>,
}

fn extract_text(path: &Path, bytes: &[u8], ocr_language: &str) -> ExtractionResult {
    let ext = extension(path);
    match ext.as_str() {
        "txt" | "md" | "rst" | "log" | "csv" | "json" | "toml" | "yaml" | "yml" => {
            let text = String::from_utf8_lossy(bytes).to_string();
            ExtractionResult { text: Some(text), ocr_confidence: None, warning: None }
        }
        "pdf" => {
            let text = lossy_pdf_text(bytes);
            if text.split_whitespace().count() >= 20 {
                ExtractionResult { text: Some(text), ocr_confidence: None, warning: None }
            } else {
                ExtractionResult {
                    text: None,
                    ocr_confidence: Some(0.0),
                    warning: Some(format!(
                        "Native PDF text was unavailable. OCR fallback for language '{ocr_language}' is queued for review until Tesseract extraction is wired into the desktop runtime."
                    )),
                }
            }
        }
        "png" | "jpg" | "jpeg" | "tif" | "tiff" | "webp" => ExtractionResult {
            text: None,
            ocr_confidence: Some(0.0),
            warning: Some(format!(
                "Image OCR fallback for language '{ocr_language}' is queued for review until Tesseract extraction is wired into the desktop runtime."
            )),
        },
        _ => ExtractionResult {
            text: None,
            ocr_confidence: None,
            warning: Some(format!("Unsupported file extension '{ext}'.")),
        },
    }
}

fn lossy_pdf_text(bytes: &[u8]) -> String {
    let raw = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut in_paren = false;
    let mut escaped = false;
    for ch in raw.chars() {
        if escaped {
            if in_paren {
                out.push(ch);
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_paren => escaped = true,
            '(' if !in_paren => in_paren = true,
            ')' if in_paren => {
                in_paren = false;
                out.push(' ');
            }
            _ if in_paren && !ch.is_control() => out.push(ch),
            _ => {}
        }
    }
    out
}

fn row_to_document(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentRecord> {
    let classification_json: Option<String> = row.get(10)?;
    let tags_json: String = row.get(11)?;
    Ok(DocumentRecord {
        id: row.get(0)?,
        original_name: row.get(1)?,
        current_name: row.get(2)?,
        suggested_name: row.get(3)?,
        original_path: row.get(4)?,
        current_path: row.get(5)?,
        extension: row.get(6)?,
        mime_type: row.get(7)?,
        size_bytes: row.get::<_, i64>(8)? as u64,
        hash: row.get(9)?,
        classification: classification_json.and_then(|json| serde_json::from_str(&json).ok()),
        tags: serde_json::from_str(&tags_json).unwrap_or_default(),
        extracted_text: row.get(12)?,
        summary: row.get(13)?,
        confidence: row.get(14)?,
        duplicate_status: row.get(15)?,
        processing_status: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

fn row_to_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentChunk> {
    Ok(DocumentChunk {
        id: row.get(0)?,
        document_id: row.get(1)?,
        chunk_index: row.get::<_, i64>(2)? as usize,
        text: row.get(3)?,
        token_start: row.get::<_, i64>(4)? as usize,
        token_end: row.get::<_, i64>(5)? as usize,
        text_hash: row.get(6)?,
        source_stage: row.get(7)?,
        created_at: row.get(8)?,
    })
}

fn row_to_pipeline_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<PipelineRun> {
    let stage_json: String = row.get(4)?;
    let failure_json: Option<String> = row.get(5)?;
    Ok(PipelineRun {
        id: row.get(0)?,
        document_id: row.get(1)?,
        file_path: row.get(2)?,
        status: row.get(3)?,
        stage_timings: serde_json::from_str(&stage_json).unwrap_or_default(),
        failure: failure_json.and_then(|json| serde_json::from_str(&json).ok()),
        retryable: row.get::<_, i64>(6)? != 0,
        indexed_chunk_count: row.get::<_, i64>(7)? as usize,
        ocr_confidence: row.get(8)?,
        embedding_provider: row.get(9)?,
        rerank_status: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn row_to_review(row: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewItem> {
    let proposed_action_json: Option<String> = row.get(6)?;
    let resolved: Option<i64> = row.get(8)?;
    Ok(ReviewItem {
        id: row.get(0)?,
        document_id: row.get(1)?,
        reason: row.get(2)?,
        severity: row.get(3)?,
        title: row.get(4)?,
        explanation: row.get(5)?,
        proposed_action: proposed_action_json.and_then(|json| serde_json::from_str(&json).ok()),
        created_at: row.get(7)?,
        resolved: resolved.map(|v| v != 0),
    })
}

fn row_to_preference(row: &rusqlite::Row<'_>) -> rusqlite::Result<LearnedPreference> {
    Ok(LearnedPreference {
        id: row.get(0)?,
        category: row.get(1)?,
        key: row.get(2)?,
        value: row.get(3)?,
        confidence: row.get(4)?,
        status: row.get(5)?,
        evidence_count: row.get::<_, i64>(6)? as usize,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn row_to_audit(row: &rusqlite::Row<'_>) -> rusqlite::Result<AuditEntry> {
    let before_json: Option<String> = row.get(5)?;
    let after_json: Option<String> = row.get(6)?;
    Ok(AuditEntry {
        id: row.get(0)?,
        document_id: row.get(1)?,
        action: row.get(2)?,
        actor: row.get(3)?,
        summary: row.get(4)?,
        before: before_json.and_then(|json| serde_json::from_str(&json).ok()),
        after: after_json.and_then(|json| serde_json::from_str(&json).ok()),
        reversible: row.get::<_, i64>(7)? != 0,
        created_at: row.get(8)?,
    })
}

fn candidate_from_row(row: &rusqlite::Row<'_>, source_stage: &str) -> rusqlite::Result<RetrievalCandidate> {
    Ok(RetrievalCandidate {
        chunk_id: row.get(0)?,
        document_id: row.get(1)?,
        title: row.get(2)?,
        path: row.get(3)?,
        text: row.get(4)?,
        score: row.get::<_, f32>(5)?,
        source_stage: source_stage.to_string(),
    })
}

fn to_json_opt<T: Serialize>(value: &Option<T>) -> Result<Option<String>> {
    value.as_ref().map(serde_json::to_string).transpose().map_err(Into::into)
}

fn hashed_embedding(text: &str, dim: usize) -> Vec<f32> {
    let mut vector = vec![0.0f32; dim.max(8)];
    for token in text.split_whitespace() {
        let digest = Sha256::digest(token.to_ascii_lowercase().as_bytes());
        let idx = u64::from_le_bytes(digest[0..8].try_into().unwrap()) as usize % vector.len();
        let sign = if digest[8] & 1 == 0 { 1.0 } else { -1.0 };
        vector[idx] += sign;
    }
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vector {
            *v /= norm;
        }
    }
    vector
}

fn normalize_vector(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector {
            *value /= norm;
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn coarse_to_fine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let mut score = 0.0;
    let mut weight_sum = 0.0;
    for (dim, weight) in [(64usize, 0.20f32), (128, 0.30), (384, 0.50)] {
        let dim = dim.min(a.len()).min(b.len());
        if dim == 0 {
            continue;
        }
        score += cosine_similarity(&a[..dim], &b[..dim]) * weight;
        weight_sum += weight;
    }
    if weight_sum > 0.0 { score / weight_sum } else { 0.0 }
}

fn build_matryoshka_profile(
    provider: &str,
    chunk: &DocumentChunk,
    vector: &[f32],
    full_dim: usize,
) -> MatryoshkaEmbeddingProfile {
    let prefix_dims = MATRYOSHKA_DIMS
        .iter()
        .copied()
        .filter(|dim| *dim <= vector.len())
        .collect::<Vec<_>>();
    let lexical_context = chunk
        .text
        .split_whitespace()
        .take(96)
        .collect::<Vec<_>>()
        .join(" ");
    let metadata_context = serde_json::json!({
        "documentId": chunk.document_id,
        "chunkId": chunk.id,
        "chunkIndex": chunk.chunk_index,
        "tokenStart": chunk.token_start,
        "tokenEnd": chunk.token_end,
        "sourceStage": chunk.source_stage,
        "textHash": chunk.text_hash,
        "matryoshkaDims": prefix_dims,
    });
    let colbert_token_hash = sha256_hex(
        chunk
            .text
            .split_whitespace()
            .map(|token| token.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
    );
    MatryoshkaEmbeddingProfile {
        provider: provider.to_string(),
        full_dim,
        prefix_dims,
        lexical_context,
        metadata_context,
        colbert_token_hash,
    }
}

fn stage_timing(stage: &str, elapsed: Duration, status: &str) -> StageTiming {
    StageTiming {
        stage: stage.to_string(),
        elapsed_ms: elapsed.as_millis() as u64,
        status: status.to_string(),
    }
}

fn stable_id(prefix: &str, bytes: &[u8]) -> String {
    format!("{prefix}-{}", &sha256_hex(bytes)[..24])
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn mime_type_for(ext: &str) -> &'static str {
    match ext {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "md" => "text/markdown",
        "json" => "application/json",
        _ => "text/plain",
    }
}

#[derive(Debug, Clone)]
struct LocalRoutingDecision {
    route: String,
    canonical_name: String,
    tags: Vec<String>,
}

fn routing_decision_for(path: &Path, content_hash: &str) -> Option<LocalRoutingDecision> {
    let ext = extension(path);
    let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("file");
    let ext_with_dot = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    let hash8 = &content_hash[..content_hash.len().min(8)];
    let (route, tags): (&str, &[&str]) = match ext.as_str() {
        "pdf" => ("documents/pdf", &["pdf", "document"]),
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tiff" => ("media/images", &["image", "media"]),
        "txt" | "md" | "rst" | "rtf" => ("documents/text", &["text", "document"]),
        "rs" | "py" | "js" | "ts" | "go" | "java" | "c" | "cpp" | "h" | "hpp" => ("code", &["code"]),
        "xlsx" | "xls" | "csv" | "ods" => ("documents/spreadsheets", &["spreadsheet", "data"]),
        "zip" | "tar" | "gz" | "bz2" | "7z" | "rar" => ("archives", &["archive"]),
        "" => return None,
        _ => ("uncategorized", &["uncategorized"]),
    };
    Some(LocalRoutingDecision {
        route: route.to_string(),
        canonical_name: format!("{stem}_{hash8}{ext_with_dot}"),
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
    })
}

fn classify_text(text: &str, ext: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if ext == "pdf" && lower.contains("invoice") {
        "Financial / Invoice".to_string()
    } else if lower.contains("receipt") || lower.contains("total:") {
        "Financial / Receipt".to_string()
    } else if lower.contains("abstract") || lower.contains("references") {
        "Academic / Research Paper".to_string()
    } else {
        "General / Document".to_string()
    }
}

fn infer_tags(ext: &str, text: &str) -> Vec<String> {
    let mut tags = vec![ext.to_string()];
    let lower = text.to_ascii_lowercase();
    for (needle, tag) in [
        ("invoice", "invoice"),
        ("receipt", "receipt"),
        ("abstract", "research"),
        ("tax", "tax"),
    ] {
        if lower.contains(needle) {
            tags.push(tag.to_string());
        }
    }
    tags.sort();
    tags.dedup();
    tags
}

fn summarize(text: &str) -> String {
    let mut summary = text.split_whitespace().take(48).collect::<Vec<_>>().join(" ");
    if text.split_whitespace().count() > 48 {
        summary.push_str("...");
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_ids_are_stable() {
        let text = (0..2200).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ");
        let a = chunk_text("doc1", "hash1", &text, "txt", 800, 120);
        let b = chunk_text("doc1", "hash1", &text, "txt", 800, 120);
        assert_eq!(a.len(), 4);
        assert_eq!(a.iter().map(|c| &c.id).collect::<Vec<_>>(), b.iter().map(|c| &c.id).collect::<Vec<_>>());
        assert_eq!(a[1].token_start, 680);
    }

    #[test]
    fn rrf_merge_deduplicates_chunks() {
        let c = |stage: &str, score: f32| RetrievalCandidate {
            document_id: "d".into(),
            chunk_id: "c".into(),
            title: "t".into(),
            path: "/tmp/t".into(),
            text: "hello".into(),
            score,
            source_stage: stage.into(),
        };
        let out = rrf_merge_candidates(vec![c("lexical", 0.9), c("semantic", 0.7)], 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source_stage, "hybrid_rrf");
    }

    #[test]
    fn model2vec_parser_accepts_embeddings_shape() {
        let body = serde_json::from_value::<Model2VecEmbeddingResponse>(serde_json::json!({
            "embeddings": [[1.0, 2.0], [3.0, 4.0]],
            "model": "fixture-model"
        }))
        .unwrap();

        let vectors = parse_model2vec_embedding_response(body, 2).unwrap();

        assert_eq!(vectors, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    }

    #[test]
    fn model2vec_parser_accepts_openai_data_shape() {
        let body = serde_json::from_value::<Model2VecEmbeddingResponse>(serde_json::json!({
            "data": [
                {"embedding": [3.0, 4.0], "index": 1, "object": "embedding"},
                {"embedding": [1.0, 2.0], "index": 0, "object": "embedding"}
            ],
            "model": "fixture-model"
        }))
        .unwrap();

        let vectors = parse_model2vec_embedding_response(body, 2).unwrap();

        assert_eq!(vectors, vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    }

    #[test]
    fn migrations_record_version() {
        let path = std::env::temp_dir().join(format!("masterd-data-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut config = DataStoreConfig::local(path.clone());
        config.meilisearch_url = None;
        config.valkey_url = None;
        config.embedding_url = None;
        config.model2vec_url = None;
        config.model2vec_model = None;
        config.colbert_url = None;
        config.lancedb_url = None;
        config.falkordb_url = None;
        let store = DataStore::open(config).unwrap();
        assert_eq!(store.migration_versions().unwrap(), vec![1]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn preference_promotes_after_evidence() {
        let path = std::env::temp_dir().join(format!("masterd-pref-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut config = DataStoreConfig::local(path.clone());
        config.meilisearch_url = None;
        config.valkey_url = None;
        config.embedding_url = None;
        config.model2vec_url = None;
        config.model2vec_model = None;
        config.colbert_url = None;
        config.lancedb_url = None;
        config.falkordb_url = None;
        let store = DataStore::open(config).unwrap();
        let mut last = None;
        for i in 0..3 {
            last = Some(store.store_preference_event(PreferenceEvent {
                id: format!("e{i}"),
                category: "naming".into(),
                signal: "prefix".into(),
                value: "date".into(),
                source: "test".into(),
                confidence: 0.95,
                created_at: now(),
            }).unwrap());
        }
        assert_eq!(last.unwrap().status, "pending_review");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejected_evidence_suppresses_preference_promotion() {
        let path = std::env::temp_dir().join(format!("masterd-pref-neg-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut config = DataStoreConfig::local(path.clone());
        config.meilisearch_url = None;
        config.valkey_url = None;
        config.embedding_url = None;
        config.model2vec_url = None;
        config.model2vec_model = None;
        config.colbert_url = None;
        config.lancedb_url = None;
        config.falkordb_url = None;
        let store = DataStore::open(config).unwrap();
        for i in 0..3 {
            let _ = store.store_preference_event(PreferenceEvent {
                id: format!("accept-{i}"),
                category: "naming".into(),
                signal: "accepted_prefix".into(),
                value: "date".into(),
                source: "test".into(),
                confidence: 0.95,
                created_at: now(),
            }).unwrap();
        }
        let learned = store.store_preference_event(PreferenceEvent {
            id: "reject-1".into(),
            category: "naming".into(),
            signal: "rejected_prefix".into(),
            value: "date".into(),
            source: "test".into(),
            confidence: 0.95,
            created_at: now(),
        }).unwrap();
        assert_eq!(learned.status, "learning");
        assert!(learned.confidence < 0.80);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn matryoshka_profile_keeps_prefix_dims_and_context() {
        let chunk = DocumentChunk {
            id: "c1".into(),
            document_id: "d1".into(),
            chunk_index: 0,
            text: "alpha beta gamma delta".into(),
            token_start: 0,
            token_end: 4,
            text_hash: "h".into(),
            source_stage: "extract_text".into(),
            created_at: now(),
        };
        let profile = build_matryoshka_profile("jina-omni", &chunk, &vec![0.0; 384], 384);
        assert_eq!(profile.prefix_dims, vec![64, 128, 256, 384]);
        assert!(profile.lexical_context.contains("alpha beta"));
        assert!(!profile.colbert_token_hash.is_empty());
    }
}
