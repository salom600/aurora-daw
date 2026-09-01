//! Procedural demo project — "Aurora Session" matching the reference design:
//! 16 tracks of real synthesized material at 124 BPM in the Aurora palette.
//! The vocal track ships contaminated (noise + hum + clicks + breaths) so the
//! one-click AI Vocal Cleaner can demonstrate genuine, audible repair.

use crate::dsp::*;
use crate::project::*;
use rand::Rng;
use std::sync::Arc;

const SR: u32 = 48_000;

pub const COL_DRUMS: [u8; 4] = [45, 212, 191, 255];
pub const COL_BASS: [u8; 4] = [56, 189, 248, 255];
pub const COL_SYNTH: [u8; 4] = [167, 139, 250, 255];
pub const COL_PAD: [u8; 4] = [129, 140, 248, 255];
pub const COL_PIANO: [u8; 4] = [251, 146, 60, 255];
pub const COL_VOCAL: [u8; 4] = [74, 222, 128, 255];
pub const COL_CHOIR: [u8; 4] = [134, 239, 172, 255];
pub const COL_GUITAR: [u8; 4] = [248, 113, 113, 255];
pub const COL_FX: [u8; 4] = [244, 114, 182, 255];
pub const COL_ATMOS: [u8; 4] = [94, 234, 212, 255];

struct Sig {
    v: Vec<f32>,
}

impl Sig {
    fn new(frames: usize) -> Self {
        Self { v: vec![0.0; frames] }
    }
    fn finish(&self, name: &str) -> SharedAudio {
        // expand mono to stereo with subtle width
        let mut st = Vec::with_capacity(self.v.len() * 2);
        let n = self.v.len();
        for i in 0..n {
            let l = self.v[i];
            let r = if i >= 64 { self.v[i - 64] * 0.35 + l * 0.9 } else { l * 0.95 };
            st.push(l);
            st.push(r);
        }
        Arc::new(AudioData {
            samples: st,
            channels: 2,
            sample_rate: SR,
        })
    }
}

fn frames(secs: f64) -> usize {
    (secs * SR as f64) as usize
}

fn env_exp(i: usize, total: usize, decay: f32) -> f32 {
    let t = i as f32 / SR as f32;
    (-t / decay).exp() * (1.0 - i as f32 / total.max(1) as f32 * 0.2)
}

fn kick(len_s: f64) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let mut phase = 0.0f32;
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let f = 46.0 + 95.0 * (-t / 0.045).exp();
        phase += std::f32::consts::TAU * f / SR as f32;
        let click = if t < 0.004 { (1.0 - t / 0.004) * 0.7 } else { 0.0 };
        s.v[i] = (phase.sin() * 0.9 + click) * env_exp(i, n, len_s as f32 * 0.35);
    }
    s
}

fn noise_burst(len_s: f64, decay: f32, hp: bool, tone_hz: f32, tone_amt: f32) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let mut rng = rand::thread_rng();
    let mut prev = 0.0f32;
    let mut phase = 0.0f32;
    let mut lp = 0.0f32;
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let white = rng.gen::<f32>() * 2.0 - 1.0;
        let noise = if hp {
            let h = white - prev;
            prev = white;
            h * 0.8
        } else {
            lp += 0.35 * (white - lp);
            lp
        };
        phase += std::f32::consts::TAU * tone_hz / SR as f32;
        s.v[i] = (noise * (1.0 - tone_amt) + phase.sin() * tone_amt)
            * env_exp(i, n, decay);
    }
    s
}

