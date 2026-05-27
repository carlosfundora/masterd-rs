//! Sidecar process supervisor.
//!
//! Manages the lifecycle of external binaries (meilisearch, valkey-server)
//! that are bundled with the app. On startup, the supervisor resolves each
//! binary from the Tauri resource directory and spawns it. On Drop (app exit),
//! all child processes are killed.
//!
//! Binaries are expected at:
//!   <resource_dir>/bin/meilisearch
//!   <resource_dir>/bin/valkey-server
//!   <resource_dir>/bin/falkordb-server
//!   <resource_dir>/modules/falkordb.so
//!   <resource_dir>/services/searxng-service/.venv/bin/python
//!   <workspace_root>/services/model2vec-service/start.sh
//!
//! Data directories are provisioned inside the app's data_dir:
//!   <data_dir>/meilisearch/
//!   <data_dir>/valkey/
//!   <data_dir>/falkordb/

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tracing::{info, warn};

#[derive(Debug)]
struct ManagedProcess {
    name: &'static str,
    child: Child,
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        if let Err(e) = self.child.kill() {
            warn!(sidecar = self.name, error = %e, "failed to kill sidecar on drop");
        } else {
            info!(sidecar = self.name, "stopped");
        }
    }
}

/// Holds all running sidecar processes and kills them on drop.
#[derive(Debug, Default)]
pub struct SidecarSupervisor {
    procs: Arc<Mutex<Vec<ManagedProcess>>>,
}

fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let services = dir.join("services");
        if services.join("colbert-service").exists() || services.join("model2vec-service").exists() {
            return Some(dir);
        }
        if let Some(parent) = dir.parent() {
            dir = parent.to_path_buf();
        } else {
            break;
        }
    }
    None
}

