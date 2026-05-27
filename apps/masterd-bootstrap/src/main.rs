use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use masterd_core::ProjectFoundation;
use masterd_prompt_core::PromptRegistry;
use masterd_runtime_tune::{TunerPolicy, ensure_startup_profile_embedded};
use masterd_sidecars::SidecarConfig;
use serde::Deserialize;
use tracing::info;

// Default pipeline config embedded at compile time.
static DEFAULT_PIPELINE_TOML: &str = include_str!("../assets/default_pipeline.toml");

#[derive(Debug, Parser)]
#[command(name = "masterd-bootstrap")]
#[command(about = "Bootstrap and validate MASTERd Rust foundation")]
struct Args {
    /// Override sidecar config with an external file (uses embedded default when omitted).
    #[arg(long)]
    config: Option<PathBuf>,

    /// Override pipeline config with an external file (uses embedded default when omitted).
    #[arg(long)]
    pipeline_config: Option<PathBuf>,

    #[arg(long, default_value_t = true)]
    tune_startup: bool,

    #[arg(long, default_value_t = true)]
    allow_tune_downloads: bool,
}

#[derive(Debug, Deserialize)]
struct PipelineConfig {
    database: DatabaseConfig,
    search: SearchConfig,
    cache: CacheConfig,
    frontend: FrontendConfig,
    embeddings: EmbeddingConfig,
}

#[derive(Debug, Deserialize)]
struct DatabaseConfig {
    engine: String,
    path: String,
}

#[derive(Debug, Deserialize)]
struct SearchConfig {
    vector_authority: String,
    colbert_model: String,
    colbert_device: String,
    lexical_engine: String,
}

#[derive(Debug, Deserialize)]
struct CacheConfig {
    hot_cache: String,
    dedup_mode: String,
}

#[derive(Debug, Deserialize)]
struct FrontendConfig {
    target: String,
}

#[derive(Debug, Deserialize)]
struct EmbeddingConfig {
    jina_model: String,
    jina_runtime: String,
    multimodal_optional: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let foundation = ProjectFoundation::rust_first();
    let prompts = PromptRegistry::from_masterd_sources();

    // Use embedded config unless caller provides an override path.
    let sidecars = match &args.config {
        Some(path) => SidecarConfig::from_path(path)?,
        None => SidecarConfig::embedded()?,
    };
    sidecars.validate_foundation()?;

    let pipeline_raw = match &args.pipeline_config {
        Some(path) => std::fs::read_to_string(path)?,
        None => DEFAULT_PIPELINE_TOML.to_string(),
    };
    let pipeline: PipelineConfig = toml::from_str(&pipeline_raw)?;

    if args.tune_startup {
        let policy = TunerPolicy {
            time_budget_secs: 600,
            strictness: "balanced".to_string(),
            allow_optional_downloads: args.allow_tune_downloads,
        };
        // Uses embedded AMD profiles + kernel manifest — no config/ directory needed.
        let lock = ensure_startup_profile_embedded(
            &policy,
            PathBuf::from("data/runtime_profile_lock.json").as_path(),
        )?;
        println!(
            "Runtime profile lock: {} [{}]",
            lock.selected_profile, lock.backend
        );
    }

    info!(project = %foundation.name, "Rust foundation initialized");
    println!("MASTERd Rust foundation: OK");
    println!("Capabilities: {}", foundation.capabilities.len());
    println!("Prompt identity: {}", prompts.identity.display_name);
    println!("Prompt avatars loaded: {}", prompts.avatars.len());
    println!("Sidecar services configured: {}", sidecars.services.len());
    println!(
        "Canonical DB: {} @ {}",
        pipeline.database.engine, pipeline.database.path
    );
    println!(
        "Search: {} on {} + lexical {} ({})",
        pipeline.search.colbert_model,
        pipeline.search.colbert_device,
        pipeline.search.lexical_engine,
        pipeline.search.vector_authority
    );
    println!(
        "Cache/dedup: {} + {}",
        pipeline.cache.hot_cache, pipeline.cache.dedup_mode
    );
    println!("Frontend target: {}", pipeline.frontend.target);
    println!(
        "Embeddings: {} [{}], multimodal optional={}",
        pipeline.embeddings.jina_model,
        pipeline.embeddings.jina_runtime,
        pipeline.embeddings.multimodal_optional
    );
    Ok(())
}
