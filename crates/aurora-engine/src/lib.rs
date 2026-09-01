//! AURORA Engine — Rust real-time audio engine powering Aurora Producer Suite.
//!
//! Capabilities:
//! - Lock-free real-time mixer graph (audio callback -> tracks -> buses -> master)
//! - Pro DSP rack: EQ, compressor, gate, de-esser, reverb, delay, chorus,
//!   flanger, phaser, saturation, limiter
//! - Polyphonic subtractive synth for instrument tracks
//! - Recording via cpal with low-latency monitoring + synthetic vocal source
//! - One-click AI vocal cleanup (noise/hum/click/breath/de-ess/de-harsh)
//! - AI mix analysis with applicable suggestions
//! - Offline bounce: WAV 16/24/32f + MP3, mix or stems, exact render parity
//! - BS.1770 loudness measurement + spectral tap for live analyzers

pub mod ai;
pub mod audio_io;
pub mod bounce;
pub mod demo;
pub mod dsp;
pub mod effects;
pub mod engine;
pub mod io;
pub mod jobs;
pub mod project;
pub mod selftest;
pub mod synth;

pub fn version() -> String {
    format!(
        "{} ({})",
        env!("CARGO_PKG_VERSION"),
        option_env!("GIT_HASH").unwrap_or("dev")
    )
}