impl SidecarSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start all bundled sidecars. Safe to call multiple times — already-running
    /// sidecars are skipped. Call from the Tauri `.setup()` hook.
    pub fn start_all(&self, resource_dir: &Path, data_dir: &Path) {
        self.start_meilisearch(resource_dir, data_dir);
        self.start_valkey(resource_dir, data_dir);
        self.start_falkordb(resource_dir, data_dir);
        self.start_searxng(resource_dir);
        self.start_embedding_services(resource_dir);
    }

    fn start_embedding_services(&self, resource_dir: &Path) {
        if let Some(root) = find_workspace_root() {
            info!("found workspace root at {}, starting embedding services...", root.display());
            self.start_python_service("colbert-service", &root, 11450);
            self.start_python_service("jina-service", &root, 11447);
            self.start_model2vec_service(resource_dir, &root);
        } else {
            warn!("could not locate workspace root, embedding services will not be started automatically");
        }
    }

    fn start_python_service(&self, name: &'static str, root: &Path, port: u16) {
        let procs = self.procs.lock().unwrap();
        if procs.iter().any(|p| p.name == name) {
            return;
        }
        drop(procs);

        let service_dir = root.join("services").join(name);
        let bin = service_dir.join(".venv").join("bin").join("python");
        let script = service_dir.join("server.py");

        if !bin.exists() {
            warn!("python binary not found at {}, skipping {name}", bin.display());
            return;
        }
        if !script.exists() {
            warn!("server script not found at {}, skipping {name}", script.display());
            return;
        }

        let child = Command::new(&bin)
            .arg(&script)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(c) => {
                info!("{name} started on port {port}");
                self.procs.lock().unwrap().push(ManagedProcess { name, child: c });
            }
            Err(e) => {
                warn!("failed to start {name}: {e}");
            }
        }
    }

    fn start_model2vec_service(&self, resource_dir: &Path, root: &Path) {
        let procs = self.procs.lock().unwrap();
        if procs.iter().any(|p| p.name == "model2vec-service") {
            return;
        }
        drop(procs);

        let packaged = resource_dir.join("services").join("model2vec-service");
        let workspace = root.join("services").join("model2vec-service");
        let service_dir = if packaged.join("start.sh").exists() {
            packaged
        } else if workspace.join("start.sh").exists() {
            workspace
        } else {
            info!("model2vec-service not found in resources or workspace, skipping");
            return;
        };

        let script = service_dir.join("start.sh");
        let child = Command::new("bash")
            .arg(&script)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(c) => {
                info!("model2vec-service started on 127.0.0.1:11448");
                self.procs.lock().unwrap().push(ManagedProcess { name: "model2vec-service", child: c });
            }
            Err(e) => {
                warn!("failed to start model2vec-service: {e}");
            }
        }
    }

    fn start_searxng(&self, resource_dir: &Path) {
        let procs = self.procs.lock().unwrap();
        if procs.iter().any(|p| p.name == "searxng-service") {
            return;
        }
        drop(procs);

        let packaged = resource_dir.join("services").join("searxng-service");
        let workspace = find_workspace_root().map(|root| root.join("services").join("searxng-service"));
        let service_dir = if packaged.join("start.py").exists() {
            packaged
        } else if let Some(workspace) = workspace {
            workspace
        } else {
            info!("searxng-service not found in resources or workspace, skipping");
            return;
        };

        let bin = service_dir.join(".venv").join("bin").join("python");
        let script = service_dir.join("start.py");
        let settings = service_dir.join("settings.yml");
        if !bin.exists() {
            warn!("SearXNG python binary not found at {}, skipping web search sidecar", bin.display());
            return;
        }
        if !script.exists() {
            warn!("SearXNG launcher not found at {}, skipping web search sidecar", script.display());
            return;
        }

        let child = Command::new(&bin)
            .arg(&script)
            .env("SEARXNG_SETTINGS_PATH", settings)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(c) => {
                info!("searxng-service started on 127.0.0.1:9265");
                self.procs.lock().unwrap().push(ManagedProcess { name: "searxng-service", child: c });
            }
            Err(e) => {
                warn!("failed to start searxng-service: {e}");
            }
        }
    }

    fn start_meilisearch(&self, resource_dir: &Path, data_dir: &Path) {
        let bin = resource_dir.join("bin").join("meilisearch");
        if !bin.exists() {
            info!("meilisearch binary not found at {}, skipping", bin.display());
            return;
        }

        let db_path = data_dir.join("meilisearch");
        if let Err(e) = std::fs::create_dir_all(&db_path) {
            warn!("could not create meilisearch data dir: {e}");
            return;
        }

        match self.spawn("meilisearch", &bin, &[
            "--db-path", &db_path.to_string_lossy(),
            "--no-analytics",
            "--http-addr", "127.0.0.1:7700",
        ]) {
            Ok(()) => info!("meilisearch started on 127.0.0.1:7700"),
            Err(e) => warn!("failed to start meilisearch: {e}"),
        }
    }

    fn start_valkey(&self, resource_dir: &Path, data_dir: &Path) {
        let bin = resource_dir.join("bin").join("valkey-server");
        if !bin.exists() {
            info!("valkey-server binary not found at {}, skipping", bin.display());
            return;
        }

        let db_path = data_dir.join("valkey");
        if let Err(e) = std::fs::create_dir_all(&db_path) {
            warn!("could not create valkey data dir: {e}");
            return;
        }

        let args: Vec<String> = vec![
            "--port".into(), "6399".into(),
            "--dir".into(), db_path.to_string_lossy().into_owned(),
            "--save".into(), "60 1".into(),
        ];

        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        match self.spawn("valkey-server", &bin, &arg_refs) {
            Ok(()) => info!("valkey-server started on 127.0.0.1:6399"),
            Err(e) => warn!("failed to start valkey-server: {e}"),
        }
    }

    fn start_falkordb(&self, resource_dir: &Path, data_dir: &Path) {
        let bin = resource_dir.join("bin").join("falkordb-server");
        if !bin.exists() {
            info!("falkordb-server binary not found at {}, skipping", bin.display());
            return;
        }

        let module_path = resource_dir.join("modules").join("falkordb.so");
        let module_path = module_path.canonicalize().unwrap_or(module_path);
        if !module_path.exists() {
            warn!("falkordb module not found at {}, skipping graph DB", module_path.display());
            return;
        }

        let db_path = data_dir.join("falkordb");
        if let Err(e) = std::fs::create_dir_all(&db_path) {
            warn!("could not create falkordb data dir: {e}");
            return;
        }

        let args: Vec<String> = vec![
            "--port".into(), "6380".into(),
            "--dir".into(), db_path.to_string_lossy().into_owned(),
            "--save".into(), "60 1".into(),
            "--loadmodule".into(), module_path.to_string_lossy().into_owned(),
        ];
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        match self.spawn("falkordb-server", &bin, &arg_refs) {
            Ok(()) => info!("falkordb-server started on 127.0.0.1:6380"),
            Err(e) => warn!("failed to start falkordb-server: {e}"),
        }
    }

    fn spawn(&self, name: &'static str, bin: &PathBuf, args: &[&str]) -> Result<()> {
        let child = Command::new(bin)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {name}"))?;

        self.procs.lock().unwrap().push(ManagedProcess { name, child });
        Ok(())
    }

    /// Gracefully stop all sidecars. Called from `RunEvent::Exit`.
    pub fn stop_all(&self) {
        let mut procs = self.procs.lock().unwrap();
        procs.clear(); // triggers Drop on each ManagedProcess → kill()
    }
}
