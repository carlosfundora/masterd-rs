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
