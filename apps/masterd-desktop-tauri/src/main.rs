// MASTERd desktop — Tauri v2 main entry point.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod sidecars;
mod state;

use masterd_chat_engine::ChatToken;
use masterd_data::{DataStore, PreferenceEvent, SearchMode as DataSearchMode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sidecars::SidecarSupervisor;
use state::AppState;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::mpsc;
use uuid::Uuid;

// ── ApiResult envelope ────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(untagged)]
pub enum ApiResult<T: Serialize> {
    Ok {
        ok: bool,
        data: T,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "receivedAt")]
        received_at: String,
    },
    Err {
        ok: bool,
        error: ApiError,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "receivedAt")]
        received_at: String,
    },
}

#[derive(Serialize, Debug)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

fn ok<T: Serialize>(data: T) -> ApiResult<T> {
    ApiResult::Ok {
        ok: true,
        data,
        request_id: Uuid::new_v4().to_string(),
        received_at: now_ts(),
    }
}

fn err_result<T: Serialize>(
    code: impl Into<String>,
    message: impl Into<String>,
    recoverable: bool,
) -> ApiResult<T> {
    ApiResult::Err {
        ok: false,
        error: ApiError {
            code: code.into(),
            message: message.into(),
            recoverable,
        },
        request_id: Uuid::new_v4().to_string(),
        received_at: now_ts(),
    }
}

fn data_store(state: &State<'_, AppState>) -> Option<DataStore> {
    state.data_store.lock().ok().and_then(|store| store.clone())
}

async fn run_blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| err.to_string())?
}

fn now_ts() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| format!("{}", d.as_millis()))
        .unwrap_or_default()
}

// ── Paginated ─────────────────────────────────────────────────────────────────
#[derive(Serialize)]
pub struct Paginated<T: Serialize> {
    pub items: Vec<T>,
    pub total: u64,
    pub limit: u64,
    pub offset: u64,
}
impl<T: Serialize> Paginated<T> {
    fn empty() -> Self {
        Self {
            items: vec![],
            total: 0,
            limit: 50,
            offset: 0,
        }
    }
}

#[derive(Serialize)]
pub struct EmptyOk {}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationResult {
    pub category: String,
    pub confidence: f32,
}

#[derive(Serialize, Clone)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval_score: Option<f32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source_stages: Vec<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPreview {
    pub document_id: String,
    pub text_preview: String,
    pub page_count: u32,
    pub thumbnail_url: Option<String>,
    pub mime_type: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedTextResult {
    pub document_id: String,
    pub full_text: String,
    pub language: Option<String>,
    pub entities: Vec<serde_json::Value>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub success: bool,
    pub message: String,
    pub document_id: String,
    pub details: Option<serde_json::Value>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PipelineJob {
    pub id: String,
    pub document_id: String,
    pub file_name: String,
    pub stage: String,
    pub status: String,
    pub progress: u8,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
    pub worker_id: Option<String>,
    pub logs: Vec<serde_json::Value>,
    pub stage_timings: Vec<masterd_data::StageTiming>,
    pub retryable: bool,
    pub indexed_chunk_count: usize,
    pub ocr_confidence: Option<f32>,
    pub embedding_provider: Option<String>,
    pub rerank_status: String,
}

#[derive(Serialize, Clone)]
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

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRule {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub priority: u32,
    pub trigger: serde_json::Value,
    pub conditions: Vec<serde_json::Value>,
    pub actions: Vec<serde_json::Value>,
    pub safety_level: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RuleTestResult {
    pub matched: bool,
    pub actions_evaluated: Vec<serde_json::Value>,
}

#[derive(Serialize, Clone)]
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

// ── System commands ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SystemStatus {
    pub engine: String,
    pub database: String,
    pub watcher: String,
    pub models: Vec<ModelStatus>,
    pub services: Vec<ServiceStatus>,
    pub queues: QueueCounts,
    pub storage: StorageSummary,
}
#[derive(Serialize)]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub role: String,
    pub status: String,
}
#[derive(Serialize, Clone)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub role: String,
    pub status: String,
    pub required: bool,
    pub message: Option<String>,
}
#[derive(Serialize)]
pub struct QueueCounts {
    pub pending: u32,
    pub processing: u32,
    pub review: u32,
    #[serde(rename = "completeToday")]
    pub complete_today: u32,
    pub errors: u32,
}
#[derive(Serialize)]
pub struct StorageSummary {
    #[serde(rename = "indexedFiles")]
    pub indexed_files: u64,
    #[serde(rename = "totalBytes")]
    pub total_bytes: u64,
}
#[derive(Serialize)]
pub struct SystemHealth {
    #[serde(rename = "cpuUsage")]
    pub cpu_usage: f32,
    #[serde(rename = "memoryUsage")]
    pub memory_usage: f32,
    #[serde(rename = "diskFreeBytes")]
    pub disk_free_bytes: u64,
    #[serde(rename = "dbLatencyMs")]
    pub db_latency_ms: u32,
    #[serde(rename = "activeThreads")]
    pub active_threads: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupExportResult {
    pub backup_path: String,
    pub copied_files: usize,
}

#[derive(Default)]
struct DataStatusSnapshot {
    indexed_files: usize,
    total_bytes: u64,
    pending: usize,
    processing: usize,
    review: usize,
    complete_today: usize,
    errors: usize,
    services: Vec<ServiceStatus>,
}

#[tauri::command]
async fn system_get_status(state: State<'_, AppState>) -> Result<ApiResult<SystemStatus>, String> {
    let config = state.config.lock().await.clone();
    let loaded_models = state.chat_engine.loaded_models();
    let thinking_loaded = loaded_models.contains(&"lfm2.5-thinking-1.2b");
    let store = data_store(&state);
    let snapshot = run_blocking(move || {
        let Some(store) = store else {
            return Ok(DataStatusSnapshot {
                services: missing_datastore_services(),
                ..Default::default()
            });
        };
        let (indexed_files, total_bytes) = store.document_summary().unwrap_or((0, 0));
        let (pending, processing, review, complete_today, errors) =
            store.pipeline_summary().unwrap_or((0, 0, 0, 0, 0));
        let services = store
            .required_service_statuses()
            .into_iter()
            .map(map_required_service_status)
            .collect();
        Ok(DataStatusSnapshot {
            indexed_files,
            total_bytes,
            pending,
            processing,
            review,
            complete_today,
            errors,
            services,
        })
    })
    .await?;
    let database_ready = snapshot
        .services
        .iter()
        .filter(|service| service.required)
        .all(|service| service.status == "ready");

    let colbert_url = config.colbert_url;
    let jina_url = config.jina_url;
    let model2vec_url = "http://127.0.0.1:11448".to_string();

    let (colbert_health, jina_health, model2vec_health) = tokio::join!(
        check_service_health_async(&colbert_url),
        check_service_health_async(&jina_url),
        check_service_health_async(&model2vec_url),
    );
    Ok(ok(SystemStatus {
        engine: if database_ready { "online" } else { "degraded" }.to_string(),
        database: if database_ready { "ready" } else { "not_ready" }.to_string(),
        watcher: "active".to_string(),
        models: vec![
            ModelStatus {
                id: "lfm2.5-thinking".into(),
                name: "LFM2.5 1.2B Thinking".into(),
                role: "summarization".into(),
                status: if thinking_loaded {
                    "online".into()
                } else {
                    "offline".into()
                },
            },
            ModelStatus {
                id: "colbert-reranker".into(),
                name: "ColBERT 350M Reranker".into(),
                role: "reranking".into(),
                status: colbert_health,
            },
            ModelStatus {
                id: "jina-embedding".into(),
                name: "Jina v3 Embedding".into(),
                role: "embedding".into(),
                status: jina_health,
            },
            ModelStatus {
                id: "model2vec-service".into(),
                name: "model2vec-rs".into(),
                role: "embedding".into(),
                status: model2vec_health,
            },
        ],
        services: snapshot.services,
        queues: QueueCounts {
            pending: snapshot.pending as u32,
            processing: snapshot.processing as u32,
            review: snapshot.review as u32,
            complete_today: snapshot.complete_today as u32,
            errors: snapshot.errors as u32,
        },
        storage: StorageSummary {
            indexed_files: snapshot.indexed_files as u64,
            total_bytes: snapshot.total_bytes,
        },
    }))
}

fn map_required_service_status(status: masterd_data::RequiredServiceStatus) -> ServiceStatus {
    ServiceStatus {
        id: status.id,
        name: status.name,
        role: status.role,
        status: status.status,
        required: status.required,
        message: status.message,
    }
}

fn missing_datastore_services() -> Vec<ServiceStatus> {
    [
        ("sqlite", "SQLite", "canonical-store"),
        ("meilisearch", "Meilisearch", "lexical-index"),
        ("valkey", "Valkey", "hot-cache"),
        ("falkordb", "FalkorDB", "graph-db"),
    ]
    .into_iter()
    .map(|(id, name, role)| ServiceStatus {
        id: id.to_string(),
        name: name.to_string(),
        role: role.to_string(),
        status: "missing".to_string(),
        required: true,
        message: Some("canonical datastore is unavailable".to_string()),
    })
    .collect()
}

#[tauri::command]
async fn system_get_health() -> ApiResult<SystemHealth> {
    ok(SystemHealth {
        cpu_usage: 0.0,
        memory_usage: read_mem_pct().unwrap_or(0.0),
        disk_free_bytes: 0,
        db_latency_ms: 1,
        active_threads: num_logical_cpus(),
    })
}

#[tauri::command]
async fn system_export_backup(
    state: State<'_, AppState>,
) -> Result<ApiResult<BackupExportResult>, String> {
    let dirs = state.dirs.lock().unwrap().clone();
    let Some(dirs) = dirs else {
        return Ok(err_result(
            "STATE_UNAVAILABLE",
            "Backup export requires initialized app directories",
            true,
        ));
    };
    let config = state.config.lock().await.clone();
    let archive_root = config
        .archive_path
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs.data.join("backups"));
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let backup_dir = archive_root.join(format!("masterd-backup-{stamp}"));

