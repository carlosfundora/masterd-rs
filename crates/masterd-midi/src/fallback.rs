//! Built-in MIDI synthesizer using oxisynth (pure-Rust SF2) + cpal audio output.
//!
//! Parses MIDI with `midly`, synthesizes audio with `oxisynth`, and streams
//! PCM samples to the default output device via `cpal`.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use midly::{Smf, Timing, TrackEventKind, MidiMessage};
use oxisynth::{MidiEvent, SoundFont, Synth, SynthDescriptor};

/// Handle to a background playback thread.
pub struct InternalHandle {
    stop_flag: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl InternalHandle {
    /// Block until playback finishes naturally.
    pub fn wait(mut self) -> Result<()> {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        Ok(())
    }

    /// Signal the synth to stop immediately.
    pub fn stop(self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

/// Render `midi_data` using `sf2_data` as the soundfont and stream to the
/// default audio output device. Returns immediately; playback runs on a
/// background thread.
pub fn play_bytes(midi_data: &[u8], sf2_data: &[u8]) -> Result<InternalHandle> {
    let events = parse_midi(midi_data)?;
    let sf2_owned = sf2_data.to_vec();
    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop_flag);

    let thread = std::thread::spawn(move || {
        if let Err(e) = render_and_play(events, &sf2_owned, stop_clone) {
            tracing::error!("MIDI built-in playback error: {e}");
        }
    });

    Ok(InternalHandle { stop_flag, thread: Some(thread) })
}

// ── MIDI parsing ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct TimedEvent {
    time_us: u64,
    msg: MidiMessage,
    channel: u8,
}

fn parse_midi(data: &[u8]) -> Result<Vec<TimedEvent>> {
    let smf = Smf::parse(data).context("MIDI parse failed")?;

    let ppq = match smf.header.timing {
        Timing::Metrical(tpb) => tpb.as_int() as u64,
        Timing::Timecode(fps, sub) => (fps.as_f32() * sub as f32) as u64,
    };

    let mut us_per_beat: u64 = 500_000; // default 120 BPM
    let mut out: Vec<TimedEvent> = Vec::new();

    for track in &smf.tracks {
        let mut time_us: u64 = 0;
        for ev in track {
            let delta = ev.delta.as_int() as u64;
            time_us += delta * us_per_beat / ppq.max(1);

            match ev.kind {
                TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) => {
                    us_per_beat = t.as_int() as u64;
                }
                TrackEventKind::Midi { channel, message } => {
                    out.push(TimedEvent {
                        time_us,
                        msg: message,
                        channel: channel.as_int(),
                    });
                }
                _ => {}
            }
        }
    }

    out.sort_by_key(|e| e.time_us);
    Ok(out)
}

// ── Synthesis + audio output ──────────────────────────────────────────────────

