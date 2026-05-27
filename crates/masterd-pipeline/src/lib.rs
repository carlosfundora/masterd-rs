use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::Utc;
use masterd_core::{CancellationSource, CancellationToken};
use redis::Commands;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

pub mod naming;
pub mod retrieval;
pub mod runtime;
pub mod telemetry;

pub use naming::{NamingRouter, NamingRouterError, NamingRulePack, RoutingDecision};
pub use retrieval::{
    NoopRetrievalStage, QueryIntent, QueryPlan, RerankerHook, RetrievalCandidate, RetrievalError,
    RetrievalPipeline, RetrievalResult, RetrievalStage,
};
pub use runtime::{
    IngestStage, IngestStageConfigError, IngestStageOrder, IngestStageRuntime,
    NoopStageRollbackHook, RuntimeExecutionOutcome, StageCancellation, StageFailure,
    StageInputEnvelope, StageOutputEnvelope, StageResult, StageRollbackHook, StageTransition,
    StageTransitionState,
};
pub use telemetry::{
    FailureClass, PipelineTelemetryReport, StageDuration, StageFailureEvent, StageTimer,
};

const MAX_FILE_HOT_CACHE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileRecord {
    pub path: PathBuf,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CanonicalDocument {
    pub path: PathBuf,
    pub content_hash: String,
    pub dedup_reason: Option<String>,
}

pub trait HotCacheStore {
    fn put_hot_path(&self, record: &FileRecord) -> Result<()>;
}

pub trait DedupEngine {
    fn is_duplicate(&self, record: &FileRecord) -> Result<Option<String>>;
}

pub trait SnapshotStore {
    fn write_snapshot(&self, doc: &CanonicalDocument) -> Result<()>;
}

pub trait ColbertReranker {
    fn rerank(&self, doc: &CanonicalDocument) -> Result<()>;
}

pub trait LexicalAnalyzer {
    fn analyze(&self, doc: &CanonicalDocument) -> Result<()>;
}

pub trait VectorGraphMirror {
    fn mirror(&self, doc: &CanonicalDocument) -> Result<()>;
}

pub trait OptionalMultimodalEmbedder {
    fn embed_multimodal(&self, doc: &CanonicalDocument) -> Result<()>;
}

pub trait CanonicalDb {
    fn write_canonical(&self, doc: &CanonicalDocument) -> Result<()>;
    fn rollback_canonical(&self, doc: &CanonicalDocument) -> Result<()>;
}

pub struct Pipeline<'a> {
    pub hot_cache: &'a dyn HotCacheStore,
    pub dedup: &'a dyn DedupEngine,
    pub snapshots: &'a dyn SnapshotStore,
    pub colbert: &'a dyn ColbertReranker,
    pub lexical: &'a dyn LexicalAnalyzer,
    pub falkor: &'a dyn VectorGraphMirror,
    pub multimodal: &'a dyn OptionalMultimodalEmbedder,
    pub canonical_db: &'a dyn CanonicalDb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestExecutionContext {
    pub record: FileRecord,
    pub canonical_document: Option<CanonicalDocument>,
}

impl<'a> Pipeline<'a> {
    pub fn process_file(&self, path: &Path, bytes: &[u8]) -> Result<CanonicalDocument> {
        self.process_file_with_stage_order(path, bytes, None)
    }

    pub fn process_file_with_stage_order(
        &self,
        path: &Path,
        bytes: &[u8],
        stage_order: Option<IngestStageOrder>,
    ) -> Result<CanonicalDocument> {
        let cancellation = CancellationSource::new();
        self.process_file_with_stage_order_and_token(
            path,
            bytes,
            stage_order,
            &cancellation.token(),
        )
    }