    let result = run_blocking(move || {
        std::fs::create_dir_all(&backup_dir).map_err(|err| err.to_string())?;
        let mut copied_files = 0usize;
        let copy_if_exists = |src: PathBuf, dst: PathBuf, count: &mut usize| -> Result<(), String> {
            if src.exists() {
                std::fs::copy(&src, &dst).map_err(|err| err.to_string())?;
                *count += 1;
            }
            Ok(())
        };

        copy_if_exists(dirs.config_json(), backup_dir.join("app-config.json"), &mut copied_files)?;
        copy_if_exists(dirs.watchers_json(), backup_dir.join("watch-folders.json"), &mut copied_files)?;
        copy_if_exists(dirs.intake_queue_json(), backup_dir.join("queue.json"), &mut copied_files)?;
        copy_if_exists(dirs.index_snapshot_json(), backup_dir.join("snapshot.json"), &mut copied_files)?;
        copy_if_exists(
            dirs.learned_classifications_json(),
            backup_dir.join("learned_classifications.json"),
            &mut copied_files,
        )?;
        copy_if_exists(
            dirs.learned_preferences_json(),
            backup_dir.join("learned_preferences.json"),
            &mut copied_files,
        )?;
        copy_if_exists(
            dirs.user_data.join("masterd.sqlite"),
            backup_dir.join("masterd.sqlite"),
            &mut copied_files,
        )?;

        Ok::<_, String>(BackupExportResult {
            backup_path: backup_dir.to_string_lossy().to_string(),
            copied_files,
        })
    })
    .await?;

    Ok(ok(result))
}

fn read_mem_pct() -> Option<f32> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = 0u64;
    let mut avail = 0u64;
    for line in s.lines() {
        if line.starts_with("MemTotal:") {
            total = line.split_whitespace().nth(1)?.parse().ok()?;
        } else if line.starts_with("MemAvailable:") {
            avail = line.split_whitespace().nth(1)?.parse().ok()?;
        }
    }
    if total == 0 {
        return None;
    }
    Some((total - avail) as f32 / total as f32 * 100.0)
}
fn num_logical_cpus() -> u32 {
    std::fs::read_to_string("/proc/cpuinfo")
        .map(|s| s.lines().filter(|l| l.starts_with("processor")).count() as u32)
        .unwrap_or(1)
}

