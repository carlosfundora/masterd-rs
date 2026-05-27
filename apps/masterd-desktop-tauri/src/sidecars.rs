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
//!   <resource_dir>/services/colbert-service/.venv/bin/python
//!   <resource_dir>/services/jina-service/.venv/bin/python
//!   <resource_dir>/services/model2vec-service/start.sh
//!
//! Data directories are provisioned inside the app's data_dir:
//!   <data_dir>/meilisearch/
//!   <data_dir>/valkey/
//!   <data_dir>/falkordb/

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tracing::{info, warn};

const RELEASE_REQUIRES_ASSETS: bool = !cfg!(debug_assertions);

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

#[cfg(debug_assertions)]
fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let services = dir.join("services");
        if services.join("colbert-service").exists() || services.join("model2vec-service").exists()
        {
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

fn resource_asset(
    resource_dir: &Path,
    relative: impl AsRef<Path>,
    required: bool,
) -> Result<Option<PathBuf>> {
    trusted_asset(resource_dir, &resource_dir.join(relative), required)
}

fn trusted_asset(root: &Path, path: &Path, required: bool) -> Result<Option<PathBuf>> {
    trusted_asset_inner(root, path, required, false)
}

fn trusted_dev_interpreter(root: &Path, path: &Path, required: bool) -> Result<Option<PathBuf>> {
    trusted_asset_inner(root, path, required, cfg!(debug_assertions))
}

fn trusted_asset_inner(
    root: &Path,
    path: &Path,
    required: bool,
    allow_external_symlink_target: bool,
) -> Result<Option<PathBuf>> {
    if !path.exists() {
        if required {
            anyhow::bail!("required sidecar asset is missing: {}", path.display());
        }
        warn!("sidecar asset not found at {}, skipping", path.display());
        return Ok(None);
    }

    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("canonicalize sidecar root {}", root.display()))?;
    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("canonicalize sidecar asset {}", path.display()))?;
    let checked_path = if allow_external_symlink_target {
        path.to_path_buf()
    } else {
        canonical_path.clone()
    };
    if !checked_path.starts_with(&canonical_root) {
        anyhow::bail!(
            "sidecar asset {} resolves outside trusted root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }

    #[cfg(unix)]
    {
        let mut current = Some(checked_path.as_path());
        while let Some(path) = current {
            let mode = std::fs::metadata(path)?.permissions().mode();
            if mode & 0o002 != 0 {
                anyhow::bail!("sidecar asset path is world-writable: {}", path.display());
            }
            if path == canonical_root {
                break;
            }
            current = path.parent();
        }
    }

    Ok(Some(canonical_path))
}

impl SidecarSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start all bundled sidecars. Safe to call multiple times — already-running
    /// sidecars are skipped. Call from the Tauri `.setup()` hook.
    pub fn start_all(&self, resource_dir: &Path, data_dir: &Path) -> Result<()> {
        self.start_meilisearch(resource_dir, data_dir)?;
        self.start_valkey(resource_dir, data_dir)?;
        self.start_falkordb(resource_dir, data_dir)?;
        self.start_searxng(resource_dir)?;
        self.start_embedding_services(resource_dir)?;
        Ok(())
    }

    fn start_embedding_services(&self, resource_dir: &Path) -> Result<()> {
        let services_dir = resource_dir.join("services");
        self.start_python_service(
            "colbert-service",
            &services_dir.join("colbert-service"),
            11450,
            resource_dir,
        )?;
        self.start_python_service(
            "jina-service",
            &services_dir.join("jina-service"),
            11447,
            resource_dir,
        )?;
        self.start_python_service(
            "jina-v5-service",
            &services_dir.join("jina-v5-service"),
            11502,
            resource_dir,
        )?;
        self.start_model2vec_service(resource_dir)?;
        Ok(())
    }

    fn start_python_service(
        &self,
        name: &'static str,
        service_dir: &Path,
        port: u16,
        trust_root: &Path,
    ) -> Result<()> {
        let procs = self.procs.lock().unwrap();
        if procs.iter().any(|p| p.name == name) {
            return Ok(());
        }
        drop(procs);

        let Some(bin) = trusted_dev_interpreter(
            trust_root,
            &service_dir.join(".venv").join("bin").join("python"),
            RELEASE_REQUIRES_ASSETS,
        )?
        else {
            #[cfg(debug_assertions)]
            if let Some(root) = find_workspace_root() {
                let workspace_service = root.join("services").join(name);
                return self.start_python_service(name, &workspace_service, port, &root);
            }
            return Ok(());
        };
        let Some(script) = trusted_asset(
            trust_root,
            &service_dir.join("server.py"),
            RELEASE_REQUIRES_ASSETS,
        )?
        else {
            #[cfg(debug_assertions)]
            if let Some(root) = find_workspace_root() {
                let workspace_service = root.join("services").join(name);
                return self.start_python_service(name, &workspace_service, port, &root);
            }
            return Ok(());
        };

        let child = Command::new(&bin)
            .arg(&script)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(c) => {
                info!("{name} started on port {port}");
                self.procs
                    .lock()
                    .unwrap()
                    .push(ManagedProcess { name, child: c });
            }
            Err(e) => {
                if RELEASE_REQUIRES_ASSETS {
                    anyhow::bail!("failed to start {name}: {e}");
                }
                warn!("failed to start {name}: {e}");
            }
        }
        Ok(())
    }

    fn start_model2vec_service(&self, resource_dir: &Path) -> Result<()> {
        let procs = self.procs.lock().unwrap();
        if procs.iter().any(|p| p.name == "model2vec-service") {
            return Ok(());
        }
        drop(procs);

        let bin = match resource_asset(
            resource_dir,
            "bin/model2vec-service",
            RELEASE_REQUIRES_ASSETS,
        )? {
            Some(bin) => bin,
            None => {
                #[cfg(debug_assertions)]
                if let Some(root) = find_workspace_root() {
                    let debug_bin = root.join("target").join("debug").join("model2vec-service");
                    if debug_bin.exists() {
                        debug_bin
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
                #[cfg(not(debug_assertions))]
                return Ok(());
            }
        };

        let child = Command::new(&bin)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(c) => {
                info!("model2vec-service started on 127.0.0.1:11448");
                self.procs.lock().unwrap().push(ManagedProcess {
                    name: "model2vec-service",
                    child: c,
                });
            }
            Err(e) => {
                if RELEASE_REQUIRES_ASSETS {
                    anyhow::bail!("failed to start model2vec-service: {e}");
                }
                warn!("failed to start model2vec-service: {e}");
            }
        }
        Ok(())
    }

    fn start_searxng(&self, resource_dir: &Path) -> Result<()> {
        let procs = self.procs.lock().unwrap();
        if procs.iter().any(|p| p.name == "searxng-service") {
            return Ok(());
        }
        drop(procs);

        let service_dir = resource_dir.join("services").join("searxng-service");
        let bin = service_dir.join(".venv").join("bin").join("python");
        let script = service_dir.join("start.py");
        let settings = service_dir.join("settings.yml");
        let (bin, script, settings) = match (
            trusted_dev_interpreter(resource_dir, &bin, RELEASE_REQUIRES_ASSETS)?,
            trusted_asset(resource_dir, &script, RELEASE_REQUIRES_ASSETS)?,
            trusted_asset(resource_dir, &settings, RELEASE_REQUIRES_ASSETS)?,
        ) {
            (Some(bin), Some(script), Some(settings)) => (bin, script, settings),
            _ => {
                #[cfg(debug_assertions)]
                if let Some(root) = find_workspace_root() {
                    let workspace_service = root.join("services").join("searxng-service");
                    let bin = workspace_service.join(".venv").join("bin").join("python");
                    let script = workspace_service.join("start.py");
                    let settings = workspace_service.join("settings.yml");
                    match (
                        trusted_dev_interpreter(&root, &bin, false)?,
                        trusted_asset(&root, &script, false)?,
                        trusted_asset(&root, &settings, false)?,
                    ) {
                        (Some(bin), Some(script), Some(settings)) => (bin, script, settings),
                        _ => return Ok(()),
                    }
                } else {
                    return Ok(());
                }
                #[cfg(not(debug_assertions))]
                return Ok(());
            }
        };

        let child = Command::new(&bin)
            .arg(&script)
            .env("SEARXNG_SETTINGS_PATH", settings)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(c) => {
                info!("searxng-service started on 127.0.0.1:9265");
                self.procs.lock().unwrap().push(ManagedProcess {
                    name: "searxng-service",
                    child: c,
                });
            }
            Err(e) => {
                if RELEASE_REQUIRES_ASSETS {
                    anyhow::bail!("failed to start searxng-service: {e}");
                }
                warn!("failed to start searxng-service: {e}");
            }
        }
        Ok(())
    }

    fn start_meilisearch(&self, resource_dir: &Path, data_dir: &Path) -> Result<()> {
        let Some(bin) = resource_asset(resource_dir, "bin/meilisearch", RELEASE_REQUIRES_ASSETS)?
        else {
            return Ok(());
        };

        let db_path = data_dir.join("meilisearch");
        std::fs::create_dir_all(&db_path)
            .with_context(|| format!("create meilisearch data dir {}", db_path.display()))?;

        self.spawn(
            "meilisearch",
            &bin,
            &[
                "--db-path",
                &db_path.to_string_lossy(),
                "--no-analytics",
                "--http-addr",
                "127.0.0.1:7700",
            ],
        )
        .context("failed to start meilisearch")?;
        info!("meilisearch started on 127.0.0.1:7700");
        Ok(())
    }

    fn start_valkey(&self, resource_dir: &Path, data_dir: &Path) -> Result<()> {
        let Some(bin) = resource_asset(resource_dir, "bin/valkey-server", RELEASE_REQUIRES_ASSETS)?
        else {
            return Ok(());
        };

        let db_path = data_dir.join("valkey");
        std::fs::create_dir_all(&db_path)
            .with_context(|| format!("create valkey data dir {}", db_path.display()))?;

        let args: Vec<String> = vec![
            "--port".into(),
            "6399".into(),
            "--dir".into(),
            db_path.to_string_lossy().into_owned(),
            "--save".into(),
            "60 1".into(),
        ];

        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.spawn("valkey-server", &bin, &arg_refs)
            .context("failed to start valkey-server")?;
        info!("valkey-server started on 127.0.0.1:6399");
        Ok(())
    }

    fn start_falkordb(&self, resource_dir: &Path, data_dir: &Path) -> Result<()> {
        let Some(bin) =
            resource_asset(resource_dir, "bin/falkordb-server", RELEASE_REQUIRES_ASSETS)?
        else {
            return Ok(());
        };
        let Some(module_path) =
            resource_asset(resource_dir, "modules/falkordb.so", RELEASE_REQUIRES_ASSETS)?
        else {
            return Ok(());
        };

        let db_path = data_dir.join("falkordb");
        std::fs::create_dir_all(&db_path)
            .with_context(|| format!("create falkordb data dir {}", db_path.display()))?;

        let args: Vec<String> = vec![
            "--port".into(),
            "6380".into(),
            "--dir".into(),
            db_path.to_string_lossy().into_owned(),
            "--save".into(),
            "60 1".into(),
            "--loadmodule".into(),
            module_path.to_string_lossy().into_owned(),
        ];
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.spawn("falkordb-server", &bin, &arg_refs)
            .context("failed to start falkordb-server")?;
        info!("falkordb-server started on 127.0.0.1:6380");
        Ok(())
    }

    fn spawn(&self, name: &'static str, bin: &PathBuf, args: &[&str]) -> Result<()> {
        let child = Command::new(bin)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn {name}"))?;

        self.procs
            .lock()
            .unwrap()
            .push(ManagedProcess { name, child });
        Ok(())
    }

    /// Gracefully stop all sidecars. Called from `RunEvent::Exit`.
    pub fn stop_all(&self) {
        let mut procs = self.procs.lock().unwrap();
        procs.clear(); // triggers Drop on each ManagedProcess → kill()
    }
}
