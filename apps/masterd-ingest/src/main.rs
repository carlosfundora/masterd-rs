use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clap::Parser;
use masterd_embed_engine::{EmbeddedEngine, LocalEmbeddingStack};
use masterd_pipeline::{
    ColbertCpuRerankerQueue, FalkorMirrorQueue, FileHotCacheStore, HotCacheStore, IngestStage,
    IngestStageOrder, LanceSnapshotStore, MeilisearchQueueAnalyzer, OptionalJinaOmniMultimodal,
    Pipeline, PipelineStats, RigorousDedupEngine, SqliteCanonicalDb, ValkeyHotCacheStore,
    sha256_hex,
};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

#[derive(Debug, Parser)]
#[command(name = "masterd-ingest")]
#[command(about = "Run MASTERd hash->cache->dedup->index pipeline")]
struct Args {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long, default_value = "config/pipeline.toml")]
    pipeline_config: PathBuf,
    #[arg(long, default_value = "redis://127.0.0.1:6379/")]
    valkey_url: String,
    #[arg(long, default_value_t = false)]
    allow_offline_hot_cache: bool,
    #[arg(long, default_value_t = true)]
    verify_engine: bool,
    #[arg(long, default_value_t = true)]
    benchmark_engine: bool,
}

#[derive(Debug, Deserialize)]
struct PipelineConfig {
    database: DatabaseConfig,
    embeddings: EmbeddingsConfig,
    runtime: Option<RuntimeConfig>,
}