    pub fn process_file_with_stage_order_and_token(
        &self,
        path: &Path,
        bytes: &[u8],
        stage_order: Option<IngestStageOrder>,
        cancellation: &CancellationToken,
    ) -> Result<CanonicalDocument> {
        let context = IngestExecutionContext {
            record: FileRecord {
                path: path.to_path_buf(),
                content_hash: sha256_hex(bytes),
            },
            canonical_document: None,
        };
        let runtime = self.ingest_runtime(stage_order);
        let adapter = PipelineStageAdapter { pipeline: self };
        let mut rollback = PipelineRollbackHook { pipeline: self };
        let outcome = runtime.execute_with_control(&adapter, context, cancellation, &mut rollback);
        let transitions = format_stage_transitions(&outcome.transitions);
        match outcome.result {
            StageResult::Success(ctx) => ctx.canonical_document.ok_or_else(|| {
                anyhow::anyhow!("pipeline runtime completed without canonical output")
            }),
            StageResult::RetryableFailure(err) => Err(anyhow::anyhow!(
                "retryable stage failure at {:?}: {} (transitions: {})",
                err.stage,
                err.message,
                transitions
            )),
            StageResult::NonRetryableFailure(err) => Err(anyhow::anyhow!(
                "non-retryable stage failure at {:?}: {} (transitions: {})",
                err.stage,
                err.message,
                transitions
            )),
            StageResult::Cancelled(cancelled) => Err(anyhow::anyhow!(
                "pipeline cancelled at {:?}: {} (transitions: {})",
                cancelled.stage,
                cancelled.reason,
                transitions
            )),
        }
    }

    pub fn process_directory_with_stage_order(
        &self,
        root: &Path,
        stage_order: Option<IngestStageOrder>,
    ) -> Result<PipelineStats> {
        let cancellation = CancellationSource::new();
        let out = self.process_directory_with_stage_order_and_token(
            root,
            stage_order,
            &cancellation.token(),
        )?;
        Ok(out.stats)
    }

    pub fn process_directory_with_stage_order_and_token(
        &self,
        root: &Path,
        stage_order: Option<IngestStageOrder>,
        cancellation: &CancellationToken,
    ) -> Result<PipelineDirectoryOutcome> {
        let stage_order = stage_order.unwrap_or_default();
        let mut stats = PipelineStats::default();
        for entry in WalkDir::new(root).follow_links(false) {
            if cancellation.is_cancelled() {
                return Ok(PipelineDirectoryOutcome {
                    stats,
                    cancelled_reason: cancellation.reason(),
                });
            }
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let bytes = fs::read(path)
                .with_context(|| format!("failed reading discovered file {}", path.display()))?;
            stats.discovered += 1;
            match self.process_file_with_stage_order_and_token(
                path,
                &bytes,
                Some(stage_order.clone()),
                cancellation,
            ) {
                Ok(_) => stats.ingested += 1,
                Err(err) => {
                    stats.skipped += 1;
                    stats.last_errors.push(err.to_string());
                }
            }
        }
        Ok(PipelineDirectoryOutcome {
            stats,
            cancelled_reason: None,
        })
    }

    pub fn process_directory(&self, root: &Path) -> Result<PipelineStats> {
        self.process_directory_with_stage_order(root, None)
    }

    pub fn ingest_runtime(&self, stage_order: Option<IngestStageOrder>) -> IngestStageRuntime {
        IngestStageRuntime::new(stage_order.unwrap_or_default())
    }
}

struct PipelineStageAdapter<'a> {
    pipeline: &'a Pipeline<'a>,
}

impl<'a> runtime::IngestStageExecutor for PipelineStageAdapter<'a> {
    type Context = IngestExecutionContext;

