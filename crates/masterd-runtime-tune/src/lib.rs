use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::Utc;
use masterd_embed_engine::{EmbeddedEngine, LocalEmbeddingStack};
use serde::{Deserialize, Serialize};

// ── Compile-time embedded assets ────────────────────────────────────────────

static PROFILE_AMD_BALANCED: &str = include_str!("../assets/amd-balanced.toml");
static PROFILE_AMD_ROCM_AGGRESSIVE: &str = include_str!("../assets/amd-rocm-aggressive.toml");
static PROFILE_SAFE_DIRECT: &str = include_str!("../assets/safe-direct.toml");
static KERNEL_MANIFEST_BYTES: &str = include_str!("../assets/kernel_manifest.toml");

/// Load the kernel manifest from embedded bytes — no file path needed.
pub fn load_kernel_manifest_embedded() -> Result<KernelManifest> {
    Ok(toml::from_str(KERNEL_MANIFEST_BYTES)?)
}

/// Load all bundled AMD/safe profiles from embedded bytes — no directory scan needed.
pub fn load_profiles_embedded() -> Result<Vec<RuntimeProfile>> {
    let sources = [
        ("amd-balanced", PROFILE_AMD_BALANCED),
        ("amd-rocm-aggressive", PROFILE_AMD_ROCM_AGGRESSIVE),
        ("safe-direct", PROFILE_SAFE_DIRECT),
    ];
    let mut profiles = Vec::new();
    for (name, raw) in &sources {
        let profile: RuntimeProfile = toml::from_str(raw)
            .with_context(|| format!("failed parsing embedded profile '{name}'"))?;
        profiles.push(profile);
    }
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapability {
    pub cpu_vendor: String,
    pub cpu_model: String,
    pub total_mem_mb: u64,
    pub gpu_vendor: Option<String>,
    pub gpu_arch: Option<String>,
    pub rocm_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeArgs {
    pub radix: bool,
    pub flash_attention: bool,
    pub turboquant: bool,
    pub rotorquant: bool,
    pub triton: bool,
    pub aiter: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProfile {
    pub name: String,
    pub backend: String,
    pub required: Vec<String>,
    pub args: RuntimeArgs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelPack {
    pub id: String,
    pub source_path: String,
    pub required: Vec<String>,
    pub download_url: Option<String>,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelManifest {
    pub version: String,
    pub packs: Vec<KernelPack>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunerPolicy {
    pub time_budget_secs: u64,
    pub strictness: String,
    pub allow_optional_downloads: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredProfile {
    pub profile_name: String,
    pub score: f64,
    pub elapsed_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileLock {
    pub selected_profile: String,
    pub backend: String,
    pub capability_snapshot: RuntimeCapability,
    pub scored: Vec<ScoredProfile>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningResult {
    pub selected_profile: RuntimeProfile,
    pub scored: Vec<ScoredProfile>,
}

pub fn probe_runtime() -> Result<RuntimeCapability> {
    let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let meminfo = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let cpu_vendor =
        parse_cpu_field(&cpuinfo, "vendor_id").unwrap_or_else(|| "unknown".to_string());
    let cpu_model =
        parse_cpu_field(&cpuinfo, "model name").unwrap_or_else(|| "unknown".to_string());
    let total_mem_mb = parse_mem_total_mb(&meminfo).unwrap_or(0);

    let lspci = std::process::Command::new("sh")
        .arg("-lc")
        .arg("lspci 2>/dev/null || true")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let gpu_vendor = if lspci.to_ascii_lowercase().contains("amd/ati")
        || lspci
            .to_ascii_lowercase()
            .contains("advanced micro devices")
    {
        Some("amd".to_string())
    } else if lspci.to_ascii_lowercase().contains("nvidia") {
        Some("nvidia".to_string())
    } else {
        None
    };
    let gpu_arch = detect_gpu_arch_hint(&lspci);

    let rocm_available = Path::new("/opt/rocm/bin/rocminfo").exists()
        || Path::new("/dev/kfd").exists()
        || std::env::var("HIP_VISIBLE_DEVICES").is_ok();

    Ok(RuntimeCapability {
        cpu_vendor,
        cpu_model,
        total_mem_mb,
        gpu_vendor,
        gpu_arch,
        rocm_available,
    })
}

pub fn load_kernel_manifest(path: &Path) -> Result<KernelManifest> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed reading kernel manifest {}", path.display()))?;
    Ok(toml::from_str(&raw)?)
}

pub fn load_profiles(profile_dir: &Path) -> Result<Vec<RuntimeProfile>> {
    let mut profiles = Vec::new();
    for entry in fs::read_dir(profile_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let raw = fs::read_to_string(&path)?;
        let profile: RuntimeProfile = toml::from_str(&raw)
            .with_context(|| format!("failed parsing profile {}", path.display()))?;
        profiles.push(profile);
    }
    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(profiles)
}

/// Run startup profile selection using only embedded assets — no directory paths needed.
pub fn ensure_startup_profile_embedded(
    policy: &TunerPolicy,
    lock_path: &Path,
) -> Result<ProfileLock> {
    let capability = probe_runtime()?;
    let profiles = load_profiles_embedded()?;
    if let Some(lock) = load_lock(lock_path)? {
        if profile_compatible(&lock.selected_profile, &capability, &profiles) {
            return Ok(lock);
        }
    }
    let tuned = autotune(policy, &capability, &profiles)?;
    let lock = write_lock(lock_path, &tuned, &capability)?;
    Ok(lock)
}

/// Legacy path-based version kept for compatibility with external callers.
pub fn ensure_startup_profile(
    policy: &TunerPolicy,
    profiles_dir: &Path,
    _manifest: &KernelManifest,
    lock_path: &Path,
) -> Result<ProfileLock> {
    let capability = probe_runtime()?;
    let profiles = load_profiles(profiles_dir)?;
    if let Some(lock) = load_lock(lock_path)? {
        if profile_compatible(&lock.selected_profile, &capability, &profiles) {
            return Ok(lock);
        }
    }
    let tuned = autotune(policy, &capability, &profiles)?;
    let lock = write_lock(lock_path, &tuned, &capability)?;
    Ok(lock)
}

pub fn autotune(
    policy: &TunerPolicy,
    capability: &RuntimeCapability,
    profiles: &[RuntimeProfile],
) -> Result<TuningResult> {
    let candidates: Vec<RuntimeProfile> = profiles
        .iter()
        .filter(|p| supports_profile(p, capability))
        .cloned()
        .collect();
    if candidates.is_empty() {
        anyhow::bail!("no compatible runtime profiles found for this machine");
    }

    let start = Instant::now();
    let mut scored = Vec::new();
    let mut best: Option<(RuntimeProfile, f64)> = None;

    for profile in candidates {
        if start.elapsed().as_secs() >= policy.time_budget_secs {
            break;
        }
        let score = score_profile(&profile, capability, policy)?;
        scored.push(ScoredProfile {
            profile_name: profile.name.clone(),
            score,
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        });
        match &best {
            Some((_, best_score)) if *best_score >= score => {}
            _ => best = Some((profile.clone(), score)),
        }
    }

    let selected_profile = if let Some((profile, _)) = best {
        profile
    } else {
        profiles
            .iter()
            .find(|p| p.name == "safe-direct")
            .cloned()
            .or_else(|| profiles.first().cloned())
            .context("missing fallback profile")?
    };

    Ok(TuningResult {
        selected_profile,
        scored,
    })
}

pub fn write_lock(
    path: &Path,
    tuned: &TuningResult,
    capability: &RuntimeCapability,
) -> Result<ProfileLock> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock = ProfileLock {
        selected_profile: tuned.selected_profile.name.clone(),
        backend: tuned.selected_profile.backend.clone(),
        capability_snapshot: capability.clone(),
        scored: tuned.scored.clone(),
        created_at: Utc::now().to_rfc3339(),
    };
    fs::write(path, serde_json::to_vec_pretty(&lock)?)?;
    Ok(lock)
}

pub fn load_lock(path: &Path) -> Result<Option<ProfileLock>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&raw)?))
}

pub fn write_tuning_report(
    path: &Path,
    lock: &ProfileLock,
    manifest: &KernelManifest,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let report = serde_json::json!({
        "ts": Utc::now().to_rfc3339(),
        "selected_profile": lock.selected_profile,
        "backend": lock.backend,
        "capability": lock.capability_snapshot,
        "scores": lock.scored,
        "manifest_version": manifest.version,
        "pack_count": manifest.packs.len(),
    });
    fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

pub fn fetch_optional_packs(
    manifest: &KernelManifest,
    capability: &RuntimeCapability,
    cache_dir: &Path,
) -> Result<Vec<PathBuf>> {
    fs::create_dir_all(cache_dir)?;
    let tags = capability_tags(capability);
    let mut pulled = Vec::new();
    for pack in &manifest.packs {
        let required: HashSet<String> = pack.required.iter().cloned().collect();
        if !required.is_subset(&tags) {
            continue;
        }
        if let Some(url) = &pack.download_url {
            let stamp = cache_dir.join(format!("{}.downloaded.txt", pack.id));
            fs::write(
                &stamp,
                format!(
                    "source={url}\nchecksum={}\n",
                    pack.checksum.clone().unwrap_or_default()
                ),
            )?;
            pulled.push(stamp);
        }
    }
    Ok(pulled)
}

fn score_profile(
    profile: &RuntimeProfile,
    capability: &RuntimeCapability,
    policy: &TunerPolicy,
) -> Result<f64> {
    let mut score = 0.0;
    if profile.backend == "direct" {
        let engine = EmbeddedEngine::new(LocalEmbeddingStack::from_env())?;
        let bench = engine.bench_embed_jina("MASTERd tune profile sample", 16)?;
        score += bench.estimated_tokens_per_sec;
    } else {
        score += 1000.0;
    }
    if profile.args.aiter {
        score += 250.0;
    }
    if profile.args.triton {
        score += 150.0;
    }
    if profile.args.flash_attention {
        score += 100.0;
    }
    if profile.args.turboquant || profile.args.rotorquant {
        score += 90.0;
    }
    if !capability.rocm_available {
        score -= 400.0;
    }
    if policy.strictness == "safe_only" && profile.name != "safe-direct" {
        score -= 300.0;
    }
    Ok(score)
}

fn supports_profile(profile: &RuntimeProfile, capability: &RuntimeCapability) -> bool {
    let tags = capability_tags(capability);
    let required: HashSet<String> = profile.required.iter().cloned().collect();
    required.is_subset(&tags)
}

fn profile_compatible(
    selected: &str,
    capability: &RuntimeCapability,
    profiles: &[RuntimeProfile],
) -> bool {
    profiles
        .iter()
        .find(|p| p.name == selected)
        .map(|p| supports_profile(p, capability))
        .unwrap_or(false)
}

fn capability_tags(capability: &RuntimeCapability) -> HashSet<String> {
    let mut tags = HashSet::new();
    tags.insert("cpu".to_string());
    if capability.rocm_available {
        tags.insert("rocm".to_string());
    }
    if capability.gpu_vendor.as_deref() == Some("amd") {
        tags.insert("gpu_amd".to_string());
    }
    if capability.total_mem_mb >= 16_000 {
        tags.insert("ram_16g".to_string());
    }
    if capability.total_mem_mb >= 32_000 {
        tags.insert("ram_32g".to_string());
    }
    tags
}

fn parse_cpu_field(cpuinfo: &str, key: &str) -> Option<String> {
    cpuinfo.lines().find_map(|line| {
        let (k, v) = line.split_once(':')?;
        if k.trim() == key {
            Some(v.trim().to_string())
        } else {
            None
        }
    })
}

fn parse_mem_total_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb = line
        .split_whitespace()
        .nth(1)
        .and_then(|v| v.parse::<u64>().ok())?;
    Some(kb / 1024)
}

fn detect_gpu_arch_hint(lspci: &str) -> Option<String> {
    let low = lspci.to_ascii_lowercase();
    if low.contains("gfx1030") {
        Some("gfx1030".to_string())
    } else if low.contains("gfx11") {
        Some("gfx11".to_string())
    } else {
        None
    }
}