#[derive(Debug, Deserialize)]
struct DatabaseConfig {
    path: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingsConfig {
    multimodal_optional: bool,
}

#[derive(Debug, Deserialize)]
struct RuntimeConfig {
    stage_order: Option<Vec<String>>,
    hash_index_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HashIndexState {
    hashes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HashGateDecision {
    New,
    Duplicate,
}

#[derive(Debug, Clone)]
struct AtomicHashIndexService {
    index_path: PathBuf,
    lock_path: PathBuf,
    lock_timeout: Duration,
    stale_lock_timeout: Duration,
    lock_retry_interval: Duration,
}

impl AtomicHashIndexService {
    fn new(index_path: impl Into<PathBuf>) -> Self {
        let index_path = index_path.into();
        let lock_path = PathBuf::from(format!("{}.lock", index_path.to_string_lossy()));
        Self {
            index_path,
            lock_path,
            lock_timeout: Duration::from_secs(10),
            stale_lock_timeout: Duration::from_secs(300),
            lock_retry_interval: Duration::from_millis(25),
        }
    }

    fn register_hash(&self, content_hash: &str) -> Result<HashGateDecision> {
        let _guard = self.acquire_lock()?;
        let mut index = self.read_index_state()?;
        if index.hashes.iter().any(|existing| existing == content_hash) {
            return Ok(HashGateDecision::Duplicate);
        }
        index.hashes.push(content_hash.to_string());
        index.hashes.sort();
        index.hashes.dedup();
        self.persist_index_state(&index)?;
        Ok(HashGateDecision::New)
    }

    fn remove_hash(&self, content_hash: &str) -> Result<()> {
        let _guard = self.acquire_lock()?;
        let mut index = self.read_index_state()?;
        let before = index.hashes.len();
        index.hashes.retain(|existing| existing != content_hash);
        if index.hashes.len() != before {
            self.persist_index_state(&index)?;
        }
        Ok(())
    }

    fn acquire_lock(&self) -> Result<HashIndexLockGuard> {
        if let Some(parent) = self.lock_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed creating hash-index lock dir {}", parent.display())
            })?;
        }
        let start = SystemTime::now();
        let mut attempt = 0u32;
        loop {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&self.lock_path) {
                Ok(mut file) => {
                    writeln!(file, "pid={}", std::process::id())?;
                    writeln!(
                        file,
                        "acquired_unix_ms={}",
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis())
                            .unwrap_or(0)
                    )?;
                    file.sync_all()?;
                    return Ok(HashIndexLockGuard {
                        lock_path: self.lock_path.clone(),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    let waited = start.elapsed().unwrap_or_default();
                    if waited >= self.lock_timeout {
                        if self.lock_is_stale()? {
                            let _ = std::fs::remove_file(&self.lock_path);
                            continue;
                        }
                        return Err(anyhow::anyhow!(
                            "timed out waiting for hash-index lock {} after {:?}",
                            self.lock_path.display(),
                            waited
                        ));
                    }
                    let base_ms = self.lock_retry_interval.as_millis() as u64;
                    let multiplier = 1_u64.checked_shl(attempt.min(5)).unwrap_or(32);
                    let backoff = std::cmp::min(100, base_ms.saturating_mul(multiplier));
                    thread::sleep(Duration::from_millis(backoff));
                    attempt = attempt.saturating_add(1);
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!(
                            "failed to create hash-index lock file {}",
                            self.lock_path.display()
                        )
                    });
                }
            }
        }
    }

    fn lock_is_stale(&self) -> Result<bool> {
        let raw = fs::read_to_string(&self.lock_path).unwrap_or_default();
        if let Some(pid) = raw.lines().find_map(|line| line.strip_prefix("pid=")) {
            if process_is_alive(pid.trim()) {
                return Ok(false);
            }
        }
        let metadata = fs::metadata(&self.lock_path).with_context(|| {
            format!("failed reading lock metadata {}", self.lock_path.display())
        })?;
        let modified = metadata
            .modified()
            .with_context(|| format!("failed reading lock mtime {}", self.lock_path.display()))?;
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default();
        Ok(age > self.stale_lock_timeout)
    }

    fn read_index_state(&self) -> Result<HashIndexState> {
        if !self.index_path.exists() {
            return Ok(HashIndexState::default());
        }
        let raw = fs::read_to_string(&self.index_path)
            .with_context(|| format!("failed reading hash-index {}", self.index_path.display()))?;
        if raw.trim().is_empty() {
            return Ok(HashIndexState::default());
        }
        let mut state: HashIndexState = serde_json::from_str(&raw)
            .with_context(|| format!("failed parsing hash-index {}", self.index_path.display()))?;
        state.hashes.sort();
        state.hashes.dedup();
        Ok(state)
    }

    fn persist_index_state(&self, state: &HashIndexState) -> Result<()> {
        if let Some(parent) = self.index_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed creating hash-index dir {}", parent.display()))?;
        }
        let mut normalized = state.clone();
        normalized.hashes.sort();
        normalized.hashes.dedup();
        let payload = serde_json::to_vec_pretty(&normalized)?;
        let mut tmp_path = self.index_path.clone();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        tmp_path.set_extension(format!(
            "{}.tmp.{}.{}",
            self.index_path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("index"),
            std::process::id(),
            nonce
        ));
        {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&tmp_path).with_context(|| {
                format!("failed opening temp hash-index {}", tmp_path.display())
            })?;
            file.write_all(&payload).with_context(|| {
                format!("failed writing temp hash-index {}", tmp_path.display())
            })?;
            file.sync_all().with_context(|| {
                format!("failed syncing temp hash-index {}", tmp_path.display())
            })?;
        }
        fs::rename(&tmp_path, &self.index_path).with_context(|| {
            format!(
                "failed replacing hash-index {} with {}",
                self.index_path.display(),
                tmp_path.display()
            )
        })?;
        if let Some(parent) = self.index_path.parent() {
            OpenOptions::new()
                .read(true)
                .open(parent)
                .with_context(|| format!("failed opening hash-index dir {}", parent.display()))?
                .sync_all()
                .with_context(|| format!("failed syncing hash-index dir {}", parent.display()))?;
        }
        Ok(())
    }
}