fn tone_note(freq: f32, len_s: f64, waveform: u8, decay: f32, detune: f32, cutoff: f32) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let mut p1 = 0.0f32;
    let mut p2 = 0.0f32;
    let mut lp = 0.0f32;
    let coeff = (std::f32::consts::TAU * cutoff.clamp(80.0, 18000.0) / SR as f32).min(0.95) / 1.0;
    let k = coeff / (coeff + 1.0);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let vib = (t * 5.2).sin() * 0.004 + 1.0;
        p1 += freq * vib / SR as f32;
        p2 += freq * vib * (1.0 + detune) / SR as f32;
        let o = |p: f32| match waveform {
            0 => 2.0 * p.fract() - 1.0,
            1 => if p.fract() < 0.5 { 1.0 } else { -1.0 },
            2 => (p.fract() * std::f32::consts::TAU).sin(),
            _ => (2.0 * (p * 2.0 * std::f32::consts::PI).sin().abs() - 1.0).signum()
                * (1.0 - 2.0 * p.fract()).abs(),
        };
        let raw = (o(p1) + o(p2)) * 0.5;
        lp += k * (raw - lp);
        let a = (t / 0.008).min(1.0);
        s.v[i] = lp * env_exp(i, n, decay) * a;
    }
    s
}

fn karplus(freq: f32, len_s: f64) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let mut rng = rand::thread_rng();
    let d = (SR as f32 / freq) as usize;
    let mut line: Vec<f32> = (0..d).map(|_| rng.gen::<f32>() * 2.0 - 1.0).collect();
    let mut idx = 0usize;
    for i in 0..n {
        let v = line[idx];
        let nxt = line[(idx + 1) % d];
        line[idx] = (v + nxt) * 0.4995;
        idx = (idx + 1) % d;
        s.v[i] = v * 0.9;
    }
    s
}

fn piano_note(freq: f32, len_s: f64) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let mut v = 0.0f32;
        for (h, amp) in [(1.0f32, 1.0), (2.0, 0.45), (3.0, 0.22), (4.01, 0.12), (5.0, 0.06)] {
            v += (std::f32::consts::TAU * freq * h * t).sin() * amp * (-t / (0.9 / h.sqrt())).exp();
        }
        s.v[i] = v * (t / 0.004).min(1.0) * 0.6;
    }
    s
}

/// Formant-synthesized "vocal" — glottal source through three formant bands.
pub fn vocal_note(freq: f32, len_s: f64, vowel: usize) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let formants: [[f32; 3]; 3] = [
        [730.0, 1090.0, 2440.0], // "ah"
        [270.0, 2290.0, 3010.0], // "ee"
        [400.0, 1700.0, 2400.0], // "eh"
    ];
    let f = formants[vowel % 3];
    let mut f1 = Biquad::design(FilterKind::BandPass, SR as f32, f[0] as f64, 6.0, 5.0);
    let mut f2 = Biquad::design(FilterKind::BandPass, SR as f32, f[1] as f64, 6.0, 7.0);
    let mut f3 = Biquad::design(FilterKind::BandPass, SR as f32, f[2] as f64, 6.0, 8.0);
    let mut phase = 0.0f32;
    let mut rng = rand::thread_rng();
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let vib = 1.0 + (t * 5.6).sin() * 0.012;
        phase += freq * vib / SR as f32;
        // glottal-ish pulse (saw + rectified harmonics)
        let p = phase.fract();
        let glot = (p * 2.0 - 1.0) * 0.7 + (p * 4.0).fract() * 0.2 - 0.1;
        let a = (t / 0.05).min(1.0) * ((1.0 - t / len_s as f32) / 0.25).min(1.0).max(0.0).min(1.0);
        let src = glot + (rng.gen::<f32>() - 0.5) * 0.03; // aspiration
        let v = f1.process(src) * 1.0 + f2.process(src) * 0.6 + f3.process(src) * 0.4;
        s.v[i] = v * a * 1.6;
    }
    s
}

fn riser(len_s: f64) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let mut rng = rand::thread_rng();
    let mut lp = 0.0f32;
    for i in 0..n {
        let t = i as f32 / n as f32;
        let white = rng.gen::<f32>() * 2.0 - 1.0;
        let k = (0.05 + t * 0.75).min(0.95);
        lp += k * (white - lp);
        s.v[i] = lp * t * t * 1.2 + (t * 2000.0).sin() * t * t * 0.2;
    }
    s
}

