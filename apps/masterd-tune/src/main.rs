use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use masterd_runtime_tune::{
    TunerPolicy, autotune, ensure_startup_profile, fetch_optional_packs, load_kernel_manifest,
    load_profiles, probe_runtime, write_lock, write_tuning_report,
};

#[derive(Debug, Parser)]
#[command(name = "masterd-tune")]
#[command(about = "MASTERd AMD runtime tuning and profile management")]
struct Args {
    #[arg(long, default_value_t = true)]
    auto: bool,
    #[arg(long, default_value_t = false)]
    safe: bool,
    #[arg(long, default_value_t = false)]
    retune: bool,
    #[arg(long, default_value = "config/amd_profiles")]
    profiles_dir: PathBuf,
    #[arg(long, default_value = "config/kernel_manifest.toml")]
    manifest: PathBuf,
    #[arg(long, default_value = "data/runtime_profile_lock.json")]
    lock: PathBuf,
    #[arg(long, default_value = "data/tuning_report.json")]
    report: PathBuf,
    #[arg(long, default_value_t = 10)]
    budget_minutes: u64,
    #[arg(long, default_value_t = true)]
    allow_downloads: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let policy = TunerPolicy {
        time_budget_secs: args.budget_minutes.saturating_mul(60),
        strictness: if args.safe {
            "safe_only".to_string()
        } else {
            "balanced".to_string()
        },
        allow_optional_downloads: args.allow_downloads,
    };

    let manifest = load_kernel_manifest(&args.manifest)?;
    if args.auto && !args.retune {
        let lock = ensure_startup_profile(&policy, &args.profiles_dir, &manifest, &args.lock)?;
        write_tuning_report(&args.report, &lock, &manifest)?;
        println!("startup profile active: {}", lock.selected_profile);
        return Ok(());
    }

    let capability = probe_runtime()?;
    if args.allow_downloads {
        let downloaded = fetch_optional_packs(
            &manifest,
            &capability,
            PathBuf::from("data/kernel_cache").as_path(),
        )?;
        if !downloaded.is_empty() {
            println!("optional packs prepared: {}", downloaded.len());
        }
    }

    let profiles = load_profiles(&args.profiles_dir)?;
    let tuned = autotune(&policy, &capability, &profiles)
        .context("autotune failed while scoring runtime profiles")?;
    let lock = write_lock(&args.lock, &tuned, &capability)?;
    write_tuning_report(&args.report, &lock, &manifest)?;
    println!("selected profile: {}", tuned.selected_profile.name);
    println!("backend: {}", tuned.selected_profile.backend);
    Ok(())
}