    fn execute_stage(
        &self,
        input: StageInputEnvelope<&mut Self::Context>,
    ) -> StageResult<StageOutputEnvelope<()>> {
        let stage = input.stage;
        let context = input.payload;
        let record = &context.record;
        let retryable_failure =
            |message: String| StageResult::RetryableFailure(StageFailure { stage, message });
        let non_retryable_failure =
            |message: String| StageResult::NonRetryableFailure(StageFailure { stage, message });

        let run = match stage {
            IngestStage::HotCache => self.pipeline.hot_cache.put_hot_path(record),
            IngestStage::Dedup => match self.pipeline.dedup.is_duplicate(record) {
                Ok(Some(reason)) => {
                    return StageResult::Cancelled(StageCancellation { stage, reason });
                }
                Ok(None) => Ok(()),
                Err(err) => return non_retryable_failure(err.to_string()),
            },
            IngestStage::CanonicalWrite => {
                let doc = context
                    .canonical_document
                    .get_or_insert_with(|| CanonicalDocument {
                        path: record.path.clone(),
                        content_hash: record.content_hash.clone(),
                        dedup_reason: None,
                    });
                self.pipeline.canonical_db.write_canonical(doc)
            }
            IngestStage::Snapshot => with_doc(context, stage, |doc| {
                self.pipeline.snapshots.write_snapshot(doc)
            }),
            IngestStage::ColbertRerank => {
                with_doc(context, stage, |doc| self.pipeline.colbert.rerank(doc))
            }
            IngestStage::LexicalAnalyze => {
                with_doc(context, stage, |doc| self.pipeline.lexical.analyze(doc))
            }
            IngestStage::MultimodalEmbed => with_doc(context, stage, |doc| {
                self.pipeline.multimodal.embed_multimodal(doc)
            }),
            IngestStage::FalkorMirror => {
                with_doc(context, stage, |doc| self.pipeline.falkor.mirror(doc))
            }
        };

        match run {
            Ok(()) => StageResult::Success(StageOutputEnvelope { stage, payload: () }),
            Err(err) => match stage {
                IngestStage::HotCache
                | IngestStage::CanonicalWrite
                | IngestStage::Snapshot
                | IngestStage::ColbertRerank
                | IngestStage::LexicalAnalyze
                | IngestStage::MultimodalEmbed
                | IngestStage::FalkorMirror => retryable_failure(err.to_string()),
                IngestStage::Dedup => non_retryable_failure(err.to_string()),
            },
        }
    }
}

fn with_doc(
    context: &IngestExecutionContext,
    stage: IngestStage,
    op: impl FnOnce(&CanonicalDocument) -> Result<()>,
) -> Result<()> {
    let doc = context.canonical_document.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "stage {:?} requires canonical document from canonical_write stage",
            stage
        )
    })?;
    op(doc)
}

fn format_stage_transitions(transitions: &[StageTransition]) -> String {
    transitions
        .iter()
        .map(|t| format!("{:?}:{:?}", t.stage, t.state))
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineStats {
    pub discovered: usize,
    pub ingested: usize,
    pub skipped: usize,
    pub last_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PipelineDirectoryOutcome {
    pub stats: PipelineStats,
    pub cancelled_reason: Option<String>,
}

struct PipelineRollbackHook<'a> {
    pipeline: &'a Pipeline<'a>,
}

impl<'a> runtime::StageRollbackHook<IngestExecutionContext> for PipelineRollbackHook<'a> {
    fn rollback_stage(
        &mut self,
        stage: IngestStage,
        context: &mut IngestExecutionContext,
    ) -> std::result::Result<(), StageFailure> {
        if stage == IngestStage::CanonicalWrite
            && let Some(doc) = context.canonical_document.as_ref()
        {
            self.pipeline
                .canonical_db
                .rollback_canonical(doc)
                .map_err(|err| StageFailure {
                    stage,
                    message: format!("rollback failed: {err}"),
                })?;
        }
        Ok(())
    }
}

pub struct ValkeyHotCacheStore {
    client: redis::Client,
    key_prefix: String,
    ttl_seconds: u64,
}

impl ValkeyHotCacheStore {
    pub fn new(redis_url: &str, key_prefix: impl Into<String>, ttl_seconds: u64) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let mut conn = client.get_connection()?;
        let _: String = redis::cmd("PING").query(&mut conn)?;
        Ok(Self {
            client,
            key_prefix: key_prefix.into(),
            ttl_seconds,
        })
    }
}

impl HotCacheStore for ValkeyHotCacheStore {
    fn put_hot_path(&self, record: &FileRecord) -> Result<()> {
        let mut conn = self.client.get_connection()?;
        let key = format!("{}:{}", self.key_prefix, record.content_hash);
        let value = record.path.to_string_lossy().to_string();
        let _: () = conn.set_ex(key, value, self.ttl_seconds)?;
        Ok(())
    }
}

pub struct FileHotCacheStore {
    cache_log: PathBuf,
}