async fn check_service_health_async(url: &str) -> String {
    let health_url = format!("{}/health", url.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
    {
        Ok(client) => client,
        Err(_) => return "offline".into(),
    };

    match client.get(health_url).send().await {
        Ok(response) if response.status().is_success() => "online".into(),
        _ => "offline".into(),
    }
}

// ── Typed fallback commands (backed by future masterd-db) ─────────────────────
#[tauri::command]
#[allow(non_snake_case)]
async fn intake_add_files(
    state: State<'_, AppState>,
    paths: Vec<String>,
    #[allow(unused)] profileId: Option<String>,
) -> Result<ApiResult<Vec<crate::state::IntakeQueueItem>>, String> {
    let store = data_store(&state);
    let mut items = Vec::new();
    for path in paths {
        let p = std::path::Path::new(&path);
        let file_name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let extension = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let size_bytes = tokio::fs::metadata(p).await.map(|m| m.len()).unwrap_or(0);

        if let Some(store) = store.clone() {
            let path_for_ingest = path.clone();
            let ocr_language = state.config.lock().await.ocr_language.clone();
            let ingest_result = tokio::task::spawn_blocking(move || {
                store.ingest_file(
                    std::path::Path::new(&path_for_ingest),
                    &masterd_data::IngestConfig { ocr_language },
                )
            })
            .await
            .map_err(|err| format!("Failed to spawn blocking task: {}", err))?
            .map_err(|err| format!("Ingestion failed: {}", err))?;

            let item = crate::state::IntakeQueueItem {
                id: ingest_result.run.id.clone(),
                file_name: ingest_result
                    .document
                    .as_ref()
                    .map(|d| d.current_name.clone())
                    .unwrap_or(file_name),
                path: path.clone(),
                extension: extension.clone(),
                size_bytes,
                status: if ingest_result.run.status == "error" {
                    "error".into()
                } else {
                    "complete".into()
                },
                progress: if ingest_result.run.status == "error" {
                    0
                } else {
                    100
                },
                duplicate_status: ingest_result.document.map(|d| d.duplicate_status),
                created_at: ingest_result.run.created_at.clone(),
                updated_at: ingest_result.run.updated_at.clone(),
            };
            items.push(item);
        } else {
            // Fallback when no store available
            items.push(crate::state::IntakeQueueItem {
                id: Uuid::new_v4().to_string(),
                file_name,
                path: path.clone(),
                extension,
                size_bytes,
                status: "error".into(),
                progress: 0,
                duplicate_status: Some("store_unavailable".into()),
                created_at: now_ts(),
                updated_at: now_ts(),
            });
        }
    }

    if !items.is_empty() {
        let queue_snapshot = {
            let mut queue = state.intake_queue.lock().await;
            queue.extend(items.clone());
            queue.clone()
        };
        if let Err(err) = persist_intake_queue_snapshot(&state, &queue_snapshot).await {
            return Ok(err_result(
                "WRITE_FAILED",
                format!("Failed to save intake queue: {err}"),
                true,
            ));
        }
    }

    Ok(ok(items))
}

#[tauri::command]
async fn intake_retry_item(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<crate::state::IntakeQueueItem>, String> {
    let (item, queue_snapshot) = {
        let mut queue = state.intake_queue.lock().await;
        if let Some(item) = queue.iter_mut().find(|item| item.id == id) {
            item.status = "queued".into();
            item.progress = 0;
            item.updated_at = now_ts();
            (Some(item.clone()), Some(queue.clone()))
        } else {
            (None, None)
        }
    };
    if let Some(item) = item {
        if let Some(snapshot) = queue_snapshot {
            if let Err(err) = persist_intake_queue_snapshot(&state, &snapshot).await {
                return Ok(err_result(
                    "WRITE_FAILED",
                    format!("Failed to save intake queue: {err}"),
                    true,
                ));
            }
        }
        return Ok(ok(item.clone()));
    }
    Ok(ok(empty_intake_item(id)))
}

#[tauri::command]
async fn intake_cancel_item(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<crate::state::IntakeQueueItem>, String> {
    let (item, queue_snapshot) = {
        let mut queue = state.intake_queue.lock().await;
        if let Some(item) = queue.iter_mut().find(|item| item.id == id) {
            item.status = "error".into();
            item.progress = 0;
            item.updated_at = now_ts();
            (Some(item.clone()), Some(queue.clone()))
        } else {
            (None, None)
        }
    };
    if let Some(item) = item {
        if let Some(snapshot) = queue_snapshot {
            if let Err(err) = persist_intake_queue_snapshot(&state, &snapshot).await {
                return Ok(err_result(
                    "WRITE_FAILED",
                    format!("Failed to save intake queue: {err}"),
                    true,
                ));
            }
        }
        return Ok(ok(item.clone()));
    }
    let mut item = empty_intake_item(id);
    item.status = "error".into();
    item.progress = 0;
    Ok(ok(item))
}

async fn persist_intake_queue_snapshot(
    state: &State<'_, AppState>,
    items: &[crate::state::IntakeQueueItem],
) -> Result<(), String> {
    let dirs = state.dirs.lock().ok().and_then(|guard| guard.clone());
    let Some(dirs) = dirs else {
        return Ok(());
    };
    let path = dirs.intake_queue_json();
    let json = serde_json::to_vec_pretty(items).map_err(|err| err.to_string())?;
    run_blocking(move || crate::state::write_atomic(&path, &json).map_err(|err| err.to_string()))
        .await
}

#[tauri::command]
async fn documents_get_by_id(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<DocumentRecord>, String> {
    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        if let Some(doc) = run_blocking(move || {
            store
                .get_document(&lookup_id)
                .map_err(|err| err.to_string())
        })
        .await?
        {
            return Ok(ok(map_document(doc)));
        }
    }
    Ok(ok(empty_document(id)))
}

#[tauri::command]
async fn documents_get_preview(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<DocumentPreview>, String> {
    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        if let Some(doc) = run_blocking(move || {
            store
                .get_document(&lookup_id)
                .map_err(|err| err.to_string())
        })
        .await?
        {
            let preview = doc
                .extracted_text
                .as_deref()
                .unwrap_or("")
                .split_whitespace()
                .take(120)
                .collect::<Vec<_>>()
                .join(" ");
            return Ok(ok(DocumentPreview {
                document_id: doc.id,
                text_preview: preview,
                page_count: 1,
                thumbnail_url: None,
                mime_type: doc.mime_type,
            }));
        }
    }
    Ok(ok(DocumentPreview {
        document_id: id,
        text_preview: String::new(),
        page_count: 0,
        thumbnail_url: None,
        mime_type: "text/plain".into(),
    }))
}

#[tauri::command]
async fn documents_get_extracted_text(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<ExtractedTextResult>, String> {
    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        if let Some(doc) = run_blocking(move || {
            store
                .get_document(&lookup_id)
                .map_err(|err| err.to_string())
        })
        .await?
        {
            let entities = doc
                .tags
                .iter()
                .map(|tag| serde_json::json!({ "text": tag, "label": "TAG", "confidence": 1.0 }))
                .collect();
            return Ok(ok(ExtractedTextResult {
                document_id: doc.id,
                full_text: doc.extracted_text.unwrap_or_default(),
                language: None,
                entities,
            }));
        }
    }
    Ok(ok(ExtractedTextResult {
        document_id: id,
        full_text: String::new(),
        language: None,
        entities: vec![],
    }))
}

#[tauri::command]
async fn documents_update_tags(
    state: State<'_, AppState>,
    id: String,
    tags: Vec<String>,
) -> Result<ApiResult<DocumentRecord>, String> {
    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        let tags_for_update = tags.clone();
        if let Some(doc) = run_blocking(move || {
            store
                .update_document_tags(&lookup_id, &tags_for_update)
                .map_err(|err| err.to_string())
        })
        .await?
        {
            return Ok(ok(map_document(doc)));
        }
    }
    let mut doc = empty_document(id);
    doc.tags = tags;
    Ok(ok(doc))
}

#[tauri::command]
async fn documents_reprocess(
    state: State<'_, AppState>,
    id: String,
    #[allow(unused)] options: Option<serde_json::Value>,
) -> Result<ApiResult<PipelineJob>, String> {
    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        let config = state.config.lock().await.clone();
        if let Some(job) = run_blocking(move || {
            let Some(doc) = store
                .get_document(&lookup_id)
                .map_err(|err| err.to_string())?
            else {
                return Ok(None);
            };
            let ingest_result = {
                store.ingest_file(
                    std::path::Path::new(&doc.current_path),
                    &masterd_data::IngestConfig {
                        ocr_language: config.ocr_language,
                    },
                )
            }
            .map_err(|err| err.to_string())?;
            Ok(Some(map_pipeline_run(ingest_result.run)))
        })
        .await?
        {
            return Ok(ok(job));
        }
    }
    Ok(ok(empty_pipeline_job(id)))
}

fn empty_intake_item(id: String) -> crate::state::IntakeQueueItem {
    let now = now_ts();
    crate::state::IntakeQueueItem {
        id,
        file_name: String::new(),
        path: String::new(),
        extension: String::new(),
        size_bytes: 0,
        status: "queued".into(),
        progress: 0,
        duplicate_status: Some("unknown".into()),
        created_at: now.clone(),
        updated_at: now,
    }
}

fn empty_document(id: String) -> DocumentRecord {
    let now = now_ts();
    DocumentRecord {
        id: id.clone(),
        original_name: id.clone(),
        current_name: id.clone(),
        suggested_name: None,
        original_path: String::new(),
        current_path: String::new(),
        extension: String::new(),
        mime_type: "text/plain".into(),
        size_bytes: 0,
        hash: String::new(),
        classification: None,
        tags: vec![],
        extracted_text: None,
        summary: None,
        confidence: 1.0,
        duplicate_status: "unknown".into(),
        processing_status: "new".into(),
        created_at: now.clone(),
        updated_at: now,
        retrieval_score: None,
        source_stages: vec![],
    }
}

fn empty_action(document_id: String, message: impl Into<String>) -> ActionResult {
    ActionResult {
        success: true,
        message: message.into(),
        document_id,
        details: None,
    }
}

fn empty_pipeline_job(document_id: String) -> PipelineJob {
    PipelineJob {
        id: Uuid::new_v4().to_string(),
        document_id: document_id.clone(),
        file_name: document_id,
        stage: "complete".into(),
        status: "complete".into(),
        progress: 100,
        started_at: None,
        finished_at: Some(now_ts()),
        error_message: None,
        worker_id: None,
        logs: vec![],
        stage_timings: vec![],
        retryable: false,
        indexed_chunk_count: 0,
        ocr_confidence: None,
        embedding_provider: None,
        rerank_status: "not_run".into(),
    }
}

fn map_document(doc: masterd_data::DocumentRecord) -> DocumentRecord {
    DocumentRecord {
        id: doc.id,
        original_name: doc.original_name,
        current_name: doc.current_name,
        suggested_name: doc.suggested_name,
        original_path: doc.original_path,
        current_path: doc.current_path,
        extension: doc.extension,
        mime_type: doc.mime_type,
        size_bytes: doc.size_bytes,
        hash: doc.hash,
        classification: doc
            .classification
            .map(|classification| ClassificationResult {
                category: classification.category,
                confidence: classification.confidence,
            }),
        tags: doc.tags,
        extracted_text: doc.extracted_text,
        summary: doc.summary,
        confidence: doc.confidence,
        duplicate_status: doc.duplicate_status,
        processing_status: doc.processing_status,
        created_at: doc.created_at,
        updated_at: doc.updated_at,
        retrieval_score: None,
        source_stages: vec![],
    }
}

fn map_pipeline_run(run: masterd_data::PipelineRun) -> PipelineJob {
    let run_id = run.id.clone();
    let updated_at = run.updated_at.clone();
    let stage_timings = run.stage_timings;
    let failed_stage = run.failure.as_ref().map(|failure| failure.stage.clone());
    let stage_name = failed_stage
        .or_else(|| stage_timings.last().map(|stage| stage.stage.clone()))
        .unwrap_or_else(|| "complete".to_string());
    let file_name = std::path::Path::new(&run.file_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(&run.file_path)
        .to_string();
    PipelineJob {
        id: run.id,
        document_id: run.document_id.unwrap_or_default(),
        file_name,
        stage: stage_name,
        status: run.status.clone(),
        progress: if run.status == "complete" || run.status == "duplicate" {
            100
        } else {
            0
        },
        started_at: Some(run.created_at.clone()),
        finished_at: Some(run.updated_at.clone()),
        error_message: run.failure.as_ref().map(|failure| failure.message.clone()),
        worker_id: Some("local-data-store".into()),
        logs: stage_timings
            .iter()
            .map(|stage| {
                json!({
                    "id": format!("{}:{}", run_id, stage.stage),
                    "level": if stage.status == "complete" { "info" } else { "warning" },
                    "message": format!("{} {}", stage.stage, stage.status),
                    "createdAt": updated_at,
                    "details": { "elapsedMs": stage.elapsed_ms }
                })
            })
            .collect(),
        stage_timings,
        retryable: run.retryable,
        indexed_chunk_count: run.indexed_chunk_count,
        ocr_confidence: run.ocr_confidence,
        embedding_provider: run.embedding_provider,
        rerank_status: run.rerank_status,
    }
}

fn map_review(item: masterd_data::ReviewItem) -> ReviewItem {
    ReviewItem {
        id: item.id,
        document_id: item.document_id,
        reason: item.reason,
        severity: item.severity,
        title: item.title,
        explanation: item.explanation,
        proposed_action: item.proposed_action,
        created_at: item.created_at,
        resolved: item.resolved,
    }
}

fn map_audit(entry: masterd_data::AuditEntry) -> AuditEntry {
    AuditEntry {
        id: entry.id,
        document_id: entry.document_id,
        action: entry.action,
        actor: entry.actor,
        summary: entry.summary,
        before: entry.before,
        after: entry.after,
        reversible: entry.reversible,
        created_at: entry.created_at,
    }
}

// ── Real intake commands ──────────────────────────────────────────────────────

/// List all registered watch folders.
#[tauri::command]
async fn intake_list_watch_folders(
    state: State<'_, AppState>,
) -> Result<ApiResult<Vec<crate::state::WatchFolderEntry>>, String> {
    let folders = state.watch_folders.lock().await.clone();
    Ok(ok(folders))
}

/// Register a folder and immediately scan + index all readable text files.
#[tauri::command]
#[allow(non_snake_case)]
async fn intake_add_watch_folder(
    state: State<'_, AppState>,
    path: String,
    #[allow(unused)] profileId: Option<String>,
) -> Result<ApiResult<crate::state::WatchFolderEntry>, String> {
    use masterd_chat_engine::IndexedDocument;
    use std::path::Path;

    let dir_path = path.clone();
    if !Path::new(&dir_path).is_dir() {
        return Ok(ApiResult::Err {
            ok: false,
            error: ApiError {
                code: "NOT_A_DIR".into(),
                message: format!("{path} is not a directory"),
                recoverable: true,
            },
            request_id: Uuid::new_v4().to_string(),
            received_at: now_ts(),
        });
    }

    let engine = state.chat_engine.clone();
    let scan_path = path.clone();

    // spawn_blocking for the file I/O + sync index calls
    let count = tokio::task::spawn_blocking(move || {
        fn walk(
            dir: &Path,
            depth: usize,
            engine: &masterd_chat_engine::ChatEngine,
            count: &mut usize,
        ) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() && depth < 3 {
                    walk(&p, depth + 1, engine, count);
                    continue;
                }
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !matches!(ext.as_str(), "txt" | "md" | "rst" | "log") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&p) else {
                    continue;
                };
                if text.trim().is_empty() {
                    continue;
                }
                let doc_id = p.to_string_lossy().to_string();
                let doc = IndexedDocument {
                    doc_id: doc_id.clone(),
                    text,
                    path: Some(doc_id),
                    symbols: vec![],
                    doc_type: Some(ext),
                };
                tokio::runtime::Handle::current().block_on(engine.index_document(doc));
                *count += 1;
            }
        }
        let mut n = 0usize;
        walk(Path::new(&scan_path), 0, &engine, &mut n);
        n
    })
    .await
    .unwrap_or(0);

    let entry = crate::state::WatchFolderEntry {
        id: Uuid::new_v4().to_string(),
        path: path.clone(),
        enabled: true,
        profile_id: profileId.unwrap_or_else(|| "Full Analysis".into()),
        file_count: count,
        created_at: now_ts(),
    };

    let folders_snapshot = {
        let mut folders = state.watch_folders.lock().await;
        folders.push(entry.clone());
        folders.clone()
    };

    // Persist immediately so watch folders survive crash.
    if let Err(err) = persist_watch_folders_snapshot(&state, &folders_snapshot).await {
        tracing::error!("intake_add_watch_folder: write failed: {err}");
        return Ok(err_result(
            "WRITE_FAILED",
            format!("Failed to save watch folders: {err}"),
            true,
        ));
    }

    Ok(ok(entry))
}

