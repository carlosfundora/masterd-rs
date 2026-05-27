//! MIDI loader and player for MASTERd.
//!
//! # Priority chain
//! 1. **fluidsynth** — best quality, uses the system or bundled soundfont
//! 2. **timidity** — widely available, good quality
//! 3. **aplaymidi** — ALSA hardware/software MIDI port
//! 4. **pmidi** — lightweight ALSA player
//! 5. **wildmidi** — GUS-patch software synth
//! 6. **Built-in** — oxisynth (pure Rust SF2 synth) + cpal audio output,
//!    driven by the bundled `TimGM6mb.sf2` soundfont embedded at compile time.
//!
//! # Usage
//! ```no_run
//! use masterd_midi::MidiPlayer;
//! use std::path::Path;
//!
//! # fn main() -> anyhow::Result<()> {
//! let handle = MidiPlayer::play(Path::new("song.mid"))?;
//! handle.wait()?; // block until done, or call .stop() to cancel
//! # Ok(())
//! # }
//! ```

pub mod detect;
pub mod fallback;

use anyhow::Result;
use std::path::Path;

pub use detect::SystemPlayer;

// The bundled GM soundfont embedded at compile time.
// Stored inside the crate so it compiles without the broader workspace models dir.
const BUNDLED_SF2: &[u8] = include_bytes!("../assets/TimGM6mb.sf2");

// ── Public API ────────────────────────────────────────────────────────────────

/// A handle to an active MIDI playback session.
pub enum PlaybackHandle {
    /// A running OS child process (system player).
    Process(std::process::Child),
    /// The built-in synth running on a background thread.
    Internal(fallback::InternalHandle),
}

impl PlaybackHandle {
    /// Block until playback completes.
    pub fn wait(self) -> Result<()> {
        match self {
            Self::Process(mut child) => {
                child.wait()?;
                Ok(())
            }
            Self::Internal(h) => h.wait(),
        }
    }

    /// Stop playback immediately.
    pub fn stop(self) {
        match self {
            Self::Process(mut child) => {
                let _ = child.kill();
            }
            Self::Internal(h) => h.stop(),
        }
    }
}

/// Unified MIDI player — tries installed system players before falling back to
/// the built-in oxisynth synthesizer.
pub struct MidiPlayer;

impl MidiPlayer {
    /// Play a MIDI file at `path`.
    ///
    /// Returns a `PlaybackHandle` so the caller can block or cancel playback.
    /// Uses the first detected system player; falls back to the built-in synth.
    pub fn play(path: &Path) -> Result<PlaybackHandle> {
        if let Some(player) = detect::find_system_player() {
            tracing::info!("MIDI: using system player {:?}", player);
            match player.spawn(path) {
                Ok(child) => return Ok(PlaybackHandle::Process(child)),
                Err(e) => {
                    tracing::warn!("MIDI: system player failed ({e}), falling back to built-in")
                }
            }
        }

        tracing::info!("MIDI: using built-in oxisynth synthesizer");
        let midi_bytes = std::fs::read(path)?;
        let handle = fallback::play_bytes(&midi_bytes, BUNDLED_SF2)?;
        Ok(PlaybackHandle::Internal(handle))
    }

    /// Play MIDI from raw bytes (no file required).
    ///
    /// Always uses the built-in synthesizer since system players require a file path.
    pub fn play_bytes(midi_data: &[u8]) -> Result<PlaybackHandle> {
        let handle = fallback::play_bytes(midi_data, BUNDLED_SF2)?;
        Ok(PlaybackHandle::Internal(handle))
    }

    /// Play MIDI from raw bytes using a custom SF2 soundfont.
    pub fn play_bytes_with_sf2(midi_data: &[u8], sf2_data: &[u8]) -> Result<PlaybackHandle> {
        let handle = fallback::play_bytes(midi_data, sf2_data)?;
        Ok(PlaybackHandle::Internal(handle))
    }

    /// Returns the name of the player that *would* be used for `play()`,
    /// without actually starting playback.
    pub fn active_backend() -> &'static str {
        match detect::find_system_player() {
            Some(p) => p.name(),
            None => "built-in (oxisynth)",
        }
    }
}
