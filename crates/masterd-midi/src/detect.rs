//! System MIDI player detection.
//!
//! Probes PATH for known MIDI player binaries in priority order.
//! Each variant knows its own CLI invocation signature.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use anyhow::Result;

/// A locally installed MIDI player binary.
#[derive(Debug, Clone)]
pub enum SystemPlayer {
    /// `fluidsynth -a pulseaudio -m alsa_seq -l -i <sf2> <midi>`
    FluidSynth { bin: PathBuf, sf2: Option<PathBuf> },
    /// `timidity <midi>`
    Timidity { bin: PathBuf },
    /// `aplaymidi --port=<auto> <midi>`
    Aplaymidi { bin: PathBuf },
    /// `pmidi -p <auto> <midi>`
    Pmidi { bin: PathBuf },
    /// `wildmidi <midi>`
    WildMidi { bin: PathBuf },
}

impl SystemPlayer {
    pub fn name(&self) -> &'static str {
        match self {
            Self::FluidSynth { .. } => "fluidsynth",
            Self::Timidity { .. }   => "timidity",
            Self::Aplaymidi { .. }  => "aplaymidi",
            Self::Pmidi { .. }      => "pmidi",
            Self::WildMidi { .. }   => "wildmidi",
        }
    }

    /// Spawn the player process for `midi_path`. Returns the `Child` handle.
    pub fn spawn(&self, midi_path: &Path) -> Result<Child> {
        let path_str = midi_path.to_string_lossy();
        match self {
            Self::FluidSynth { bin, sf2 } => {
                let sf = sf2.as_deref()
                    .and_then(|p| p.to_str())
                    .unwrap_or_else(|| best_system_sf2());
                Command::new(bin)
                    .args(["-a", "pulseaudio", "-m", "alsa_seq", "-l", "-i", sf, &path_str])
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("fluidsynth spawn: {e}"))
            }
            Self::Timidity { bin } => {
                Command::new(bin)
                    .arg(&*path_str)
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("timidity spawn: {e}"))
            }
            Self::Aplaymidi { bin } => {
                // Find the first writable ALSA sequencer port.
                let port = detect_alsa_port().unwrap_or_else(|| "128:0".to_string());
                Command::new(bin)
                    .args(["--port", &port, &path_str])
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("aplaymidi spawn: {e}"))
            }
            Self::Pmidi { bin } => {
                let port = detect_alsa_port().unwrap_or_else(|| "128:0".to_string());
                Command::new(bin)
                    .args(["-p", &port, &path_str])
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("pmidi spawn: {e}"))
            }
            Self::WildMidi { bin } => {
                Command::new(bin)
                    .arg(&*path_str)
                    .spawn()
                    .map_err(|e| anyhow::anyhow!("wildmidi spawn: {e}"))
            }
        }
    }
}

/// Scan PATH for the first available system MIDI player, in priority order.
pub fn find_system_player() -> Option<SystemPlayer> {
    // fluidsynth — best quality if a soundfont is available
    if let Ok(bin) = which::which("fluidsynth") {
        let sf2 = best_system_sf2_path();
        return Some(SystemPlayer::FluidSynth { bin, sf2 });
    }

    // timidity — most common on Ubuntu/Debian
    if let Ok(bin) = which::which("timidity") {
        return Some(SystemPlayer::Timidity { bin });
    }

    // aplaymidi — ALSA native, needs a software synth port open
    if let Ok(bin) = which::which("aplaymidi") {
        if detect_alsa_port().is_some() {
            return Some(SystemPlayer::Aplaymidi { bin });
        }
        // Skip aplaymidi if no MIDI port found — it would just hang.
    }

    // pmidi — alternative ALSA player
    if let Ok(bin) = which::which("pmidi") {
        if detect_alsa_port().is_some() {
            return Some(SystemPlayer::Pmidi { bin });
        }
    }

    // wildmidi — GUS-patch synth
    if let Ok(bin) = which::which("wildmidi") {
        return Some(SystemPlayer::WildMidi { bin });
    }

    None
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// System soundfont search path for fluidsynth.
const SYSTEM_SF2_PATHS: &[&str] = &[
    "/usr/share/sounds/sf2/FluidR3_GM.sf2",
    "/usr/share/sounds/sf2/default-GM.sf2",
    "/usr/share/sounds/sf2/TimGM6mb.sf2",
    "/usr/share/sounds/sf2/FluidR3_GS.sf2",
    "/usr/share/soundfonts/default.sf2",
];

fn best_system_sf2_path() -> Option<PathBuf> {
    SYSTEM_SF2_PATHS.iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

fn best_system_sf2() -> &'static str {
    SYSTEM_SF2_PATHS.iter()
        .copied()
        .find(|p| std::path::Path::new(p).exists())
        .unwrap_or("/usr/share/sounds/sf2/FluidR3_GM.sf2")
}

/// Run `aplaymidi -l` and return the first listed MIDI port, e.g. `"128:0"`.
fn detect_alsa_port() -> Option<String> {
    let out = Command::new("aplaymidi").arg("-l").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines().skip(1) {
        // Lines look like: " 128:0    Timidity++"
        let cols: Vec<&str> = line.split_whitespace().collect();
        if let Some(port) = cols.first() {
            if port.contains(':') {
                return Some(port.to_string());
            }
        }
    }
    None
}
