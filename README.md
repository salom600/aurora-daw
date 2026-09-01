# AURORA Producer Suite

A modern, Rust-powered virtual studio DAW for singers, producers, arrangers,
engineers and studios. Record, produce, edit, mix, process, export and prepare
music for release — with a lightweight engine designed for speed and stability.

![AURORA](https://img.shields.io/badge/version-2.7.0-blue) ![Rust](https://img.shields.io/badge/rust-stable-orange) ![engine](https://img.shields.io/badge/audio%20engine-Rust%20real--time-success)

## Highlights

- **Rust real-time engine** — lock-free command queue, atomic parameter store,
  sample-accurate mixer graph with buses, sends, per-track FX chains.
- **Pro DSP rack** — EQ, compressor, gate, de-esser, reverb (Freeverb), ping-pong
  delay, chorus, flanger, phaser, saturation, brickwall limiter.
- **One-click AI vocal cleanup** — STFT spectral analysis removes broadband noise,
  50/60 Hz hum (+harmonics), clicks, breaths, sibilance and harshness. Runs as a
  background job: playback/editing/export never stall.
- **AI Mix Assistant** — analyzes every track (RMS, spectral centroid, low/high
  energy) and applies gain, pan, EQ and reverb-send suggestions with one click.
- **Recording** — mic input via cpal (ALSA/WASAPI/CoreAudio) with low-latency
  monitoring; automatic synthetic driver fallback so the app is always testable.
- **Arranger & editing** — waveform clips (precomputed peaks), MIDI clips with a
  piano roll, split/duplicate/delete, snap, zoom, loop region, comping takes.
- **Mixer** — channel strips with FX slots, pan knobs, faders + meters, master
  strip with BS.1770 loudness (LUFS-I, dBTP) and live spectral analyzer.
- **Export** — offline bounce with exact render parity: WAV 16/24/32f + MP3 320,
  full mix or per-track stems.
- **Scale** — virtualized UI and efficient graph keep 1000+ track projects fluid.

## Build

```bash
# Linux (ALSA dev headers needed for audio devices; the app also runs without)
sudo apt install libasound2-dev pkg-config
cargo build --release
./target/release/aurora-daw            # AURORA Studio (demo session loads)

# Headless engine self-test
./target/release/aurora-daw --selftest

# Scripted end-to-end UI validation + screenshots (needs X)
xvfb-run ./target/release/aurora-daw --autotest ./shots
```

Windows/macOS: `cargo build --release` (WASAPI/CoreAudio used automatically).

## Architecture

```
crates/
  aurora-engine/   # DSP, mixer graph, synth, AI, I/O, bounce — no GUI deps
  aurora-app/      # egui UI: topbar, browser, arranger, mixer, inspector
```

The UI thread owns the project; the audio thread owns an engine snapshot.
They talk through a lock-free ring (commands), atomics (hot params/meters) and
a bounded channel (events). The offline bounce re-uses the exact realtime graph,
so exports sound precisely like playback.

MIT License — Copyright (c) 2026 salom600