fn process_is_alive(pid: &str) -> bool {
    !pid.is_empty()
        && pid.chars().all(|ch| ch.is_ascii_digit())
        && Path::new("/proc").join(pid).exists()
}

#[derive(Debug)]
struct HashIndexLockGuard {
    lock_path: PathBuf,
}

impl Drop for HashIndexLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock_path);
    }
}

fn process_directory_with_hash_index(
    pipeline: &Pipeline<'_>,
    root: &Path,
    stage_order: Option<IngestStageOrder>,
    hash_index: &AtomicHashIndexService,
) -> Result<PipelineStats> {
    let stage_order = stage_order.unwrap_or_default();
    let mut stats = PipelineStats::default();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let bytes = fs::read(path)
            .with_context(|| format!("failed reading discovered file {}", path.display()))?;
        stats.discovered += 1;
        let content_hash = sha256_hex(&bytes);
        match hash_index.register_hash(&content_hash).with_context(|| {
            format!(
                "hash-index gate failed for {} (hash={content_hash})",
                path.display()
            )
        })? {
            HashGateDecision::Duplicate => {
                stats.skipped += 1;
                stats
                    .last_errors
                    .push(format!("duplicate hash gate skipped {}", path.display()));
                continue;
            }
            HashGateDecision::New => {}
        }
        match pipeline.process_file_with_stage_order(path, &bytes, Some(stage_order.clone())) {
            Ok(_) => stats.ingested += 1,
            Err(err) => {
                stats.skipped += 1;
                stats.last_errors.push(err.to_string());
                hash_index.remove_hash(&content_hash).with_context(|| {
                    format!(
                        "failed rolling back hash-index entry for {} (hash={content_hash})",
                        path.display()
                    )
                })?;
            }
        }
    }
    Ok(stats)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let cfg_raw = fs::read_to_string(&args.pipeline_config)?;
    let cfg: PipelineConfig = toml::from_str(&cfg_raw)?;

    let hot_cache: Box<dyn HotCacheStore> =
        match ValkeyHotCacheStore::new(&args.valkey_url, "masterd:hot", 86_400) {
            Ok(cache) => Box::new(cache),
            Err(err) if args.allow_offline_hot_cache => {
                eprintln!("valkey unavailable, using explicit file-backed hot-cache: {err}");
                Box::new(FileHotCacheStore::new("data/offline_hot_cache.jsonl"))
            }
            Err(err) => return Err(err),
        };
    let dedup = RigorousDedupEngine::new();
    let snapshots = LanceSnapshotStore::new("data/lancedb_snapshots.jsonl");
    let colbert = ColbertCpuRerankerQueue::new("data/colbert_rerank_queue.jsonl");
    let lexical = MeilisearchQueueAnalyzer::new("data/meilisearch_queue.jsonl");
    let falkor = FalkorMirrorQueue::new("data/falkor_queue.jsonl");
    let multimodal = OptionalJinaOmniMultimodal::new(
        cfg.embeddings.multimodal_optional,
        "data/jina_omni_queue.jsonl",
    );
    let canonical_db = SqliteCanonicalDb::new(&cfg.database.path);

    let pipeline = Pipeline {
        hot_cache: &*hot_cache,
        dedup: &dedup,
        snapshots: &snapshots,
        colbert: &colbert,
        lexical: &lexical,
        falkor: &falkor,
        multimodal: &multimodal,
        canonical_db: &canonical_db,
    };
    let configured_stage_order = cfg
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.stage_order.clone())
        .map(|names| {
            let stages = names
                .into_iter()
                .map(|name| IngestStage::from_str(&name))
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, anyhow::Error>(IngestStageOrder::from_config(stages))
        })
        .transpose()?;
    let hash_index_path = cfg
        .runtime
        .as_ref()
        .and_then(|runtime| runtime.hash_index_path.as_ref())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/ingest_hash_index.json"));
    let hash_index = AtomicHashIndexService::new(hash_index_path);

    let stats = process_directory_with_hash_index(
        &pipeline,
        &args.root,
        configured_stage_order,
        &hash_index,
    )?;
    println!(
        "MASTERd ingest done: discovered={}, ingested={}, skipped={}",
        stats.discovered, stats.ingested, stats.skipped
    );
    for err in stats.last_errors.iter().take(5) {
        println!("skip: {err}");
    }

    if args.verify_engine {
        let engine = EmbeddedEngine::new(LocalEmbeddingStack::from_env())?;
        let engine_ready = match engine.health_check() {
            Ok(()) => {
                println!(
                    "embedded engine health: OK (backend={:?})",
                    engine.cfg.backend
                );
                let sample = vec![
                    "MASTERd retrieval smoke test text for embeddings".to_string(),
                    "Second sample text for batch and mean-pool check".to_string(),
                ];
                let embeds = engine.embed_jina_fast(&sample)?;
                println!(
                    "jina embed smoke: vectors={} dim={}",
                    embeds.len(),
                    embeds.first().map(|v| v.len()).unwrap_or(0)
                );
                let matrix_hash = EmbeddedEngine::token_matrix_hash(&embeds);
                let pooled_dim = EmbeddedEngine::mean_pool_embedding(&embeds)
                    .map(|v| v.len())
                    .unwrap_or(0);
                println!("jina token-matrix hash: {matrix_hash} (mean_pool_dim={pooled_dim})");
                let rerank = engine.rerank_colbert_topk(
                    "MASTERd retrieval query",
                    &[
                        "first retrieval candidate".to_string(),
                        "second retrieval candidate".to_string(),
                        "third retrieval candidate with cache semantics".to_string(),
                    ],
                    2,
                )?;
                println!("colbert rerank smoke: topk_results={}", rerank.len());
                true
            }
            Err(err) => {
                println!("embedded engine health: FAILED ({err})");
                false
            }
        };

        if args.benchmark_engine && engine_ready {
            let bench = engine.bench_embed_jina(
                "benchmark text payload for local embedding throughput measurement",
                16,
            )?;
            fs::create_dir_all("data")?;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("data/engine_benchmarks.jsonl")?;
            writeln!(file, "{}", serde_json::to_string(&bench)?)?;
            println!(
                "engine benchmark: {} ms, est {:.2} t/s",
                bench.elapsed_ms, bench.estimated_tokens_per_sec
            );
        } else if args.benchmark_engine {
            println!("engine benchmark skipped: live embedding endpoints are not reachable");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn test_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::current_dir()
            .expect("cwd")
            .join("target")
            .join("test-artifacts")
            .join("masterd-ingest")
            .join(format!("{name}-{nonce}"))
    }

    #[test]
    fn hash_gate_dedups_existing_hash() {
        let dir = test_dir("dedup");
        fs::create_dir_all(&dir).expect("create test dir");
        let index_path = dir.join("ingest_hash_index.json");
        let service = AtomicHashIndexService::new(&index_path);

        assert_eq!(
            service.register_hash("hash-1").expect("first insert"),
            HashGateDecision::New
        );
        assert_eq!(
            service.register_hash("hash-1").expect("second insert"),
            HashGateDecision::Duplicate
        );

        let raw = fs::read_to_string(&index_path).expect("read index");
        let state: HashIndexState = serde_json::from_str(&raw).expect("parse index");
        assert_eq!(state.hashes, vec!["hash-1".to_string()]);
        fs::remove_dir_all(&dir).expect("cleanup test dir");
    }

    #[test]
    fn hash_index_persist_is_atomic_and_cleans_lock_file() {
        let dir = test_dir("atomic");
        fs::create_dir_all(&dir).expect("create test dir");
        let index_path = dir.join("ingest_hash_index.json");
        let lock_path = PathBuf::from(format!("{}.lock", index_path.to_string_lossy()));
        let service = AtomicHashIndexService::new(&index_path);

        service.register_hash("hash-a").expect("insert hash-a");
        service.register_hash("hash-b").expect("insert hash-b");

        assert!(index_path.exists(), "index file should exist");
        assert!(!lock_path.exists(), "lock file should be released");
        let dir_entries: Vec<String> = fs::read_dir(&dir)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            !dir_entries.iter().any(|name| name.contains(".tmp.")),
            "temporary file should not remain after atomic rename"
        );
        fs::remove_dir_all(&dir).expect("cleanup test dir");
    }

    #[test]
    fn hash_index_handles_parallel_registration_without_corruption() {
        let dir = test_dir("parallel");
        fs::create_dir_all(&dir).expect("create test dir");
        let index_path = dir.join("ingest_hash_index.json");
        let service = Arc::new(AtomicHashIndexService::new(&index_path));
        let inputs = [
            "hash-1", "hash-2", "hash-3", "hash-1", "hash-2", "hash-4", "hash-5", "hash-3",
        ];

        let mut handles = Vec::new();
        for hash in inputs {
            let svc = Arc::clone(&service);
            let hash = hash.to_string();
            handles.push(thread::spawn(move || svc.register_hash(&hash)));
        }
        for handle in handles {
            handle
                .join()
                .expect("join thread")
                .expect("register hash result");
        }

        let raw = fs::read_to_string(&index_path).expect("read index");
        let state: HashIndexState = serde_json::from_str(&raw).expect("parse index");
        assert_eq!(
            state.hashes,
            vec![
                "hash-1".to_string(),
                "hash-2".to_string(),
                "hash-3".to_string(),
                "hash-4".to_string(),
                "hash-5".to_string()
            ]
        );
        fs::remove_dir_all(&dir).expect("cleanup test dir");
    }

    #[test]
    fn active_pid_lock_file_is_not_removed() {
        let dir = test_dir("active-pid-lock");
        fs::create_dir_all(&dir).expect("create test dir");
        let index_path = dir.join("ingest_hash_index.json");
        let mut service = AtomicHashIndexService::new(&index_path);
        service.lock_timeout = Duration::from_millis(3);
        service.lock_retry_interval = Duration::from_millis(1);
        service.stale_lock_timeout = Duration::from_millis(0);
        fs::write(
            &service.lock_path,
            format!("pid={}\nacquired_unix_ms=0\n", std::process::id()),
        )
        .expect("write active lock");

        let err = service
            .acquire_lock()
            .expect_err("active lock should time out");
        assert!(
            err.to_string()
                .contains("timed out waiting for hash-index lock"),
            "unexpected error: {err}"
        );
        assert!(
            service.lock_path.exists(),
            "active PID lock must not be removed"
        );
        fs::remove_dir_all(&dir).expect("cleanup test dir");
    }

    #[test]
    fn stale_dead_pid_lock_file_is_recovered() {
        let dir = test_dir("dead-pid-lock");
        fs::create_dir_all(&dir).expect("create test dir");
        let index_path = dir.join("ingest_hash_index.json");
        let mut service = AtomicHashIndexService::new(&index_path);
        service.lock_timeout = Duration::from_millis(3);
        service.lock_retry_interval = Duration::from_millis(1);
        service.stale_lock_timeout = Duration::from_millis(0);
        fs::write(&service.lock_path, "pid=2147483647\nacquired_unix_ms=0\n")
            .expect("write dead lock");
        thread::sleep(Duration::from_millis(2));

        assert_eq!(
            service
                .register_hash("hash-recovered")
                .expect("register after stale lock"),
            HashGateDecision::New
        );
        assert!(
            !service.lock_path.exists(),
            "lock should be released after recovery"
        );
        let raw = fs::read_to_string(&index_path).expect("read index");
        let state: HashIndexState = serde_json::from_str(&raw).expect("parse index");
        assert_eq!(state.hashes, vec!["hash-recovered".to_string()]);
        fs::remove_dir_all(&dir).expect("cleanup test dir");
    }
}
