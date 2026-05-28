use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

use masterd_chat_engine::{ChatEngine, ChatEngineConfig, ChatSession};
use masterd_data::{DataStore, DataStoreConfig};

// ── Persistent user configuration ────────────────────────────────────────────

/// All user-configurable settings. Persisted to `config/app-config.json`.
/// Loaded on startup; every Tauri command that respects these values reads from here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    /// Where exported archives and backups are stored.
    pub archive_path: Option<String>,
    /// Tesseract OCR language code (e.g. "eng", "spa").
    pub ocr_language: String,
    /// Confidence % below which items go to the review queue (40–99).
    pub safety_confidence_pct: u8,
    /// Model to use for classification: "instruct" | "thinking" | "auto".
    pub chat_model: String,
    /// SearXNG base URL for web search.
    pub searxng_url: String,
    /// How many BM25 candidates to retrieve before reranking.
    pub bm25_top_k: usize,
    /// How many RAG context chunks to inject into the prompt.
    pub rag_top_k: usize,
    /// Generation temperature (0.0–2.0).
    pub generation_temp: f64,
    /// Max new tokens per chat response.
    pub generation_max_tokens: usize,
    /// Embedding backend: "http" (external service) or "direct" (smoke-test only).
    pub embedding_backend: String,
    /// ColBERT wrapper service URL.
    pub colbert_url: String,
    /// Jina embedding service URL.
    pub jina_url: String,
    /// Maximum walk depth when scanning a watch folder.
    pub intake_max_depth: usize,
    /// File extensions to index (without leading dot, lowercase).
    pub intake_extensions: Vec<String>,
    /// Ollama daemon base URL — used as fallback when embedded models fail to load.
    pub ollama_url: String,
    /// Ollama model name to use (e.g. "llama3.2", "mistral", "phi3").
    pub ollama_model: String,
    pub preference_learning_enabled: bool,
    pub classification_learning_enabled: bool,
    pub entity_extraction_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            archive_path: None,
            ocr_language: "eng".into(),
            safety_confidence_pct: 85,
            chat_model: "auto".into(),
            searxng_url: "http://127.0.0.1:9265".into(),
            bm25_top_k: 8,
            rag_top_k: 8,
            generation_temp: 0.7,
            generation_max_tokens: 1024,
            embedding_backend: "http".into(),
            colbert_url: "http://127.0.0.1:11450".into(),
            jina_url: "http://127.0.0.1:11447".into(),
            intake_max_depth: 3,
            intake_extensions: vec![
                "txt".into(),
                "md".into(),
                "rst".into(),
                "log".into(),
                "pdf".into(),
            ],
            ollama_url: "http://127.0.0.1:11434".into(),
            ollama_model: "llama3.2".into(),
            preference_learning_enabled: true,
            classification_learning_enabled: true,
            entity_extraction_enabled: true,
        }
    }
}

const MAX_JSON_LOG_BYTES: u64 = 10 * 1024 * 1024;

pub type SessionMap = Arc<Mutex<HashMap<String, ChatSession>>>;