fn impact(len_s: f64) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let mut rng = rand::thread_rng();
    let mut phase = 0.0f32;
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let f = 30.0 + 50.0 * (-t / 0.12).exp();
        phase += std::f32::consts::TAU * f / SR as f32;
        let boom = phase.sin() * (-t / 0.9).exp();
        let crash = (rng.gen::<f32>() * 2.0 - 1.0) * (-t / 0.25).exp() * 0.5;
        s.v[i] = boom * 1.1 + crash;
    }
    s
}

fn atmos_pad(len_s: f64, base_freq: f32) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let mut rng = rand::thread_rng();
    let mut lp1 = 0.0f32;
    let mut lp2 = 0.0f32;
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let white = rng.gen::<f32>() * 2.0 - 1.0;
        let k = 0.02 + (t * 0.13).sin().abs() * 0.05;
        lp1 += k * (white - lp1);
        lp2 += k * 0.7 * (lp1 - lp2);
        let drone = (std::f32::consts::TAU * base_freq * t).sin() * 0.2
            + (std::f32::consts::TAU * base_freq * 1.5 * t).sin() * 0.1
            + (std::f32::consts::TAU * base_freq * 2.02 * t).sin() * 0.06;
        let swell = (0.35 + 0.3 * (t * 0.5).sin()).min(1.0);
        s.v[i] = (lp2 * 1.4 + drone) * swell * 0.5;
    }
    s
}

/// Guitar power chord through tanh saturation.
fn power_chord(root: f32, len_s: f64) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    for i in 0..n {
        let t = i as f32 / SR as f32;
        let mut v = 0.0f32;
        for (m, a) in [(1.0f32, 1.0), (1.5, 0.8), (2.0, 0.6), (2.997, 0.35)] {
            v += (std::f32::consts::TAU * root * m * t + (t * 3.0).sin() * 0.01).sin() * a;
        }
        let drive = 6.0;
        let sat = (drive * v * 0.4).tanh() / drive.tanh();
        let pick = (t / 0.003).min(1.0);
        s.v[i] = sat * env_exp(i, n, len_s as f32 * 0.7) * pick * 0.8;
    }
    s
}