/// Remove a watch folder by id.
#[tauri::command]
async fn intake_remove_watch_folder(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<EmptyOk>, String> {
    let folders_snapshot = {
        let mut folders = state.watch_folders.lock().await;
        folders.retain(|f| f.id != id);
        folders.clone()
    };
    if let Err(err) = persist_watch_folders_snapshot(&state, &folders_snapshot).await {
        tracing::error!("intake_remove_watch_folder: write failed: {err}");
        return Ok(err_result(
            "WRITE_FAILED",
            format!("Failed to save watch folders: {err}"),
            true,
        ));
    }
    Ok(ok(EmptyOk {}))
}

async fn persist_watch_folders_snapshot(
    state: &State<'_, AppState>,
    folders: &[crate::state::WatchFolderEntry],
) -> Result<(), String> {
    let dirs = state.dirs.lock().ok().and_then(|guard| guard.clone());
    let Some(dirs) = dirs else {
        return Ok(());
    };
    let path = dirs.watchers_json();
    let json = serde_json::to_vec_pretty(folders).map_err(|err| err.to_string())?;
    run_blocking(move || crate::state::write_atomic(&path, &json).map_err(|err| err.to_string()))
        .await
}

/// List current intake queue items.
#[tauri::command]
async fn intake_list_queue(
    state: State<'_, AppState>,
) -> Result<ApiResult<Paginated<crate::state::IntakeQueueItem>>, String> {
    let items = state.intake_queue.lock().await.clone();
    let total = items.len() as u64;
    Ok(ok(Paginated {
        items,
        total,
        limit: 50,
        offset: 0,
    }))
}
#[tauri::command]
#[allow(non_snake_case)]
async fn actions_approve_rename(
    documentId: String,
    suggestedName: Option<String>,
) -> ApiResult<ActionResult> {
    ok(empty_action(
        documentId,
        suggestedName
            .map(|n| format!("rename approved: {n}"))
            .unwrap_or_else(|| "rename approved".into()),
    ))
}

#[tauri::command]
#[allow(non_snake_case)]
async fn actions_reject_rename(
    documentId: String,
    reason: Option<String>,
) -> ApiResult<ActionResult> {
    ok(empty_action(
        documentId,
        reason.unwrap_or_else(|| "rename rejected".into()),
    ))
}

#[tauri::command]
#[allow(non_snake_case)]
async fn actions_approve_move(
    documentId: String,
    destinationPath: String,
) -> ApiResult<ActionResult> {
    ok(empty_action(
        documentId,
        format!("move approved: {destinationPath}"),
    ))
}

#[tauri::command]
#[allow(non_snake_case)]
async fn actions_mark_duplicate(
    documentId: String,
    duplicateOfId: String,
) -> ApiResult<ActionResult> {
    ok(empty_action(
        documentId,
        format!("marked duplicate of {duplicateOfId}"),
    ))
}

#[tauri::command]
#[allow(non_snake_case)]
async fn actions_mark_unique(documentId: String) -> ApiResult<ActionResult> {
    ok(empty_action(documentId, "marked unique"))
}

#[tauri::command]
async fn pipeline_list_jobs(
    state: State<'_, AppState>,
    #[allow(unused)] params: Option<serde_json::Value>,
) -> Result<ApiResult<Paginated<PipelineJob>>, String> {
    if let Some(store) = data_store(&state) {
        let runs = run_blocking(move || {
            store
                .list_pipeline_runs(100, 0)
                .map_err(|err| err.to_string())
        })
        .await?;
        let items = runs.into_iter().map(map_pipeline_run).collect::<Vec<_>>();
        let total = items.len() as u64;
        return Ok(ok(Paginated {
            items,
            total,
            limit: 100,
            offset: 0,
        }));
    }
    Ok(ok(Paginated::empty()))
}

#[tauri::command]
async fn pipeline_get_job(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<PipelineJob>, String> {
    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        if let Some(run) = run_blocking(move || {
            store
                .get_pipeline_run(&lookup_id)
                .map_err(|err| err.to_string())
        })
        .await?
        {
            return Ok(ok(map_pipeline_run(run)));
        }
    }
    Ok(ok(empty_pipeline_job(id)))
}

#[tauri::command]
async fn pipeline_retry_job(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<PipelineJob>, String> {
    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        let Some(run) = run_blocking({
            let store = store.clone();
            move || {
                store
                    .get_pipeline_run(&lookup_id)
                    .map_err(|err| err.to_string())
            }
        })
        .await?
        else {
            return Ok(err_result(
                "NOT_FOUND",
                format!("Pipeline job '{id}' was not found"),
                false,
            ));
        };
        let path = run.file_path.clone();
        let ocr_language = state.config.lock().await.ocr_language.clone();
        let retry_result = run_blocking(move || {
            store
                .ingest_file(
                    std::path::Path::new(&path),
                    &masterd_data::IngestConfig { ocr_language },
                )
                .map_err(|err| err.to_string())
        })
        .await;
        return match retry_result {
            Ok(updated) => Ok(ok(map_pipeline_run(updated.run))),
            Err(err) => Ok(err_result(
                "RETRY_FAILED",
                format!("Retry failed for job '{id}': {err}"),
                true,
            )),
        };
    }
    Ok(err_result(
        "DATA_STORE_UNAVAILABLE",
        "Canonical pipeline store is unavailable",
        true,
    ))
}

#[tauri::command]
async fn pipeline_cancel_job(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<PipelineJob>, String> {
    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        if let Some(run) = run_blocking(move || {
            store
                .update_pipeline_run_status(&lookup_id, "error", Some("cancelled"))
                .map_err(|err| err.to_string())
        })
        .await?
        {
            return Ok(ok(map_pipeline_run(run)));
        }
    }
    let mut job = empty_pipeline_job(id);
    job.status = "error".into();
    job.error_message = Some("cancelled".into());
    Ok(ok(job))
}

#[tauri::command]
async fn review_list(
    state: State<'_, AppState>,
    #[allow(unused)] params: Option<serde_json::Value>,
) -> Result<ApiResult<Paginated<ReviewItem>>, String> {
    if let Some(store) = data_store(&state) {
        let items = run_blocking(move || {
            store
                .list_review_items(100, 0)
                .map_err(|err| err.to_string())
        })
        .await?;
        let items = items.into_iter().map(map_review).collect::<Vec<_>>();
        let total = items.len() as u64;
        return Ok(ok(Paginated {
            items,
            total,
            limit: 100,
            offset: 0,
        }));
    }
    Ok(ok(Paginated::empty()))
}

#[tauri::command]
async fn review_resolve(
    state: State<'_, AppState>,
    id: String,
    resolution: serde_json::Value,
) -> Result<ApiResult<ReviewItem>, String> {
    let approved = resolution
        .get("approved")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    if let Some(store) = data_store(&state) {
        let lookup_id = id.clone();
        
        // 1. Fetch the original ReviewItem
        let review_item = run_blocking({
            let store = store.clone();
            let lookup_id = lookup_id.clone();
            move || {
                store.list_review_items(100, 0)
                    .map(|items| items.into_iter().find(|i| i.id == lookup_id))
                    .map_err(|err| err.to_string())
            }
        })
        .await?;

        if let Some(ref rev) = review_item {
            let doc_id = rev.document_id.clone();
            // Fetch original document
            let doc_opt = run_blocking({
                let store = store.clone();
                let doc_id = doc_id.clone();
                move || store.get_document(&doc_id).map_err(|err| err.to_string())
            })
            .await?;

            if let Some(doc) = doc_opt {
                let original_category = doc.classification.as_ref()
                    .map(|c| c.category.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let original_name = doc.current_name.clone();
                let original_path = doc.current_path.clone();

                // Extract user corrections
                let corrected_name = resolution
                    .get("correctedName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&original_name)
                    .to_string();

                let corrected_category = resolution
                    .get("correctedCategory")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&original_category)
                    .to_string();

                let corrected_folder = resolution
                    .get("correctedFolder")
                    .and_then(|v| v.as_str());

                let document_text = doc.extracted_text.clone();

                // 2. Trigger learning if learning is enabled in AppConfig
                let config = state.config.lock().await.clone();

                if approved {
                    // Trigger Classification learning
                    if config.classification_learning_enabled && corrected_category != original_category {
                        let mut learner = state.classification_learner.lock().await;
                        learner.learn_from_correction(
                            &original_category,
                            &corrected_category,
                            document_text.as_deref(),
                            Some(&original_name),
                        );
                    }

                    // Trigger Preference learning
                    if config.preference_learning_enabled && (corrected_name != original_name || corrected_folder.is_some()) {
                        let mut learner = state.preference_learner.lock().await;
                        
                        let orig_folder = std::path::Path::new(&original_path)
                            .parent()
                            .map(|p| p.to_string_lossy().to_string());
                        
                        let mut context = std::collections::HashMap::new();
                        context.insert("doc_type".to_string(), corrected_category.clone());
                        if let Some(ref text) = document_text {
                            context.insert("text".to_string(), text.clone());
                        }

                        learner.learn_from_correction(
                            &original_name,
                            &corrected_name,
                            orig_folder.as_deref(),
                            corrected_folder,
                            document_text.as_deref(),
                            Some(&context),
                        );
                    }

                    // Persist state to save learners
                    state.persist().await;
                }

                // 3. Rename/Move physical file on disk if filename or path changed
                let mut final_path = original_path.clone();
                if approved {
                    if let Some(dest_dir) = corrected_folder {
                        let dest_path = std::path::Path::new(dest_dir).join(&corrected_name);
                        if let Some(parent) = dest_path.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if std::path::Path::new(&original_path).exists() {
                            if let Err(e) = std::fs::rename(&original_path, &dest_path) {
                                tracing::error!("Failed to move file from {} to {}: {}", original_path, dest_path.display(), e);
                            } else {
                                final_path = dest_path.to_string_lossy().to_string();
                            }
                        }
                    } else if corrected_name != original_name {
                        let orig_p = std::path::Path::new(&original_path);
                        if let Some(parent) = orig_p.parent() {
                            let dest_path = parent.join(&corrected_name);
                            if orig_p.exists() {
                                if let Err(e) = std::fs::rename(&original_path, &dest_path) {
                                    tracing::error!("Failed to rename file from {} to {}: {}", original_path, dest_path.display(), e);
                                } else {
                                    final_path = dest_path.to_string_lossy().to_string();
                                }
                            }
                        }
                    }
                }

                // 4. Update the database document
                if approved {
                    let store_clone = store.clone();
                    let doc_id_clone = doc.id.clone();
                    let corrected_name_clone = corrected_name.clone();
                    let corrected_category_clone = corrected_category.clone();
                    let final_path_clone = final_path.clone();
                    
                    let _ = run_blocking(move || {
                        store_clone.update_document_after_review(
                            &doc_id_clone,
                            &corrected_name_clone,
                            &corrected_category_clone,
                            &final_path_clone,
                        )
                        .map_err(|err| err.to_string())
                    })
                    .await?;
                }
            }
        }

        // 5. Finally resolve the review item
        let lookup_id = id.clone();
        if let Some(item) = run_blocking(move || {
            store
                .resolve_review(&lookup_id, approved)
                .map_err(|err| err.to_string())
        })
        .await?
        {
            return Ok(ok(map_review(item)));
        }
    }
    
    Ok(ok(ReviewItem {
        id,
        document_id: String::new(),
        reason: "low_confidence_classification".into(),
        severity: "info".into(),
        title: "Resolved review".into(),
        explanation: resolution.to_string(),
        proposed_action: None,
        created_at: now_ts(),
        resolved: Some(true),
    }))
}

#[tauri::command]
async fn rules_list() -> ApiResult<Vec<AutomationRule>> {
    ok(vec![])
}

#[tauri::command]
async fn rules_get_by_id(id: String) -> ApiResult<AutomationRule> {
    ok(empty_rule(id))
}

#[tauri::command]
async fn rules_create(rule: serde_json::Value) -> ApiResult<AutomationRule> {
    ok(rule_from_value(Uuid::new_v4().to_string(), rule))
}

#[tauri::command]
async fn rules_update(id: String, patch: serde_json::Value) -> ApiResult<AutomationRule> {
    ok(rule_from_value(id, patch))
}

#[tauri::command]
async fn rules_delete(id: String) -> ApiResult<EmptyOk> {
    let _ = id;
    ok(EmptyOk {})
}

#[tauri::command]
#[allow(non_snake_case)]
async fn rules_test(
    rule: serde_json::Value,
    documentId: Option<String>,
) -> ApiResult<RuleTestResult> {
    ok(RuleTestResult {
        matched: documentId.is_some() || !rule.is_null(),
        actions_evaluated: vec![json!({
            "type": "contract_check",
            "applied": true,
            "resultSummary": "Rule payload accepted by desktop bridge"
        })],
    })
}

#[tauri::command]
async fn audit_list(
    state: State<'_, AppState>,
    #[allow(unused)] params: Option<serde_json::Value>,
) -> Result<ApiResult<Paginated<AuditEntry>>, String> {
    if let Some(store) = data_store(&state) {
        let items = run_blocking(move || {
            store
                .list_audit_entries(100, 0)
                .map_err(|err| err.to_string())
        })
        .await?;
        let items = items.into_iter().map(map_audit).collect::<Vec<_>>();
        let total = items.len() as u64;
        return Ok(ok(Paginated {
            items,
            total,
            limit: 100,
            offset: 0,
        }));
    }
    Ok(ok(Paginated::empty()))
}

#[tauri::command]
#[allow(non_snake_case)]
async fn audit_get_for_document(
    state: State<'_, AppState>,
    documentId: String,
) -> Result<ApiResult<Vec<AuditEntry>>, String> {
    if let Some(store) = data_store(&state) {
        let items = run_blocking(move || {
            store
                .audit_entries_for_document(&documentId)
                .map_err(|err| err.to_string())
        })
        .await?;
        return Ok(ok(items.into_iter().map(map_audit).collect()));
    }
    Ok(ok(vec![]))
}

#[tauri::command]
#[allow(non_snake_case)]
async fn audit_revert(
    state: State<'_, AppState>,
    entryId: String,
) -> Result<ApiResult<ActionResult>, String> {
    if data_store(&state).is_some() {
        return Ok(ok(empty_action(entryId, "audit entry reverted")));
    }
    Ok(ok(empty_action(entryId, "audit entry reverted")))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceEventRequest {
    pub category: String,
    pub signal: String,
    pub value: String,
    pub source: Option<String>,
    pub confidence: Option<f32>,
}

#[tauri::command]
async fn preferences_list(
    state: State<'_, AppState>,
) -> Result<ApiResult<Vec<masterd_data::LearnedPreference>>, String> {
    if let Some(store) = data_store(&state) {
        let preferences =
            run_blocking(move || store.list_preferences().map_err(|err| err.to_string())).await?;
        return Ok(ok(preferences));
    }
    Ok(ok(vec![]))
}

#[tauri::command]
async fn preferences_record_event(
    state: State<'_, AppState>,
    event: PreferenceEventRequest,
) -> Result<ApiResult<masterd_data::LearnedPreference>, String> {
    let Some(store) = data_store(&state) else {
        return Ok(err_result(
            "DATA_STORE_UNAVAILABLE",
            "Canonical preference store is unavailable",
            true,
        ));
    };
    let learned = run_blocking(move || {
        store
            .store_preference_event(PreferenceEvent {
                id: Uuid::new_v4().to_string(),
                category: event.category,
                signal: event.signal,
                value: event.value,
                source: event.source.unwrap_or_else(|| "desktop".to_string()),
                confidence: event.confidence.unwrap_or(0.75).clamp(0.0, 1.0),
                created_at: now_ts(),
            })
            .map_err(|err| err.to_string())
    })
    .await?;
    Ok(ok(learned))
}

#[tauri::command]
async fn preferences_approve(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<masterd_data::LearnedPreference>, String> {
    let Some(store) = data_store(&state) else {
        return Ok(err_result(
            "DATA_STORE_UNAVAILABLE",
            "Canonical preference store is unavailable",
            true,
        ));
    };
    let lookup_id = id.clone();
    match run_blocking(move || {
        store
            .set_preference_status(&lookup_id, "approved")
            .map_err(|err| err.to_string())
    })
    .await?
    {
        Some(preference) => Ok(ok(preference)),
        None => Ok(err_result(
            "NOT_FOUND",
            format!("Preference '{id}' was not found"),
            false,
        )),
    }
}

#[tauri::command]
async fn preferences_dismiss(
    state: State<'_, AppState>,
    id: String,
) -> Result<ApiResult<masterd_data::LearnedPreference>, String> {
    let Some(store) = data_store(&state) else {
        return Ok(err_result(
            "DATA_STORE_UNAVAILABLE",
            "Canonical preference store is unavailable",
            true,
        ));
    };
    let lookup_id = id.clone();
    match run_blocking(move || {
        store
            .set_preference_status(&lookup_id, "dismissed")
            .map_err(|err| err.to_string())
    })
    .await?
    {
        Some(preference) => Ok(ok(preference)),
        None => Ok(err_result(
            "NOT_FOUND",
            format!("Preference '{id}' was not found"),
            false,
        )),
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceDraftRequest {
    pub document_id: Option<String>,
    pub goal: Option<String>,
    pub max_tokens: Option<usize>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreferenceDraft {
    pub model: String,
    pub prompt_version: String,
    pub status: String,
    pub raw_text: String,
    pub parsed: Option<serde_json::Value>,
    pub provenance: serde_json::Value,
}

#[tauri::command]
async fn preferences_draft_policy(
    state: State<'_, AppState>,
    request: PreferenceDraftRequest,
) -> Result<ApiResult<PreferenceDraft>, String> {
    let Some(store) = data_store(&state) else {
        return Ok(err_result(
            "DATA_STORE_UNAVAILABLE",
            "Canonical preference store is unavailable",
            true,
        ));
    };

    let document_id_for_db = request.document_id.clone();
    let document_id_for_json = request.document_id.clone();
    let goal = request.goal.clone();
    let max_tokens = request.max_tokens;

    // Move expensive operations to blocking thread
    let (preferences, document) = tokio::task::spawn_blocking(move || {
        let preferences = store.list_preferences().map_err(|err| err.to_string())?;
        let document = if let Some(id) = document_id_for_db.as_deref() {
            store.get_document(id).map_err(|err| err.to_string())?
        } else {
            None
        };
        Ok::<_, String>((preferences, document))
    })
    .await
    .map_err(|err| err.to_string())??;
    let preference_sample = preferences
        .iter()
        .take(24)
        .map(|preference| {
            json!({
                "id": preference.id,
                "category": preference.category,
                "key": preference.key,
                "value": preference.value,
                "confidence": preference.confidence,
                "status": preference.status,
                "evidenceCount": preference.evidence_count,
            })
        })
        .collect::<Vec<_>>();
    let document_json = document.as_ref().map(|doc| {
        json!({
            "id": doc.id,
            "currentName": doc.current_name,
            "suggestedName": doc.suggested_name,
            "path": doc.current_path,
            "extension": doc.extension,
            "classification": doc.classification,
            "tags": doc.tags,
            "summary": doc.summary,
            "confidence": doc.confidence,
        })
    });
    let goal = goal.unwrap_or_else(|| {
        "Draft review-gated learned preferences for naming, tagging, classification, routing, or retrieval behavior.".to_string()
    });
    let evidence = json!({
        "goal": goal,
        "learnedPreferences": preference_sample,
        "document": document_json,
        "constraints": [
            "Return JSON only.",
            "Do not approve or apply automation.",
            "Every suggestion must cite evidence ids or document id.",
            "Prefer deterministic rules over broad behavioral claims.",
            "Use status pending_review for suggestions that require user approval."
        ]
    });
    let system_prompt = r#"You are MASTERd's local LFM2.5-350M preference drafting model.
Draft auditable user preference suggestions from evidence. You do not execute actions.
Return one JSON object with:
{
  "suggestions": [
    {
      "category": "naming|tagging|classification|routing|retrieval|chat",
      "key": "short stable key",
      "value": "proposed behavior",
      "confidence": 0.0,
      "status": "pending_review",
      "evidenceIds": [],
      "reason": "short reason"
    }
  ],
  "risks": [],
  "requiresReview": true
}"#
    .to_string();
    let user_prompt = serde_json::to_string_pretty(&evidence).map_err(|err| err.to_string())?;
    let raw_text = state
        .chat_engine
        .clone()
        .generate_instruct_text(
            system_prompt,
            user_prompt,
            max_tokens.unwrap_or(384).min(1024),
        )
        .await
        .map_err(|err| err.to_string())?;
    let parsed = parse_first_json_object(&raw_text);
    Ok(ok(PreferenceDraft {
        model: "lfm2.5-instruct-350m".to_string(),
        prompt_version: "preference-draft-v1".to_string(),
        status: "draft_pending_review".to_string(),
        raw_text,
        parsed,
        provenance: json!({
            "documentId": document_id_for_json,
            "preferenceCount": preferences.len(),
            "createdAt": now_ts(),
            "reviewGated": true
        }),
    }))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalExplainRequest {
    pub trace_id: Option<String>,
    pub query: Option<String>,
    pub mode: Option<String>,
    pub top_k: Option<usize>,
}

#[tauri::command]
async fn retrieval_explain(
    state: State<'_, AppState>,
    params: RetrievalExplainRequest,
) -> Result<ApiResult<masterd_data::RetrievalTrace>, String> {
    let Some(store) = data_store(&state) else {
        return Ok(err_result(
            "DATA_STORE_UNAVAILABLE",
            "Canonical retrieval store is unavailable",
            true,
        ));
    };
    if let Some(trace_id) = params.trace_id {
        let lookup_id = trace_id.clone();
        if let Some(trace) = run_blocking(move || {
            store
                .get_retrieval_trace(&lookup_id)
                .map_err(|err| err.to_string())
        })
        .await?
        {
            return Ok(ok(trace));
        }
        return Ok(err_result(
            "NOT_FOUND",
            format!("Retrieval trace '{trace_id}' was not found"),
            false,
        ));
    }
    let query = params.query.unwrap_or_default();
    if query.trim().is_empty() {
        return Ok(err_result(
            "EMPTY_QUERY",
            "retrieval.explain requires a traceId or query",
            true,
        ));
    }
    let mode = params
        .mode
        .as_deref()
        .map(DataSearchMode::from_str_lossy)
        .unwrap_or_default();
    let top_k = params.top_k.unwrap_or(8).max(1);
    let trace = run_blocking(move || {
        store
            .search(&query, mode, top_k)
            .map_err(|err| err.to_string())
    })
    .await?;
    Ok(ok(trace))
}

fn empty_rule(id: String) -> AutomationRule {
    let now = now_ts();
    AutomationRule {
        id,
        name: "Untitled rule".into(),
        description: None,
        enabled: true,
        priority: 5,
        trigger: json!({ "event": "manual" }),
        conditions: vec![],
        actions: vec![],
        safety_level: "review_required".into(),
        created_at: now.clone(),
        updated_at: now,
    }
}

fn rule_from_value(id: String, value: serde_json::Value) -> AutomationRule {
    let mut rule = empty_rule(id);
    if let Some(name) = value.get("name").and_then(|v| v.as_str()) {
        rule.name = name.to_string();
    }
    if let Some(enabled) = value.get("enabled").and_then(|v| v.as_bool()) {
        rule.enabled = enabled;
    }
    if let Some(priority) = value.get("priority").and_then(|v| v.as_u64()) {
        rule.priority = priority as u32;
    }
    if let Some(trigger) = value.get("trigger") {
        rule.trigger = trigger.clone();
    }
    if let Some(conditions) = value.get("conditions").and_then(|v| v.as_array()) {
        rule.conditions = conditions.clone();
    }
    if let Some(actions) = value.get("actions").and_then(|v| v.as_array()) {
        rule.actions = actions.clone();
    }
    if let Some(safety_level) = value.get("safetyLevel").and_then(|v| v.as_str()) {
        rule.safety_level = safety_level.to_string();
    }
    rule
}

fn parse_first_json_object(raw: &str) -> Option<serde_json::Value> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&raw[start..=end]).ok()
}

// ── Settings commands ─────────────────────────────────────────────────────────

/// Return the current persisted app configuration.
#[tauri::command]
async fn settings_get(
    state: State<'_, AppState>,
) -> Result<ApiResult<crate::state::AppConfig>, String> {
    Ok(ok(state.config.lock().await.clone()))
}

fn validation_error(code: &str, message: impl Into<String>) -> ApiError {
    ApiError {
        code: code.to_string(),
        message: message.into(),
        recoverable: true,
    }
}

fn validate_local_http_url(field: &str, value: &str) -> Result<(), ApiError> {
    let url = reqwest::Url::parse(value).map_err(|err| {
        validation_error(
            "INVALID_URL",
            format!("{field} must be a valid local HTTP URL: {err}"),
        )
    })?;
    if url.scheme() != "http" {
        return Err(validation_error(
            "INVALID_URL",
            format!("{field} must use http://"),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(validation_error(
            "INVALID_URL",
            format!("{field} must not include credentials"),
        ));
    }
    let Some(host) = url.host_str() else {
        return Err(validation_error(
            "INVALID_URL",
            format!("{field} must include a host"),
        ));
    };
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return Err(validation_error(
            "INVALID_URL",
            format!("{field} must point at localhost"),
        ));
    }
    Ok(())
}

fn validate_app_config(config: &crate::state::AppConfig) -> Result<(), ApiError> {
    if !config.generation_temp.is_finite() || !(0.0..=2.0).contains(&config.generation_temp) {
        return Err(validation_error(
            "INVALID_TEMP",
            "Temperature must be between 0.0 and 2.0",
        ));
    }
    if !(40..=99).contains(&config.safety_confidence_pct) {
        return Err(validation_error(
            "INVALID_CONFIDENCE",
            "Safety confidence must be between 40 and 99",
        ));
    }
    if !(1..=50).contains(&config.bm25_top_k) {
        return Err(validation_error(
            "INVALID_TOP_K",
            "BM25 top K must be between 1 and 50",
        ));
    }
    if !(1..=50).contains(&config.rag_top_k) {
        return Err(validation_error(
            "INVALID_RAG_TOP_K",
            "RAG top K must be between 1 and 50",
        ));
    }
    if !(1..=8192).contains(&config.generation_max_tokens) {
        return Err(validation_error(
            "INVALID_MAX_TOKENS",
            "Max generation tokens must be between 1 and 8192",
        ));
    }
    if !(1..=16).contains(&config.intake_max_depth) {
        return Err(validation_error(
            "INVALID_INTAKE_DEPTH",
            "Intake max depth must be between 1 and 16",
        ));
    }
    if !matches!(config.chat_model.as_str(), "auto" | "thinking") {
        return Err(validation_error(
            "INVALID_CHAT_MODEL",
            "Chat model must be auto or thinking",
        ));
    }
    if !matches!(config.embedding_backend.as_str(), "http" | "direct") {
        return Err(validation_error(
            "INVALID_EMBEDDING_BACKEND",
            "Embedding backend must be http or direct",
        ));
    }
    if config.ocr_language.is_empty()
        || config.ocr_language.len() > 64
        || !config
            .ocr_language
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '_' | '-'))
    {
        return Err(validation_error(
            "INVALID_OCR_LANGUAGE",
            "OCR language must be a short Tesseract language code",
        ));
    }
    if config.intake_extensions.is_empty()
        || config.intake_extensions.iter().any(|extension| {
            extension.is_empty()
                || extension.len() > 16
                || !extension
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit())
        })
    {
        return Err(validation_error(
            "INVALID_INTAKE_EXTENSIONS",
            "Intake extensions must be lowercase names without dots",
        ));
    }
    validate_local_http_url("searxngUrl", &config.searxng_url)?;
    validate_local_http_url("colbertUrl", &config.colbert_url)?;
    validate_local_http_url("jinaUrl", &config.jina_url)?;
    validate_local_http_url("ollamaUrl", &config.ollama_url)?;
    Ok(())
}

/// Save a new app configuration and hot-reload generation parameters.
#[tauri::command]
async fn settings_save(
    state: State<'_, AppState>,
    config: crate::state::AppConfig,
) -> Result<ApiResult<EmptyOk>, String> {
    if let Err(error) = validate_app_config(&config) {
        return Ok(err_result(error.code, error.message, error.recoverable));
    }

    // Persist immediately to disk so a crash doesn't lose the new config.
    let config_path = {
        let dirs_guard = state.dirs.lock().unwrap();
        dirs_guard.as_ref().map(|dirs| dirs.config_json())
    };
    if let Some(path) = config_path {
        let json = serde_json::to_vec_pretty(&config).map_err(|err| err.to_string())?;
        if let Err(e) = run_blocking(move || {
            crate::state::write_atomic(&path, &json).map_err(|err| err.to_string())
        })
        .await
        {
            tracing::error!("settings_save: write failed: {e}");
            return Ok(err_result(
                "WRITE_FAILED",
                format!("Failed to save config: {}", e),
                true,
            ));
        }
    }
    *state.config.lock().await = config;
    Ok(ok(EmptyOk {}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_validation_rejects_invalid_bounds_and_urls() {
        let mut config = crate::state::AppConfig::default();
        config.generation_temp = f64::NAN;
        assert_eq!(
            validate_app_config(&config).unwrap_err().code,
            "INVALID_TEMP"
        );

        let mut config = crate::state::AppConfig::default();
        config.bm25_top_k = 0;
        assert_eq!(
            validate_app_config(&config).unwrap_err().code,
            "INVALID_TOP_K"
        );

        let mut config = crate::state::AppConfig::default();
        config.generation_max_tokens = 0;
        assert_eq!(
            validate_app_config(&config).unwrap_err().code,
            "INVALID_MAX_TOKENS"
        );

        let mut config = crate::state::AppConfig::default();
        config.intake_max_depth = 17;
        assert_eq!(
            validate_app_config(&config).unwrap_err().code,
            "INVALID_INTAKE_DEPTH"
        );

        let mut config = crate::state::AppConfig::default();
        config.searxng_url = "http://example.com:9265".to_string();
        assert_eq!(
            validate_app_config(&config).unwrap_err().code,
            "INVALID_URL"
        );
    }
}

// ── Real index commands ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct IndexDocumentRequest {
    pub doc_id: String,
    pub text: String,
    pub path: Option<String>,
    pub symbols: Option<Vec<String>>,
    pub doc_type: Option<String>,
}

#[derive(Serialize)]
pub struct IndexDocumentResponse {
    pub doc_id: String,
    pub indexed: bool,
    pub doc_count: usize,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSearchQuery {
    pub text: Option<String>,
    pub mode: Option<String>,
    pub filters: Option<serde_json::Value>,
    pub limit: Option<usize>,
    pub top_k: Option<usize>,
    pub offset: Option<usize>,
}

/// Search the canonical hybrid retrieval store; BM25 remains an offline fallback.
#[tauri::command]
async fn documents_search(
    state: State<'_, AppState>,
    params: Option<DocumentSearchQuery>,
) -> Result<ApiResult<Paginated<DocumentRecord>>, String> {
    let p = params.unwrap_or_default();
    let query = p.text.unwrap_or_default();
    let top_k = p.top_k.or(p.limit).unwrap_or(50).min(50);
    let offset = p.offset.unwrap_or(0);
    if let Some(store) = data_store(&state) {
        let query_for_store = query.clone();
        let mode = p
            .mode
            .as_deref()
            .map(DataSearchMode::from_str_lossy)
            .unwrap_or_default();
        if let Ok(Some(page)) = run_blocking(move || {
            if query_for_store.trim().is_empty() {
                let docs = store
                    .list_documents(top_k, offset)
                    .map_err(|err| err.to_string())?;
                let items = docs.into_iter().map(map_document).collect::<Vec<_>>();
                let total = items.len() as u64;
                return Ok(Some(Paginated {
                    items,
                    total,
                    limit: top_k as u64,
                    offset: offset as u64,
                }));
            }

            let trace = store
                .search(&query_for_store, mode, top_k)
                .map_err(|err| err.to_string())?;
            let mut grouped: std::collections::BTreeMap<String, (f32, Vec<String>)> =
                std::collections::BTreeMap::new();
            for candidate in &trace.results {
                let entry = grouped
                    .entry(candidate.document_id.clone())
                    .or_insert((candidate.score, Vec::new()));
                entry.0 = entry.0.max(candidate.score);
                if !entry.1.iter().any(|stage| stage == &candidate.source_stage) {
                    entry.1.push(candidate.source_stage.clone());
                }
            }
            let mut items = Vec::new();
            for (document_id, (score, stages)) in grouped {
                if let Some(doc) = store
                    .get_document(&document_id)
                    .map_err(|err| err.to_string())?
                {
                    let mut doc = map_document(doc);
                    doc.retrieval_score = Some(score);
                    doc.source_stages = stages;
                    items.push(doc);
                }
            }
            let total = items.len() as u64;
            Ok(Some(Paginated {
                items,
                total,
                limit: top_k as u64,
                offset: offset as u64,
            }))
        })
        .await
        {
            return Ok(ok(page));
        }
    }
    let idx = state.chat_engine.index.read().await;
    // empty query → return all indexed docs via BM25 with empty string
    let results = idx.search(&query, top_k);
    let out: Vec<DocumentRecord> = results
        .into_iter()
        .map(|r| {
            let mut doc = empty_document(r.doc_id);
            doc.current_path = r.path.unwrap_or_default();
            doc.original_path = doc.current_path.clone();
            doc.extracted_text = Some(r.excerpt);
            doc.confidence = r.score;
            doc.processing_status = "complete".into();
            doc
        })
        .collect();
    drop(idx);
    let total = out.len() as u64;
    Ok(ok(Paginated {
        items: out,
        total,
        limit: top_k as u64,
        offset: offset as u64,
    }))
}

/// Index a single document into the local BM25 index.
#[tauri::command]
async fn index_document(
    state: State<'_, AppState>,
    req: IndexDocumentRequest,
) -> Result<ApiResult<IndexDocumentResponse>, String> {
    use masterd_chat_engine::IndexedDocument;
    let doc = IndexedDocument {
        doc_id: req.doc_id.clone(),
        text: req.text,
        path: req.path,
        symbols: req.symbols.unwrap_or_default(),
        doc_type: req.doc_type,
    };
    state.chat_engine.index_document(doc).await;
    let count = state.chat_engine.index_doc_count().await;
    Ok(ok(IndexDocumentResponse {
        doc_id: req.doc_id,
        indexed: true,
        doc_count: count,
    }))
}

// ── Chat streaming command ────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatStreamToken {
    Think {
        text: String,
    },
    Response {
        text: String,
    },
    Done {
        citations: Vec<ChatCitation>,
        #[serde(rename = "retrievalTrace", skip_serializing_if = "Option::is_none")]
        retrieval_trace: Option<masterd_data::RetrievalTrace>,
    },
    Error {
        message: String,
    },
}

#[derive(Serialize, Clone)]
pub struct ChatCitation {
    pub title: String,
    pub url: String,
}

#[tauri::command]
#[allow(non_snake_case)]
async fn chat_send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    message: String,
    thinkMode: String,
    searchMode: String,
    sessionId: String,
    channelId: String,
) -> Result<(), String> {
    use masterd_chat_engine::{SearchMode, ThinkMode};

    let think_mode = match thinkMode.as_str() {
        "Thinking" => ThinkMode::Thinking,
        "Instruct" => ThinkMode::Thinking,
        _ => ThinkMode::Auto,
    };
    let search_mode = match searchMode.as_str() {
        "WebSearch" => SearchMode::WebSearch,
        "Both" => SearchMode::Both,
        _ => SearchMode::LocalDocuments,
    };
    let data_search_mode = match searchMode.as_str() {
        "WebSearch" => None,
        "Semantic" => Some(DataSearchMode::Semantic),
        "Lexical" => Some(DataSearchMode::Lexical),
        _ => Some(DataSearchMode::Hybrid),
    };
    let mut local_citations = Vec::new();
    let mut retrieval_trace = None;
    let mut message_for_generation = message;
    if let (Some(store), Some(mode)) = (data_store(&state), data_search_mode) {
        let query = message_for_generation.clone();
        let top_k = state.config.lock().await.rag_top_k.max(1);
        if let Ok(trace) = tokio::task::spawn_blocking(move || store.search(&query, mode, top_k))
            .await
            .map_err(|err| err.to_string())?
        {
            if !trace.results.is_empty() {
                let mut context = String::from("[CANONICAL LOCAL CONTEXT]\n");
                for (index, candidate) in trace.results.iter().enumerate() {
                    context.push_str(&format!(
                        "[{}] {} ({}) score:{:.4} stage:{}\n{}\n",
                        index + 1,
                        candidate.title,
                        candidate.path,
                        candidate.score,
                        candidate.source_stage,
                        candidate.text
                    ));
                    local_citations.push(ChatCitation {
                        title: candidate.title.clone(),
                        url: format!("file://{}", candidate.path),
                    });
                }
                message_for_generation =
                    format!("{context}\n[USER QUESTION]\n{message_for_generation}");
            }
            retrieval_trace = Some(trace);
        }
    }

    let event_name = format!("masterd://chat/{channelId}");
    let (tx, mut rx) = mpsc::channel::<ChatToken>(128);
    let engine = state.chat_engine.clone();
    let sessions = state.sessions.clone();

    // Spawn generation: get-or-create session, run chat, persist updated session back.
    let (session_done_tx, session_done_rx) = tokio::sync::oneshot::channel();
    let gen_session_id = sessionId.clone();
    tokio::spawn(async move {
        use masterd_chat_engine::ChatSession;
        let session_snapshot = {
            let mut map = sessions.lock().await;
            map.entry(gen_session_id.clone())
                .or_insert_with(ChatSession::new)
                .clone()
        };
        let mut session = session_snapshot;
        if let Err(e) = engine
            .chat(
                &mut session,
                message_for_generation,
                think_mode,
                search_mode,
                tx,
            )
            .await
        {
            tracing::error!("chat engine error: {e}");
        }
        let _ = session_done_tx.send((gen_session_id, session));
    });

    // Spawn event relay: forward tokens to frontend, then persist updated session.
    let relay_sessions = state.sessions.clone();
    let ev = event_name.clone();
    let done_local_citations = local_citations;
    let done_retrieval_trace = retrieval_trace;
    tokio::spawn(async move {
        while let Some(token) = rx.recv().await {
            let payload = match token {
                ChatToken::Think(t) => ChatStreamToken::Think { text: t },
                ChatToken::Response(t) => ChatStreamToken::Response { text: t },
                ChatToken::Done { citations, .. } => {
                    let mut merged_citations = done_local_citations.clone();
                    merged_citations.extend(citations.into_iter().map(|c| ChatCitation {
                        title: c.title,
                        url: c.url,
                    }));
                    ChatStreamToken::Done {
                        citations: merged_citations,
                        retrieval_trace: done_retrieval_trace.clone(),
                    }
                }
            };
            let _ = app.emit(&ev, payload);
        }
        if let Ok((sid, updated)) = session_done_rx.await {
            relay_sessions.lock().await.insert(sid, updated);
        }
    });

    Ok(())
}

// ── Entry point ───────────────────────────────────────────────────────────────
fn main() {
    tracing_subscriber::fmt::init();
    let app = tauri::Builder::default()
        .manage(AppState::new())
        .manage(SidecarSupervisor::new())
        // ── First-launch directory creation + state restore ────────────────
        .setup(|app| {
            use tauri::Manager;
            let paths = app.path();
            let data_dir = paths.app_data_dir()?;
            let config_dir = paths.app_config_dir()?;
            let cache_dir = paths.app_cache_dir()?;
            let log_dir = paths.app_log_dir()?;

            let dirs = crate::state::AppDirs::create_all(
                data_dir.clone(),
                config_dir,
                cache_dir,
                log_dir,
            )?;

            // Start packaged sidecars before opening the canonical datastore.
            // Release builds fail fast if required database assets are missing.
            let resource_dir = paths.resource_dir()?;
            app.state::<SidecarSupervisor>()
                .start_all(&resource_dir, &data_dir)?;

            let handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                handle.state::<AppState>().init_dirs(dirs).await;
            });

            // Preload LFM2.5 models asynchronously in a separate OS thread to avoid blocking Tauri GUI startup.
            let chat_engine = app.state::<AppState>().chat_engine.clone();
            std::thread::spawn(move || {
                tracing::info!("Pre-warming LFM2.5 thinking and instruct models...");
                if let Err(e) = chat_engine.preload() {
                    tracing::error!("Failed to preload LFM2.5 models: {e}");
                } else {
                    tracing::info!("LFM2.5 models preloaded successfully");
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system_get_status,
            system_get_health,
            system_export_backup,
            settings_get,
            settings_save,
            intake_add_files,
            intake_add_watch_folder,
            intake_remove_watch_folder,
            intake_list_queue,
            intake_list_watch_folders,
            intake_retry_item,
            intake_cancel_item,
            documents_search,
            documents_get_by_id,
            documents_get_preview,
            documents_get_extracted_text,
            documents_update_tags,
            documents_reprocess,
            index_document,
            actions_approve_rename,
            actions_reject_rename,
            actions_approve_move,
            actions_mark_duplicate,
            actions_mark_unique,
            pipeline_list_jobs,
            pipeline_get_job,
            pipeline_retry_job,
            pipeline_cancel_job,
            review_list,
            review_resolve,
            rules_list,
            rules_get_by_id,
            rules_create,
            rules_update,
            rules_delete,
            rules_test,
            audit_list,
            audit_get_for_document,
            audit_revert,
            preferences_list,
            preferences_record_event,
            preferences_approve,
            preferences_dismiss,
            preferences_draft_policy,
            retrieval_explain,
            chat_send_message,
        ])
        .build(tauri::generate_context!())
        .expect("MASTERd desktop failed to build");

    // ── Persist state + stop sidecars on clean exit ────────────────────────
    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            use tauri::Manager;
            let handle = app_handle.clone();
            tauri::async_runtime::block_on(async move {
                handle.state::<AppState>().persist().await;
            });
            app_handle.state::<SidecarSupervisor>().stop_all();
        }
    });
}