fn render_and_play(
    events: Vec<TimedEvent>,
    sf2_data: &[u8],
    stop: Arc<AtomicBool>,
) -> Result<()> {
    const SAMPLE_RATE: u32 = 44_100;

    let mut synth = Synth::new(SynthDescriptor {
        sample_rate: SAMPLE_RATE as f32,
        ..Default::default()
    })
    .map_err(|e| anyhow::anyhow!("oxisynth init: {e:?}"))?;

    let mut cursor = std::io::Cursor::new(sf2_data);
    let sf = SoundFont::load(&mut cursor)
        .map_err(|e| anyhow::anyhow!("SF2 load: {e:?}"))?;
    synth.add_font(sf, true);

    let tail_us: u64 = 2_000_000;
    let total_us = events.last().map(|e| e.time_us).unwrap_or(0) + tail_us;
    let total_samples = (total_us as f64 / 1_000_000.0 * SAMPLE_RATE as f64) as usize;

    let mut pcm_left  = vec![0.0f32; total_samples];
    let mut pcm_right = vec![0.0f32; total_samples];

    let samples_per_us = SAMPLE_RATE as f64 / 1_000_000.0;
    let mut sample_pos: usize = 0;
    let mut last_event_sample: usize = 0;

    for ev in &events {
        if stop.load(Ordering::Relaxed) { break; }

        let event_sample = (ev.time_us as f64 * samples_per_us) as usize;
        let render_count = event_sample.saturating_sub(last_event_sample);

        if render_count > 0 {
            let n = render_count.min(total_samples.saturating_sub(sample_pos));
            if n > 0 {
                synth.write_f32(n, &mut pcm_left, sample_pos, 1, &mut pcm_right, sample_pos, 1);
                sample_pos += n;
            }
        }
        last_event_sample = event_sample;

        let midi_ev = match ev.msg {
            MidiMessage::NoteOn  { key, vel } =>
                MidiEvent::NoteOn  { channel: ev.channel, key: key.as_int(), vel: vel.as_int() },
            MidiMessage::NoteOff { key, .. } =>
                MidiEvent::NoteOff { channel: ev.channel, key: key.as_int() },
            MidiMessage::ProgramChange { program } =>
                MidiEvent::ProgramChange { channel: ev.channel, program_id: program.as_int() },
            MidiMessage::Controller { controller, value } =>
                MidiEvent::ControlChange { channel: ev.channel, ctrl: controller.as_int(), value: value.as_int() },
            MidiMessage::PitchBend { bend } => {
                let v = (bend.as_f32() * 8192.0 + 8192.0).clamp(0.0, 16383.0) as u16;
                MidiEvent::PitchBend { channel: ev.channel, value: v }
            }
            MidiMessage::Aftertouch { key, vel } =>
                MidiEvent::PolyphonicKeyPressure { channel: ev.channel, key: key.as_int(), value: vel.as_int() },
            MidiMessage::ChannelAftertouch { vel } =>
                MidiEvent::ChannelPressure { channel: ev.channel, value: vel.as_int() },
        };
        let _ = synth.send_event(midi_ev);
    }

    let remaining = total_samples.saturating_sub(sample_pos);
    if remaining > 0 && !stop.load(Ordering::Relaxed) {
        synth.write_f32(remaining, &mut pcm_left, sample_pos, 1, &mut pcm_right, sample_pos, 1);
    }

    stream_pcm(pcm_left, pcm_right, stop)
}

fn stream_pcm(left: Vec<f32>, right: Vec<f32>, stop: Arc<AtomicBool>) -> Result<()> {
    let host   = cpal::default_host();
    let device = host.default_output_device().context("no audio output device")?;
    let config = device.default_output_config().context("no default output config")?;

    let channels = config.channels() as usize;
    let buf   = Arc::new(std::sync::Mutex::new((left, right, 0usize)));
    let buf2  = Arc::clone(&buf);
    let done  = Arc::new(AtomicBool::new(false));
    let done2 = Arc::clone(&done);

    let stream = device.build_output_stream(
        &config.config(),
        move |data: &mut [f32], _| fill_output(data, channels, &buf2, &done2),
        |e| tracing::error!("cpal stream error: {e}"),
        None,
    )?;
    stream.play()?;

    while !done.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

fn fill_output(
    output: &mut [f32],
    channels: usize,
    buf: &Arc<std::sync::Mutex<(Vec<f32>, Vec<f32>, usize)>>,
    done: &Arc<AtomicBool>,
) {
    let mut guard = buf.lock().unwrap();
    let (ref left, ref right, ref mut pos) = *guard;
    let frames = output.len() / channels;

    for i in 0..frames {
        if *pos >= left.len() {
            for s in &mut output[i * channels..] { *s = 0.0; }
            done.store(true, Ordering::Relaxed);
            return;
        }
        output[i * channels] = left[*pos];
        if channels > 1 { output[i * channels + 1] = right[*pos]; }
        for c in 2..channels { output[i * channels + c] = 0.0; }
        *pos += 1;
    }
}
