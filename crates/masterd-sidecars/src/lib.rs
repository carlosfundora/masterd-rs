use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// Embedded default sidecar config — no file path needed at runtime.
static DEFAULT_SIDECARS_TOML: &str = include_str!("../assets/default_sidecars.toml");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    Sidecar,
    InProcess,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceSpec {
    pub name: String,
    pub mode: ServiceMode,
    pub binary: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SidecarConfig {
    pub services: Vec<ServiceSpec>,
}

#[derive(Debug, Error)]
pub enum FoundationError {
    #[error("service `{0}` cannot be truly embedded in a single binary; use sidecar mode")]
    NotSingleBinaryEmbeddable(String),
}

impl SidecarConfig {
    /// Load from embedded compile-time bytes — the canonical production constructor.
    pub fn embedded() -> Result<Self> {
        toml::from_str(DEFAULT_SIDECARS_TOML).context("failed to parse embedded sidecars config")
    }

    /// Load from an external path (for user overrides at runtime).
    pub fn from_path(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read sidecar config at {}", path.display()))?;
        let config = toml::from_str(&raw).context("failed to parse sidecar config TOML")?;
        Ok(config)
    }

    pub fn validate_foundation(&self) -> std::result::Result<(), FoundationError> {
        for service in &self.services {
            if matches!(
                service.name.as_str(),
                "meilisearch" | "valkey" | "falkordb-module"
            ) && service.mode == ServiceMode::InProcess
            {
                return Err(FoundationError::NotSingleBinaryEmbeddable(
                    service.name.clone(),
                ));
            }
        }
        Ok(())
    }
}