impl FileHotCacheStore {
    pub fn new(cache_log: impl Into<PathBuf>) -> Self {
        Self {
            cache_log: cache_log.into(),
        }
    }
}

impl HotCacheStore for FileHotCacheStore {
    fn put_hot_path(&self, record: &FileRecord) -> Result<()> {
        if let Some(parent) = self.cache_log.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::metadata(&self.cache_log)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
            >= MAX_FILE_HOT_CACHE_BYTES
        {
            let rotated = self.cache_log.with_extension("jsonl.1");
            let _ = fs::remove_file(&rotated);
            fs::rename(&self.cache_log, rotated)?;
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&self.cache_log)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        let row = json!({
            "ts": Utc::now().to_rfc3339(),
            "path": record.path,
            "content_hash": record.content_hash,
            "mode": "offline_hot_cache"
        });
        writeln!(file, "{}", serde_json::to_string(&row)?)?;
        file.sync_all()?;
        Ok(())
    }
}

pub struct RigorousDedupEngine {
    seen_hashes: Mutex<HashSet<String>>,
}

impl RigorousDedupEngine {
    pub fn new() -> Self {
        Self {
            seen_hashes: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for RigorousDedupEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DedupEngine for RigorousDedupEngine {
    fn is_duplicate(&self, record: &FileRecord) -> Result<Option<String>> {
        let mut seen = self
            .seen_hashes
            .lock()
            .map_err(|_| anyhow::anyhow!("dedup mutex poisoned"))?;
        if seen.contains(&record.content_hash) {
            return Ok(Some("exact_hash_duplicate".to_string()));
        }
        seen.insert(record.content_hash.clone());
        Ok(None)
    }
}

pub struct LanceSnapshotStore {
    snapshot_log: PathBuf,
}

impl LanceSnapshotStore {
    pub fn new(snapshot_log: impl Into<PathBuf>) -> Self {
        Self {
            snapshot_log: snapshot_log.into(),
        }
    }
}

impl SnapshotStore for LanceSnapshotStore {
    fn write_snapshot(&self, doc: &CanonicalDocument) -> Result<()> {
        if let Some(parent) = self.snapshot_log.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.snapshot_log)?;
        let row = json!({
            "ts": Utc::now().to_rfc3339(),
            "path": doc.path,
            "content_hash": doc.content_hash,
            "engine": "lancedb_primary"
        });
        writeln!(file, "{}", serde_json::to_string(&row)?)?;
        Ok(())
    }
}

pub struct ColbertCpuRerankerQueue {
    queue_log: PathBuf,
}

impl ColbertCpuRerankerQueue {
    pub fn new(queue_log: impl Into<PathBuf>) -> Self {
        Self {
            queue_log: queue_log.into(),
        }
    }
}

impl ColbertReranker for ColbertCpuRerankerQueue {
    fn rerank(&self, doc: &CanonicalDocument) -> Result<()> {
        if let Some(parent) = self.queue_log.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.queue_log)?;
        let row = json!({
            "ts": Utc::now().to_rfc3339(),
            "path": doc.path,
            "content_hash": doc.content_hash,
            "model": "lfm2.5-colbert-250m",
            "device": "cpu"
        });
        writeln!(file, "{}", serde_json::to_string(&row)?)?;
        Ok(())
    }
}

pub struct MeilisearchQueueAnalyzer {
    queue_log: PathBuf,
}

impl MeilisearchQueueAnalyzer {
    pub fn new(queue_log: impl Into<PathBuf>) -> Self {
        Self {
            queue_log: queue_log.into(),
        }
    }
}

impl LexicalAnalyzer for MeilisearchQueueAnalyzer {
    fn analyze(&self, doc: &CanonicalDocument) -> Result<()> {
        if let Some(parent) = self.queue_log.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.queue_log)?;
        let row = json!({
            "ts": Utc::now().to_rfc3339(),
            "path": doc.path,
            "content_hash": doc.content_hash,
            "engine": "meilisearch_lexical_context"
        });
        writeln!(file, "{}", serde_json::to_string(&row)?)?;
        Ok(())
    }
}