fn chord_of(root: f32, ratios: &[f32], len_s: f64, f: impl Fn(f32, f64) -> Sig) -> Sig {
    let n = frames(len_s);
    let mut out = Sig::new(n);
    for (k, r) in ratios.iter().enumerate() {
        let sig = f(root * r, len_s);
        let g = 1.0 / (k as f32 + 1.0) + 0.25;
        for i in 0..n {
            out.v[i] += sig.v[i] * g;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Project assembly
// ---------------------------------------------------------------------------

/// The "Vocal Lead" material is intentionally imperfect: noise floor, 50 Hz
/// hum, clicks and breaths are baked in — exactly what singers' raw takes
/// contain and what the AI Vocal Cleaner is built to repair.
fn contaminated_vocal(len_s: f64) -> Sig {
    let n = frames(len_s);
    let mut s = Sig::new(n);
    let mut rng = rand::thread_rng();
    // phrases: (start_s, dur_s, freq, vowel)
    let mut t = 0.5f32;
    while t < len_s as f32 - 1.5 {
        let dur = 0.45 + rng.gen::<f32>() * 0.5;
        let freq = 200.0 + rng.gen::<f32>() * 130.0;
        let note = vocal_note(freq, dur as f64, rng.gen_range(0..3));
        let start = frames(t as f64);
        for i in 0..note.v.len() {
            if start + i < n {
                s.v[start + i] += note.v[i] * 0.9;
            }
        }
        // breath right before phrase
        let b0 = frames(((t - 0.28).max(0.0)) as f64);
        for i in 0..frames(0.18) {
            if b0 + i < n {
                s.v[b0 + i] += (rng.gen::<f32>() - 0.5) * 0.16;
            }
        }
        t += dur + 0.55 + rng.gen::<f32>() * 0.4;
    }
    // contamination
    let mut hp_state = 0.0f32;
    for i in 0..n {
        let tt = i as f32 / SR as f32;
        let hum = (std::f32::consts::TAU * 50.0 * tt).sin() * 0.035
            + (std::f32::consts::TAU * 150.0 * tt).sin() * 0.012;
        let noise = {
            let w = rng.gen::<f32>() * 2.0 - 1.0;
            hp_state += 0.5 * (w - hp_state);
            hp_state * 0.05
        };
        s.v[i] += hum + noise;
    }
    // clicks
    for _ in 0..12 {
        let p = rng.gen_range(0..n.saturating_sub(200));
        for i in 0..40 {
            if p + i < n {
                s.v[p + i] += (rng.gen::<f32>() - 0.5) * 1.4 * (1.0 - i as f32 / 40.0);
            }
        }
    }
    s
}

/// Interleaved-stereo contaminated vocal sample buffer (for self-tests).
pub fn demo_vocal_samples(secs: f64) -> Vec<f32> {
    let arc = contaminated_vocal(secs).finish("vocal");
    Arc::try_unwrap(arc).map(|d| d.samples).unwrap_or_else(|a| a.samples.clone())
}

pub fn build_demo_project() -> Project {
    let mut p = Project::new_empty("Aurora Session 2026");
    p.tempo = 124.0;
    p.loop_range = (0.0, 8.0 * 60.0 / 124.0 * 4.0);
    p.loop_enabled = true;

    let beat = 60.0 / p.tempo;
    let bar = beat * 4.0;
    let total_bars = 16.0;

    // ---- material (generated once, shared where repeated) ----
    let mut rng = rand::thread_rng();
    let bass_freqs = [55.0f32, 55.0, 73.42, 65.41];
    let lead_scale = [523.25f32, 587.33, 659.25, 783.99, 880.0, 1046.5];

    let drums_kick = kick(0.42).finish("kick");
    let drums_snare = noise_burst(0.32, 0.09, true, 190.0, 0.35).finish("snare");
    let drums_hat = noise_burst(0.14, 0.03, true, 0.0, 0.0).finish("hat");
    let drums_tom = noise_burst(0.4, 0.12, false, 110.0, 0.7).finish("tom");
    let bass_loop: SharedAudio = {
        let n = frames(bar * 4.0);
        let mut s = Sig::new(n);
        let mut pos = 0usize;
        for b in 0..16 {
            let f = bass_freqs[b % 4];
            let note = tone_note(f, beat as f64 * 0.95, 0, 0.5, 0.002, 500.0);
            for (i, v) in note.v.iter().enumerate() {
                if pos + i < n {
                    s.v[pos + i] += v * 1.4;
                }
            }
            pos += frames(beat as f64);
        }
        s.finish("bass")
    };
    let lead_loop: SharedAudio = {
        let n = frames(bar * 4.0);
        let mut s = Sig::new(n);
        let mut pos = 0usize;
        let pattern = [0usize, 2, 4, 3, 5, 4, 2, 1];
        for step in 0..32 {
            let f = lead_scale[pattern[step % pattern.len()]];
            let dur = beat * 0.5;
            let note = tone_note(f, dur as f64 * 1.6, 0, 0.35, 0.01, 7500.0);
            for (i, v) in note.v.iter().enumerate() {
                if pos + i < n {
                    s.v[pos + i] += v * 0.9;
                }
            }
            pos += frames(dur as f64);
        }
        s.finish("lead")
    };
    let pad_chord = {
        let n = frames(bar * 8.0);
        let mut s = Sig::new(n);
        for root in [261.63f32, 329.63, 392.0, 523.25] {
            let sig = chord_of(root, &[1.0, 1.5, 2.0], bar * 8.0, |f, l| {
                tone_note(f, l, 0, 6.0, 0.012, 1800.0)
            });
            for (i, v) in sig.v.iter().enumerate() {
                if i < n {
                    s.v[i] += v * 0.3;
                }
            }
        }
        s.finish("pad")
    };
    let pluck_seq: SharedAudio = {
        let n = frames(bar * 4.0);
        let mut s = Sig::new(n);
        for step in 0..16 {
            let f = 392.0 * 2f32.powf(rng.gen_range(-2..3) as f32 / 12.0);
            let note = karplus(f, 0.5);
            let pos = frames(beat as f64 * step as f64);
            for (i, v) in note.v.iter().enumerate() {
                if pos + i < n {
                    s.v[pos + i] += v * 0.8;
                }
            }
        }
        s.finish("pluck")
    };
    let piano_chords: SharedAudio = {
        let n = frames(bar * 4.0);
        let mut s = Sig::new(n);
        let prog = [(261.63f32, &[1.0f32, 1.25, 1.5][..]), (220.0, &[1.0, 1.25, 1.5]), (174.61, &[1.0, 1.26, 1.5]), (196.0, &[1.0, 1.26, 1.5])];
        for (bi, (root, ratios)) in prog.iter().enumerate() {
            let pos = frames(bar * bi as f64);
            for (ri, r) in ratios.iter().enumerate() {
                let note = piano_note(root * r, bar * 0.9);
                for (i, v) in note.v.iter().enumerate() {
                    if pos + i < n {
                        s.v[pos + i] += v * 0.5 / (ri as f32 + 1.0);
                    }
                }
            }
        }
        s.finish("piano")
    };
    let vocal_lead: SharedAudio = contaminated_vocal(bar * 8.0).finish("vocal");
    let vocal_choir: SharedAudio = {
        let n = frames(bar * 8.0);
        let mut s = Sig::new(n);
        for (root, g) in [(261.63f32, 0.5), (329.63, 0.4), (392.0, 0.35)] {
            let note = vocal_note(root, bar * 8.0, 0);
            for (i, v) in note.v.iter().enumerate() {
                if i < n {
                    s.v[i] += v * g;
                }
            }
        }
        s.finish("choir")
    };
    let guitar_riff: SharedAudio = {
        let n = frames(bar * 4.0);
        let mut s = Sig::new(n);
        for bi in 0..4 {
            let root = [110.0f32, 110.0, 146.83, 130.81][bi];
            let ch = power_chord(root, beat * 1.9);
            for hb in 0..2 {
                let pos = frames(bar * bi as f64 + beat * hb as f64);
                for (i, v) in ch.v.iter().enumerate() {
                    if pos + i < n {
                        s.v[pos + i] += v * 0.7;
                    }
                }
            }
        }
        s.finish("guitar")
    };
    let riser_4bar: SharedAudio = riser(bar * 4.0).finish("riser");
    let impact_1: SharedAudio = impact(2.2).finish("impact");
    let atmos_8: SharedAudio = atmos_pad(bar * 8.0, 65.41).finish("atmos");

    // ---- tracks ----
    let cid_start = 1u64;
    let _ = cid_start;
    let mk = |p: &mut Project, name: &str, sub: &str, color: [u8; 4], kind: TrackKind| {
        let mut t = p.add_track(name, kind, color);
        t.subtitle = sub.to_string();
        t.id
    };

    let _kick_id = mk(&mut p, "KICK", "PUNCH 909", COL_DRUMS, TrackKind::Audio);
    let _snare_id = mk(&mut p, "SNARE", "CLAP & SNAP", COL_DRUMS, TrackKind::Audio);
    let _hat_id = mk(&mut p, "HI HAT", "CLOSED & OPEN", COL_DRUMS, TrackKind::Audio);
    let _tom_id = mk(&mut p, "TOM", "FILLS", COL_DRUMS, TrackKind::Audio);
    let drums_bus = mk(&mut p, "DRUMS BUS", "4 TRACKS", COL_DRUMS, TrackKind::Bus);
    let _bass_id = mk(&mut p, "BASS LINE", "ANALOG SUB", COL_BASS, TrackKind::Audio);
    let _lead_id = mk(&mut p, "SYNTH LEAD", "PLUCK SAW", COL_SYNTH, TrackKind::Audio);
    let _pad_id = mk(&mut p, "PAD ATMOS", "DEEP STRINGS", COL_PAD, TrackKind::Audio);
    let _pluck_id = mk(&mut p, "PLUCK", "ARP SYNTH", COL_SYNTH, TrackKind::Audio);
    let _piano_id = mk(&mut p, "PIANO", "GRAND WARM", COL_PIANO, TrackKind::Audio);
    let vocal_id = mk(&mut p, "VOCAL LEAD", "TAKE 3", COL_VOCAL, TrackKind::Audio);
    let _choir_id = mk(&mut p, "VOCAL CHOIR", "HARMONIES", COL_CHOIR, TrackKind::Audio);
    let _guitar_id = mk(&mut p, "GUITAR", "POWER CHORDS", COL_GUITAR, TrackKind::Audio);
    let _riser_id = mk(&mut p, "FX RISER", "SWEEP UP", COL_FX, TrackKind::Audio);
    let _impact_id = mk(&mut p, "IMPACT", "HITS", COL_FX, TrackKind::Audio);
    let _atmos_id = mk(&mut p, "ATMOS FX", "TEXTURE", COL_ATMOS, TrackKind::Audio);

    // route drums into the bus
    for i in 0..4 {
        p.tracks[i].output_bus = Some(drums_bus);
    }

    // ---- clips ----
    let mut cid = 1u64;
    fn place_clip(p: &mut Project, cid: &mut u64, track: usize, start_bar: f64, bars: f64, a: &SharedAudio, name: &str) {
        let bar = 60.0 / p.tempo * 4.0;
        let mut clip = Clip::with_audio(*cid, name, start_bar * bar, a.clone());
        *cid += 1;
        clip.length = bars * bar;
        p.tracks[track].clips.push(clip);
    }

    // Kick: every beat bars 0-16
    {
        let mut b = 0.0;
        while b < total_bars {
            place_clip(&mut p, &mut cid, 0, b, 0.25, &drums_kick, "KICK");
            place_clip(&mut p, &mut cid, 0, b + 0.5, 0.25, &drums_kick, "KICK");
            b += 1.0;
        }
    }
    // Snare: beats 2 & 4
    {
        let mut b = 0.0;
        while b < total_bars {
            place_clip(&mut p, &mut cid, 1, b + 1.0, 0.25, &drums_snare, "SNARE");
            place_clip(&mut p, &mut cid, 1, b + 3.0, 0.25, &drums_snare, "SNARE");
            b += 1.0;
        }
    }
    // Hats: 8ths
    {
        let mut b = 0.0;
        while b < total_bars {
            for e in 0..8 {
                place_clip(&mut p, &mut cid, 2, b + e as f64 * 0.5, 0.125, &drums_hat, "HAT");
                if let Some(c) = p.tracks[2].clips.last_mut() {
                    c.gain_db = if e % 2 == 1 { -6.0 } else { 0.0 };
                }
            }
            b += 1.0;
        }
    }
    // Toms: fills at end of every 8 bars
    {
        for b in [7.0f64, 15.0] {
            for off in [0.0f64, 0.25, 0.5, 0.75] {
                place_clip(&mut p, &mut cid, 3, b + 3.0 + off, 0.25, &drums_tom, "TOM");
            }
        }
    }
    // Bass: 4-bar loops
    for b in (0..16).step_by(4) {
        place_clip(&mut p, &mut cid, 5, b as f64, 4.0, &bass_loop, "BASS");
    }
    // Lead: bars 4-16
    for b in (4..16).step_by(4) {
        place_clip(&mut p, &mut cid, 6, b as f64, 4.0, &lead_loop, "LEAD");
    }
    // Pad: bars 0-16 (8-bar loops)
    for b in (0..16).step_by(8) {
        place_clip(&mut p, &mut cid, 7, b as f64, 8.0, &pad_chord, "PAD");
    }
    // Pluck: bars 8-16
    for b in (8..16).step_by(4) {
        place_clip(&mut p, &mut cid, 8, b as f64, 4.0, &pluck_seq, "PLUCK");
    }
    // Piano: 4-bar loops from bar 4
    for b in (4..16).step_by(4) {
        place_clip(&mut p, &mut cid, 9, b as f64, 4.0, &piano_chords, "PIANO");
    }
    // Vocal: bars 2-10 (contaminated)
    {
        let clip = Clip::with_audio(cid, "VOCAL TAKE 1", 2.0 * bar, vocal_lead.clone());
        cid += 1;
        p.tracks[10].clips.push(clip);
    }
    // Choir: bars 4-12
    place_clip(&mut p, &mut cid, 11, 4.0, 8.0, &vocal_choir, "CHOIR");
    // Guitar: bars 4-12
    for b in (4..12).step_by(4) {
        place_clip(&mut p, &mut cid, 12, b as f64, 4.0, &guitar_riff, "GTR");
    }
    // Riser into bar 8 and 16
    place_clip(&mut p, &mut cid, 13, 4.0, 4.0, &riser_4bar, "RISE");
    place_clip(&mut p, &mut cid, 13, 12.0, 4.0, &riser_4bar, "RISE");
    // Impacts at 8 and 16
    place_clip(&mut p, &mut cid, 14, 7.95, 2.2, &impact_1, "HIT");
    place_clip(&mut p, &mut cid, 14, 15.9, 2.2, &impact_1, "HIT");
    // Atmos: full length
    place_clip(&mut p, &mut cid, 15, 0.0, 8.0, &atmos_8, "ATMOS");
    place_clip(&mut p, &mut cid, 15, 8.0, 8.0, &atmos_8, "ATMOS");

    // ---- mixer flavor (gain staging + sends like a real session) ----
    let flavor: [(usize, f32, f32, f32, f32); 8] = [
        (0, -3.0, 0.0, 0.05, 0.0),    // kick
        (1, -6.0, 0.05, 0.12, 0.0),   // snare
        (2, -12.0, -0.2, 0.08, 0.0),  // hats
        (3, -10.0, 0.1, 0.15, 0.0),   // toms
        (5, -2.0, 0.0, 0.0, 0.15),    // bass
        (10, -4.0, -0.1, 0.3, 0.25),  // vocal
        (11, -7.0, 0.25, 0.4, 0.2),   // choir
        (15, -14.0, 0.3, 0.5, 0.0),   // atmos
    ];
    for (idx, vol, pan, rev, del) in flavor {
        if let Some(t) = p.tracks.get_mut(idx) {
            t.volume_db = vol;
            t.pan = pan;
            t.reverb_send = rev;
            t.delay_send = del;
        }
    }
    // channel FX (real-session chains so the mixer rack looks lived-in)
    {
        use crate::effects::{EffectInstance, EffectType};
        let chains: [(usize, &[EffectType]); 5] = [
            (1, &[EffectType::Compressor, EffectType::Eq3]),       // snare
            (4, &[EffectType::Compressor, EffectType::Eq3]),       // drums bus glue
            (10, &[EffectType::Eq3, EffectType::Compressor, EffectType::DeEsser]), // vocal
            (9, &[EffectType::Eq3]),                                // piano
            (12, &[EffectType::Chorus]),                            // guitar
        ];
        for (track_idx, types) in chains {
            let built: Vec<EffectInstance> = types
                .iter()
                .map(|et| {
                    let mut fx = EffectInstance::new(*et, 0);
                    fx.uid = p.alloc_id();
                    fx
                })
                .collect();
            if let Some(t) = p.tracks.get_mut(track_idx) {
                t.fx.extend(built);
            }
        }
        // vocal reverb send for depth
        if let Some(vt) = p.track_by_id_mut(vocal_id) {
            vt.reverb_send = 0.3;
        }
    }

    // master chain
    p.master_fx = vec![
        crate::effects::EffectInstance::new(crate::effects::EffectType::Eq3, 9001),
        crate::effects::EffectInstance::new(crate::effects::EffectType::Compressor, 9002),
        crate::effects::EffectInstance::new(crate::effects::EffectType::Limiter, 9003),
    ];

    // demo takes on the vocal track for comping
    if let Some(vt) = p.track_by_id_mut(vocal_id) {
        vt.takes = vec![TakeLane {
            name: "Take 1 — Main".into(),
            take_id: 0,
            color: COL_VOCAL,
        }];
        vt.volume_db = -4.0;
        vt.reverb_send = 0.28;
        vt.delay_send = 0.18;
    }

    p
}

fn chords_cleanup() {}

/// Stress project: N audio tracks with shared clips — engine stability test.
pub fn build_stress_project(n_tracks: usize, clip_density: f64) -> Project {
    let mut p = Project::new_empty("Stress Test");
    let mut rng = rand::thread_rng();
    p.tempo = 124.0;
    let beat = 60.0 / p.tempo;
    let total = beat * 4.0 * 16.0;
    // one shared 2-second texture to keep memory sane (Arc)
    let tex = noise_burst(2.0, 1.4, false, 220.0, 0.2).finish("tex");
    let mut cid = 1u64;
    let colors = [COL_DRUMS, COL_BASS, COL_SYNTH, COL_PIANO, COL_VOCAL, COL_GUITAR, COL_FX];
    for i in 0..n_tracks {
        let color = colors[i % colors.len()];
        let t = p.add_track(&format!("STRESS {:04}", i + 1), TrackKind::Audio, color);
        let start = (i as f64 * 0.37) % total;
        let mut cpos = start;
        let mut step = 0;
        while cpos < total {
            if rng.gen::<f64>() < clip_density {
                let mut clip = Clip::with_audio(cid, &format!("CLIP {}", step + 1), cpos, tex.clone());
                cid += 1;
                clip.length = clip.length.min(total - cpos);
                clip.gain_db = -6.0;
                t.clips.push(clip);
                cpos += 2.3;
            } else {
                cpos += 0.9;
            }
            step += 1;
        }
        t.volume_db = -8.0;
        t.pan = (i % 7) as f32 / 6.0 - 0.5;
    }
    p
}

/// A fresh vocal recording session: music bed + armed empty vocal track.
pub fn build_vocal_session() -> Project {
    let mut p = Project::new_empty("Vocal Session");
    p.tempo = 124.0;
    let beat = 60.0 / p.tempo;
    let bar = beat * 4.0;
    let bed = atmos_pad(bar * 8.0, 110.0).finish("bed");
    let bass = {
        let n = frames(bar * 8.0);
        let mut s = Sig::new(n);
        let mut pos = 0usize;
        for b in 0..32 {
            let f = if b % 8 < 4 { 55.0 } else { 65.41 };
            let note = tone_note(f, beat as f64 * 0.9, 0, 0.5, 0.002, 480.0);
            for (i, v) in note.v.iter().enumerate() {
                if pos + i < n {
                    s.v[pos + i] += v * 1.3;
                }
            }
            pos += frames(beat as f64);
        }
        s.finish("bass")
    };
    let mut bed_t = p.add_track("MUSIC BED", TrackKind::Audio, COL_ATMOS);
    let mut clip = Clip::with_audio(1, "BED", 0.0, bed);
    clip.length = bar * 8.0;
    bed_t.clips.push(clip);
    bed_t.volume_db = -8.0;
    let mut bass_t = p.add_track("BASS", TrackKind::Audio, COL_BASS);
    let mut clip2 = Clip::with_audio(2, "BASS", 0.0, bass);
    clip2.length = bar * 8.0;
    bass_t.clips.push(clip2);
    bass_t.volume_db = -4.0;
    let voc = p.add_track("VOCAL LEAD", TrackKind::Audio, COL_VOCAL);
    voc.armed = true;
    voc.monitoring = true;
    voc.reverb_send = 0.25;
    p
}