pub fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut tmp_path = path.to_path_buf();
    tmp_path.set_extension(format!(
        "{}.tmp.{}.{}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("json"),
        std::process::id(),
        nonce
    ));

    let write_result = (|| {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        std::fs::rename(&tmp_path, path)?;
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();

    if write_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }

    write_result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchFolderEntry {
    pub id: String,
    pub path: String,
    pub enabled: bool,
    pub profile_id: String,
    pub file_count: usize,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeQueueItem {
    pub id: String,
    pub file_name: String,
    pub path: String,
    pub extension: String,
    pub size_bytes: u64,
    pub status: String,
    pub progress: u8,
    pub duplicate_status: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// All runtime directories that are created on first launch.
pub struct AppDirs {
    /// ~/.local/share/com.masterd.desktop/
    pub data: PathBuf,
    /// ~/.local/share/com.masterd.desktop/index/
    pub index: PathBuf,
    /// ~/.local/share/com.masterd.desktop/intake/
    pub intake: PathBuf,
    /// ~/.local/share/com.masterd.desktop/watchers/
    pub watchers: PathBuf,
    /// ~/.local/share/com.masterd.desktop/models/
    pub models: PathBuf,
    /// ~/.local/share/com.masterd.desktop/data/
    pub user_data: PathBuf,
    /// ~/.config/com.masterd.desktop/
    pub config: PathBuf,
    /// ~/.cache/com.masterd.desktop/
    pub cache: PathBuf,
    /// ~/.local/state/com.masterd.desktop/logs/
    pub logs: PathBuf,
}

impl AppDirs {
    /// Create every directory that MASTERd needs.  Safe to call every launch —
    /// `create_dir_all` is a no-op when the directory already exists.
    pub fn create_all(
        data_dir: PathBuf,
        config_dir: PathBuf,
        cache_dir: PathBuf,
        log_dir: PathBuf,
    ) -> std::io::Result<Self> {
        let index = data_dir.join("index");
        let intake = data_dir.join("intake");
        let watchers = data_dir.join("watchers");
        let models = data_dir.join("models");
        let user_data = data_dir.join("data");

        for dir in &[
            &data_dir,
            &index,
            &intake,
            &watchers,
            &models,
            &user_data,
            &config_dir,
            &cache_dir,
            &log_dir,
        ] {
            std::fs::create_dir_all(dir)?;
        }

        let dirs = Self {
            data: data_dir,
            index,
            intake,
            watchers,
            models,
            user_data,
            config: config_dir,
            cache: cache_dir,
            logs: log_dir,
        };

        tracing::info!("MASTERd data:      {}", dirs.data.display());
        tracing::info!("MASTERd user data: {}", dirs.user_data.display());
        tracing::info!("MASTERd models:    {}", dirs.models.display());
        tracing::info!("MASTERd config:    {}", dirs.config.display());
        tracing::info!("MASTERd cache:     {}", dirs.cache.display());
        tracing::info!("MASTERd logs:      {}", dirs.logs.display());

        Ok(dirs)
    }

    pub fn watchers_json(&self) -> PathBuf {
        self.watchers.join("watch-folders.json")
    }
    pub fn index_snapshot_json(&self) -> PathBuf {
        self.index.join("snapshot.json")
    }
    pub fn intake_queue_json(&self) -> PathBuf {
        self.intake.join("queue.json")
    }
    pub fn config_json(&self) -> PathBuf {
        self.config.join("app-config.json")
    }
    pub fn event_log_jsonl(&self) -> PathBuf {
        self.logs.join("masterd-events.jsonl")
    }

    pub fn append_json_log(&self, value: &serde_json::Value) -> std::io::Result<()> {
        let path = self.event_log_jsonl();
        if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) >= MAX_JSON_LOG_BYTES {
            let rolled = self.logs.join(format!(
                "masterd-events-{}.jsonl",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or_default()
            ));
            std::fs::rename(&path, rolled)?;
        }

        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        Ok(())
    }
}

/// Shared application state injected into every Tauri command via `tauri::State`.
pub struct AppState {
    pub chat_engine: Arc<ChatEngine>,
    pub sessions: SessionMap,
    pub watch_folders: Arc<Mutex<Vec<WatchFolderEntry>>>,
    pub intake_queue: Arc<Mutex<Vec<IntakeQueueItem>>>,
    /// User-configurable settings, hot-reloadable at runtime.
    pub config: Arc<Mutex<AppConfig>>,
    /// Resolved at startup; None in unit-test contexts.
    pub dirs: std::sync::Mutex<Option<Arc<AppDirs>>>,
    /// Canonical SQLite-backed document/retrieval/preference store.
    pub data_store: std::sync::Mutex<Option<DataStore>>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        let dirs = self.dirs.lock().ok().and_then(|g| g.clone());
        let data_store = self.data_store.lock().ok().and_then(|g| g.clone());
        Self {
            chat_engine: self.chat_engine.clone(),
            sessions: self.sessions.clone(),
            watch_folders: self.watch_folders.clone(),
            intake_queue: self.intake_queue.clone(),
            config: self.config.clone(),
            dirs: std::sync::Mutex::new(dirs),
            data_store: std::sync::Mutex::new(data_store),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        let config = ChatEngineConfig::default();
        Self {
            chat_engine: Arc::new(ChatEngine::new(config)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            watch_folders: Arc::new(Mutex::new(Vec::new())),
            intake_queue: Arc::new(Mutex::new(Vec::new())),
            config: Arc::new(Mutex::new(AppConfig::default())),
            dirs: std::sync::Mutex::new(None),
            data_store: std::sync::Mutex::new(None),
        }
    }

    /// Attach resolved app dirs and restore persisted state from disk.
    pub async fn init_dirs(&self, dirs: AppDirs) {
        // Restore user config
        if let Ok(json) = std::fs::read_to_string(dirs.config_json()) {
            if let Ok(cfg) = serde_json::from_str::<AppConfig>(&json) {
                *self.config.lock().await = cfg;
                tracing::info!("Restored app config from disk");
            }
        }
        if let Ok(json) = std::fs::read_to_string(dirs.watchers_json()) {
            if let Ok(folders) = serde_json::from_str::<Vec<WatchFolderEntry>>(&json) {
                *self.watch_folders.lock().await = folders;
                tracing::info!("Restored watch folders from disk");
            }
        }

        if let Ok(json) = std::fs::read_to_string(dirs.intake_queue_json()) {
            if let Ok(items) = serde_json::from_str::<Vec<IntakeQueueItem>>(&json) {
                *self.intake_queue.lock().await = items;
                tracing::info!("Restored intake queue from disk");
            }
        }

        // Restore BM25 index snapshot
        if let Ok(json) = std::fs::read_to_string(dirs.index_snapshot_json()) {
            use masterd_index::IndexSnapshot;
            if let Ok(snapshot) = IndexSnapshot::from_json(&json) {
                self.chat_engine.restore_index(snapshot).await;
                tracing::info!("Restored BM25 index from disk");
            }
        }

        let restored_config = self.config.lock().await.clone();
        let mut data_config = DataStoreConfig::local(dirs.user_data.join("masterd.sqlite"));
        data_config.embedding_url = Some(restored_config.jina_url.clone());
        data_config.colbert_url = Some(restored_config.colbert_url.clone());
        let store_result = tokio::task::spawn_blocking(move || DataStore::open(data_config)).await;
        match store_result {
            Ok(Ok(store)) => {
                tracing::info!(
                    db = %store.db_path().display(),
                    searxng = %restored_config.searxng_url,
                    "Canonical MASTERd data store opened"
                );
                let backfill_store = store.clone();
                *self.data_store.lock().unwrap() = Some(store);
                std::thread::spawn(move || {
                    match backfill_store.backfill_model2vec_embeddings(128) {
                        Ok(count) if count > 0 => {
                            tracing::info!("Backfilled {count} model2vec embeddings")
                        }
                        Ok(_) => tracing::info!("model2vec embeddings already up to date"),
                        Err(err) => tracing::warn!("model2vec backfill failed: {err}"),
                    }
                });
            }
            Ok(Err(err)) => {
                tracing::error!("Failed to open canonical MASTERd data store: {err}");
            }
            Err(err) => {
                tracing::error!("Failed to join canonical MASTERd data store opener: {err}");
            }
        }

        *self.dirs.lock().unwrap() = Some(Arc::new(dirs));
    }

    /// Persist watch folders and index snapshot to disk.
    pub async fn persist(&self) {
        let dirs = {
            let dirs_guard = self.dirs.lock().unwrap();
            let Some(dirs) = dirs_guard.as_ref() else {
                return;
            };
            Arc::clone(dirs)
        };

        // Save user config
        let cfg = self.config.lock().await.clone();
        if let Ok(json) = serde_json::to_string_pretty(&cfg) {
            if let Err(e) = write_atomic(&dirs.config_json(), json.as_bytes()) {
                tracing::error!("Failed to save app config: {e}");
            }
        }

        // Save watch folders
        let folders = self.watch_folders.lock().await.clone();
        if let Ok(json) = serde_json::to_string_pretty(&folders) {
            if let Err(e) = write_atomic(&dirs.watchers_json(), json.as_bytes()) {
                tracing::error!("Failed to save watch folders: {e}");
            }
        }

        let intake_queue = self.intake_queue.lock().await.clone();
        if let Ok(json) = serde_json::to_string_pretty(&intake_queue) {
            if let Err(e) = write_atomic(&dirs.intake_queue_json(), json.as_bytes()) {
                tracing::error!("Failed to save intake queue: {e}");
            }
        }

        // Save BM25 index snapshot
        let snapshot = self.chat_engine.snapshot_index().await;
        if let Ok(json) = snapshot.to_json() {
            if let Err(e) = write_atomic(&dirs.index_snapshot_json(), json.as_bytes()) {
                tracing::error!("Failed to save index snapshot: {e}");
            }
        }

        tracing::info!("MASTERd state persisted to disk");
        let _ = dirs.append_json_log(&serde_json::json!({
            "event": "state_persisted",
            "ts": chrono_like_now(),
            "watchFolders": folders.len(),
            "intakeItems": intake_queue.len()
        }));
    }
}

fn chrono_like_now() -> String {
    Local::now().to_rfc3339()
}