pub struct FalkorMirrorQueue {
    queue_log: PathBuf,
}

impl FalkorMirrorQueue {
    pub fn new(queue_log: impl Into<PathBuf>) -> Self {
        Self {
            queue_log: queue_log.into(),
        }
    }
}

impl VectorGraphMirror for FalkorMirrorQueue {
    fn mirror(&self, doc: &CanonicalDocument) -> Result<()> {
        if let Some(parent) = self.queue_log.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.queue_log)?;
        let row = json!({
            "ts": Utc::now().to_rfc3339(),
            "path": doc.path,
            "content_hash": doc.content_hash,
            "engine": "falkor_vector_graph_mirror"
        });
        writeln!(file, "{}", serde_json::to_string(&row)?)?;
        Ok(())
    }
}

pub struct OptionalJinaOmniMultimodal {
    enabled: bool,
    queue_log: PathBuf,
}

impl OptionalJinaOmniMultimodal {
    pub fn new(enabled: bool, queue_log: impl Into<PathBuf>) -> Self {
        Self {
            enabled,
            queue_log: queue_log.into(),
        }
    }
}

impl OptionalMultimodalEmbedder for OptionalJinaOmniMultimodal {
    fn embed_multimodal(&self, doc: &CanonicalDocument) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        if let Some(parent) = self.queue_log.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.queue_log)?;
        let row = json!({
            "ts": Utc::now().to_rfc3339(),
            "path": doc.path,
            "content_hash": doc.content_hash,
            "model": "jina-embeddings-v5-omni-small",
            "mode": "optional_multimodal"
        });
        writeln!(file, "{}", serde_json::to_string(&row)?)?;
        Ok(())
    }
}

pub struct SqliteCanonicalDb {
    db_path: PathBuf,
}

impl SqliteCanonicalDb {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    fn init(&self, conn: &Connection) -> Result<()> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS masterd_documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                content_hash TEXT NOT NULL,
                ingested_at TEXT NOT NULL
            )",
            [],
        )?;
        Ok(())
    }
}

impl CanonicalDb for SqliteCanonicalDb {
    fn write_canonical(&self, doc: &CanonicalDocument) -> Result<()> {
        if let Some(parent) = self.db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&self.db_path)?;
        self.init(&conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO masterd_documents(path, content_hash, ingested_at)
             VALUES (?1, ?2, ?3)",
            params![
                doc.path.to_string_lossy().to_string(),
                doc.content_hash,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    fn rollback_canonical(&self, doc: &CanonicalDocument) -> Result<()> {
        if !self.db_path.exists() {
            return Ok(());
        }
        let conn = Connection::open(&self.db_path)?;
        self.init(&conn)?;
        conn.execute(
            "DELETE FROM masterd_documents WHERE path = ?1",
            params![doc.path.to_string_lossy().to_string()],
        )?;
        Ok(())
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{digest:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        std::env::current_dir()
            .expect("cwd")
            .join("target")
            .join("test-artifacts")
            .join("masterd-pipeline")
            .join(format!("{name}-{nonce}"))
    }

    #[test]
    fn file_hot_cache_rotates_and_syncs_append() {
        let dir = test_dir("hot-cache-rotation");
        fs::create_dir_all(&dir).expect("create test dir");
        let cache_log = dir.join("offline_hot_cache.jsonl");
        let oversized = fs::File::create(&cache_log).expect("create oversized cache");
        oversized
            .set_len(MAX_FILE_HOT_CACHE_BYTES + 1)
            .expect("make cache oversized");

        let store = FileHotCacheStore::new(&cache_log);
        store
            .put_hot_path(&FileRecord {
                path: PathBuf::from("/tmp/example.txt"),
                content_hash: "abc123".to_string(),
            })
            .expect("append hot-cache row");

        assert!(
            cache_log.with_extension("jsonl.1").exists(),
            "old cache should be rotated"
        );
        let raw = fs::read_to_string(&cache_log).expect("read new cache log");
        assert!(raw.contains("/tmp/example.txt"));
        assert!(raw.contains("abc123"));
        fs::remove_dir_all(&dir).expect("cleanup test dir");
    }
}
