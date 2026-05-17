use std::time::Duration;

use rodio::{OutputStream, OutputStreamHandle, Sink, Source};

// ── Tuning per phase ─────────────────────────────────────────────────────────
// Two detuned oscillators (beating), an LFO, and master volume.
struct PhaseProfile {
    /// Fundamental Hz
    freq_a: f32,
    /// Second oscillator Hz (slight detune → beating)
    freq_b: f32,
    /// LFO rate Hz
    lfo_hz: f32,
    /// LFO depth 0..1  (0 = no wobble, 1 = full mute on trough)
    lfo_depth: f32,
    /// Master gain 0..1
    gain: f32,
}

const DAWN: PhaseProfile =  PhaseProfile { freq_a: 55.0, freq_b: 56.2, lfo_hz: 0.09, lfo_depth: 0.25, gain: 0.08 };
const DAY: PhaseProfile =   PhaseProfile { freq_a: 70.0, freq_b: 71.5, lfo_hz: 0.14, lfo_depth: 0.20, gain: 0.07 };
const DUSK: PhaseProfile =  PhaseProfile { freq_a: 45.0, freq_b: 46.1, lfo_hz: 0.22, lfo_depth: 0.35, gain: 0.10 };
const NIGHT: PhaseProfile = PhaseProfile { freq_a: 28.0, freq_b: 29.3, lfo_hz: 0.04, lfo_depth: 0.45, gain: 0.12 };

fn profile(phase: &str) -> &'static PhaseProfile {
    match phase {
        "day"   => &DAY,
        "dusk"  => &DUSK,
        "night" => &NIGHT,
        _       => &DAWN,
    }
}

// ── Drone oscillator ─────────────────────────────────────────────────────────

/// Two-oscillator drone with LFO amplitude modulation.
struct Drone {
    sample_rate: u32,
    freq_a: f32,
    freq_b: f32,
    lfo_hz: f32,
    lfo_depth: f32,
    gain: f32,
    t: f64,
}

impl Drone {
    fn new(p: &PhaseProfile) -> Self {
        Self {
            sample_rate: 44_100,
            freq_a: p.freq_a,
            freq_b: p.freq_b,
            lfo_hz: p.lfo_hz,
            lfo_depth: p.lfo_depth,
            gain: p.gain,
            t: 0.0,
        }
    }
}

impl Iterator for Drone {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        use std::f64::consts::TAU;

        let t = self.t;
        let osc_a = (TAU * self.freq_a as f64 * t).sin() as f32;
        let osc_b = (TAU * self.freq_b as f64 * t).sin() as f32;
        let lfo = ((TAU * self.lfo_hz as f64 * t).sin() as f32) * self.lfo_depth;
        // lfo_depth controls how much amplitude dips — lfo range: [1-depth .. 1]
        let env = 1.0 - self.lfo_depth * 0.5 + lfo * 0.5;

        self.t += 1.0 / self.sample_rate as f64;
        Some(((osc_a + osc_b) * 0.5) * env * self.gain)
    }
}

impl Source for Drone {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 1 }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { None }
}

// ── Sting (descending glide) ──────────────────────────────────────────────────

/// Short descending pitch glide — triggered on player choice.
struct Sting {
    sample_rate: u32,
    t: f64,
    duration_secs: f64,
}

impl Sting {
    fn new() -> Self {
        Self { sample_rate: 44_100, t: 0.0, duration_secs: 0.6 }
    }
}

impl Iterator for Sting {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.t >= self.duration_secs {
            return None;
        }
        use std::f64::consts::TAU;
        // Frequency glides from 440 Hz down to 110 Hz
        let progress = self.t / self.duration_secs;
        let freq = 440.0 - (330.0 * progress);
        // Exponential decay envelope
        let env = (-5.0 * progress).exp() as f32;
        let sample = (TAU * freq * self.t).sin() as f32 * env * 0.18;
        self.t += 1.0 / self.sample_rate as f64;
        Some(sample)
    }
}

impl Source for Sting {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 1 }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.duration_secs))
    }
}

// ── Death impact ──────────────────────────────────────────────────────────────

/// Low-frequency thud with fast decay — played on NPC death.
struct DeathImpact {
    sample_rate: u32,
    t: f64,
    duration_secs: f64,
}

impl DeathImpact {
    fn new() -> Self {
        Self { sample_rate: 44_100, t: 0.0, duration_secs: 1.4 }
    }
}

impl Iterator for DeathImpact {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.t >= self.duration_secs {
            return None;
        }
        use std::f64::consts::TAU;
        let progress = self.t / self.duration_secs;
        // Pitch drops fast (impact character)
        let freq = 80.0 * (-8.0 * progress).exp() + 28.0;
        let env  = (-4.0 * progress).exp() as f32;
        // Add a noise layer for impact texture
        let osc   = (TAU * freq * self.t).sin() as f32;
        let noise = (self.t.fract() as f32 - 0.5) * 0.3;
        let sample = (osc * 0.7 + noise * 0.3) * env * 0.25;
        self.t += 1.0 / self.sample_rate as f64;
        Some(sample)
    }
}

impl Source for DeathImpact {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 1 }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(self.duration_secs))
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub enum SoundCue {
    Phase(String),
    Sting,
    Death,
}

pub struct SoundEngine {
    // Must be held alive — dropping it closes the audio stream.
    _stream: OutputStream,
    _handle: OutputStreamHandle,
    ambient: Sink,
    fx: Sink,
}

impl SoundEngine {
    /// Tries to initialise audio. Returns `None` if no audio device is available.
    pub fn try_init() -> Option<Self> {
        let (stream, handle) = OutputStream::try_default().ok()?;
        let ambient = Sink::try_new(&handle).ok()?;
        let fx      = Sink::try_new(&handle).ok()?;
        Some(Self { _stream: stream, _handle: handle, ambient, fx })
    }

    pub fn play(&self, cue: SoundCue) {
        match cue {
            SoundCue::Phase(phase) => {
                self.ambient.stop();
                self.ambient.append(Drone::new(profile(&phase)));
                self.ambient.play();
            }
            SoundCue::Sting => {
                self.fx.stop();
                self.fx.append(Sting::new());
                self.fx.play();
            }
            SoundCue::Death => {
                self.fx.stop();
                self.fx.append(DeathImpact::new());
                self.fx.play();
            }
        }
    }
}
