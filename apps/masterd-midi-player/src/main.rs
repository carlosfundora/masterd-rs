use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use midly::{MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use rodio::{OutputStream, Sink, buffer::SamplesBuffer};

const SAMPLE_RATE: u32 = 44_100;

#[derive(Debug, Parser)]
#[command(name = "masterd-midi-player")]
#[command(about = "Rust-native boot synth player for MASTERd installer")]
struct Args {
    #[arg(long, default_value_t = 24.0)]
    seconds: f32,
    #[arg(long)]
    midi_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
struct NoteSegment {
    start_sec: f32,
    end_sec: f32,
    freq_hz: f32,
    amp: f32,
}

#[derive(Debug, Clone, Copy)]
enum MidiEventKind {
    Tempo(u32),
    NoteOn { key: u8, vel: u8 },
    NoteOff { key: u8 },
}

#[derive(Debug, Clone, Copy)]
struct MidiEvent {
    tick: u64,
    kind: MidiEventKind,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let (_stream, handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&handle)?;

    let target_seconds = args.seconds.max(1.0);
    let midi_path = args
        .midi_file
        .or_else(default_midi_path)
        .filter(|p| p.exists());

    let stereo = if let Some(path) = midi_path {
        render_midi_track(&path, target_seconds)
            .unwrap_or_else(|_| render_synth_fallback(target_seconds))
    } else {
        render_synth_fallback(target_seconds)
    };

    let src = SamplesBuffer::new(2, SAMPLE_RATE, stereo);
    sink.append(src);
    sink.sleep_until_end();
    Ok(())
}

fn default_midi_path() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let local = cwd.join("apps/masterd-midi-player/assets/sample.mid");
    if local.exists() {
        return Some(local);
    }
    let alt = cwd.join("references/sample.mid");
    if alt.exists() {
        return Some(alt);
    }
    None
}

fn render_midi_track(path: &PathBuf, seconds: f32) -> Result<Vec<f32>> {
    let bytes =
        fs::read(path).with_context(|| format!("failed reading midi file {}", path.display()))?;
    let smf =
        Smf::parse(&bytes).with_context(|| format!("failed parsing midi {}", path.display()))?;
    let ticks_per_beat = match smf.header.timing {
        Timing::Metrical(tpq) => tpq.as_int() as f32,
        Timing::Timecode(_fps, _subframe) => 480.0,
    };

    let mut events = Vec::new();
    for track in &smf.tracks {
        let mut tick = 0u64;
        for ev in track {
            tick += ev.delta.as_int() as u64;
            if let Some(kind) = to_midi_event_kind(&ev.kind) {
                events.push(MidiEvent { tick, kind });
            }
        }
    }
    events.sort_by_key(|e| e.tick);

    let segments = events_to_segments(&events, ticks_per_beat);
    let mono = render_segments(&segments, seconds);
    Ok(to_stereo_with_fx(mono))
}

fn to_midi_event_kind(kind: &TrackEventKind<'_>) -> Option<MidiEventKind> {
    match kind {
        TrackEventKind::Meta(MetaMessage::Tempo(t)) => Some(MidiEventKind::Tempo(t.as_int())),
        TrackEventKind::Midi {
            channel: _,
            message,
        } => match message {
            MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => Some(MidiEventKind::NoteOn {
                key: key.as_int(),
                vel: vel.as_int(),
            }),
            MidiMessage::NoteOn { key, vel: _ } => {
                Some(MidiEventKind::NoteOff { key: key.as_int() })
            }
            MidiMessage::NoteOff { key, vel: _ } => {
                Some(MidiEventKind::NoteOff { key: key.as_int() })
            }
            _ => None,
        },
        _ => None,
    }
}

fn events_to_segments(events: &[MidiEvent], ticks_per_beat: f32) -> Vec<NoteSegment> {
    let mut tempo_us_per_qn = 500_000u32;
    let mut last_tick = 0u64;
    let mut now_sec = 0.0f32;
    let mut active: HashMap<u8, (f32, u8)> = HashMap::new();
    let mut out = Vec::new();

    for ev in events {
        let dt_ticks = ev.tick.saturating_sub(last_tick) as f32;
        now_sec += (dt_ticks / ticks_per_beat) * (tempo_us_per_qn as f32 / 1_000_000.0);
        last_tick = ev.tick;

        match ev.kind {
            MidiEventKind::Tempo(t) => tempo_us_per_qn = t,
            MidiEventKind::NoteOn { key, vel } => {
                active.insert(key, (now_sec, vel));
            }
            MidiEventKind::NoteOff { key } => {
                if let Some((start_sec, vel)) = active.remove(&key) {
                    if now_sec > start_sec {
                        out.push(NoteSegment {
                            start_sec,
                            end_sec: now_sec,
                            freq_hz: midi_key_to_hz(key),
                            amp: (vel as f32 / 127.0) * 0.20,
                        });
                    }
                }
            }
        }
    }
    out
}

fn render_segments(segments: &[NoteSegment], seconds: f32) -> Vec<f32> {
    let total_samples = (seconds * SAMPLE_RATE as f32) as usize;
    let mut mono = vec![0.0f32; total_samples];

    for seg in segments {
        let start = ((seg.start_sec * SAMPLE_RATE as f32).max(0.0)) as usize;
        let end = ((seg.end_sec * SAMPLE_RATE as f32).min(total_samples as f32)) as usize;
        if end <= start {
            continue;
        }
        let dur = (end - start) as f32 / SAMPLE_RATE as f32;
        for i in start..end {
            let rel_t = (i - start) as f32 / SAMPLE_RATE as f32;
            let phase = frac(rel_t * seg.freq_hz);
            let env = adsr(rel_t, dur);
            let osc = saw(phase) * 0.58 + triangle(phase) * 0.30 + square(phase) * 0.12;
            mono[i] += osc * env * seg.amp;
        }
    }
    mono
}

fn to_stereo_with_fx(mut mono: Vec<f32>) -> Vec<f32> {
    // subtle low reverb tail
    let delay = (SAMPLE_RATE as f32 * 0.11) as usize;
    let mut reverb = vec![0.0f32; delay.max(1)];
    let mut idx = 0usize;
    let feedback = 0.22f32;
    let mix = 0.10f32;

    for sample in &mut mono {
        let wet = reverb[idx];
        let dry = *sample;
        reverb[idx] = (dry + wet * feedback).clamp(-1.0, 1.0);
        idx = (idx + 1) % reverb.len();
        // slight arcade texture
        let arcade = quantize_amp(dry * 0.86 + wet * mix, 112.0);
        *sample = arcade.clamp(-0.95, 0.95);
    }

    let mut stereo = Vec::with_capacity(mono.len() * 2);
    for (i, sample) in mono.iter().enumerate() {
        let t = i as f32 / SAMPLE_RATE as f32;
        let drift = (2.0 * std::f32::consts::PI * 0.22 * t).sin() * 0.04;
        stereo.push((*sample * (0.98 - drift)).clamp(-1.0, 1.0));
        stereo.push((*sample * (1.00 + drift)).clamp(-1.0, 1.0));
    }
    stereo
}

fn render_synth_fallback(seconds: f32) -> Vec<f32> {
    let samples = (seconds * SAMPLE_RATE as f32) as usize;
    let mut mono = vec![0.0f32; samples];
    let bpm = 108.0f32;
    let sec_per_beat = 60.0 / bpm;
    let notes = [41.20, 49.00, 55.00, 49.00, 43.65, 41.20, 36.71, 41.20];
    for (i, slot) in mono.iter_mut().enumerate() {
        let t = i as f32 / SAMPLE_RATE as f32;
        let n = notes[((t / sec_per_beat) as usize) % notes.len()];
        let ph = frac(t * n);
        *slot = (saw(ph) * 0.22 + triangle(ph) * 0.10).clamp(-0.8, 0.8);
    }
    to_stereo_with_fx(mono)
}

#[inline]
fn midi_key_to_hz(key: u8) -> f32 {
    440.0 * 2.0f32.powf((key as f32 - 69.0) / 12.0)
}

#[inline]
fn adsr(t: f32, dur: f32) -> f32 {
    let attack = 0.008f32;
    let release = 0.08f32.min(dur * 0.5);
    if t < attack {
        return t / attack;
    }
    if t > dur - release {
        return ((dur - t) / release).clamp(0.0, 1.0);
    }
    0.92
}

#[inline]
fn frac(x: f32) -> f32 {
    x - x.floor()
}

#[inline]
fn saw(phase: f32) -> f32 {
    (phase * 2.0) - 1.0
}

#[inline]
fn square(phase: f32) -> f32 {
    if phase < 0.5 { 1.0 } else { -1.0 }
}

#[inline]
fn triangle(phase: f32) -> f32 {
    (2.0 * (2.0 * phase - 1.0).abs()) - 1.0
}

#[inline]
fn quantize_amp(x: f32, steps: f32) -> f32 {
    (x * steps).round() / steps
}
