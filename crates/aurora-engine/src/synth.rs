//! AURORA polyphonic subtractive synth — renders instrument clip notes.

use crate::project::{Note, SynthPatch};

pub const POLY: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnvStage {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
pub struct Voice {
    pub key: u8,
    pub freq: f32,
    pub vel: f32,
    pub phase: f32,
    pub phase2: f32,
    pub env: f32,
    pub stage: EnvStage,
    pub level: f32,
    pub note_len_samples: f32,
    pub samples_played: f32,
    pub lp_state: f32,
}

pub fn key_freq(key: u8) -> f32 {
    // key 0 = C2 (65.41 Hz), chromatic
    65.406 * 2f32.powf(key as f32 / 12.0)
}

pub struct PolySynth {
    pub patch: SynthPatch,
    voices: Vec<Voice>,
    sr: f32,
    pub active_voices: usize,
}

impl PolySynth {
    pub fn new(sr: f32) -> Self {
        Self {
            patch: SynthPatch::default(),
            voices: Vec::with_capacity(POLY),
            sr,
            active_voices: 0,
        }
    }

    pub fn set_patch(&mut self, p: SynthPatch) {
        self.patch = p;
    }

    pub fn note_on(&mut self, key: u8, vel: f32, note_len_samples: f32) {
        if self.voices.len() >= POLY {
            // steal quietest
            let mut victim: Option<Voice> = None;
            if let Some((idx, _)) = self
                .voices
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.env.total_cmp(&b.env))
            {
                victim = Some(self.voices[idx]);
            }
            if let Some(mut v) = victim {
                start_voice(&mut v, key, vel, note_len_samples);
                if let Some(slot) = self.voices.iter_mut().find(|x| x.key == v.key) {
                    *slot = v;
                }
            }
            return;
        }
        let mut v = Voice {
            key: 255,
            freq: 0.0,
            vel: 0.0,
            phase: 0.0,
            phase2: 0.0,
            env: 0.0,
            stage: EnvStage::Off,
            level: 0.0,
            note_len_samples: 0.0,
            samples_played: 0.0,
            lp_state: 0.0,
        };
        start_voice(&mut v, key, vel, note_len_samples);
        self.voices.push(v);
    }

    pub fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            if v.stage != EnvStage::Off {
                v.stage = EnvStage::Release;
            }
        }
    }

    /// Render `frames` mono samples. Notes scheduled externally via note_on
    /// with note_len_samples; voices auto-release.
    pub fn render_mono(&mut self, out: &mut [f32]) {
        let sr = self.sr;
        let p = self.patch.clone();
        let detune_ratio = 2f32.powf(p.detune * 0.08);
        let lp_coeff = {
            let cutoff = p.cutoff.clamp(120.0, 18000.0) / sr;
            let g = (std::f32::consts::TAU * cutoff).min(0.95);
            g / (g + 1.0)
        };
        out.fill(0.0);
        for v in &mut self.voices {
            if v.stage == EnvStage::Off {
                continue;
            }
            // per-sample loop
            for s in out.iter_mut() {
                v.samples_played += 1.0;
                if v.samples_played >= v.note_len_samples && v.stage != EnvStage::Release {
                    v.stage = EnvStage::Release;
                }
                // envelope
                match v.stage {
                    EnvStage::Attack => {
                        v.env += 1.0 / (p.attack.max(0.001) * sr);
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.stage = EnvStage::Decay;
                        }
                    }
                    EnvStage::Decay => {
                        v.env -= (1.0 - p.sustain) / (p.decay.max(0.01) * sr);
                        if v.env <= p.sustain {
                            v.env = p.sustain;
                            v.stage = EnvStage::Sustain;
                        }
                    }
                    EnvStage::Sustain => v.env = p.sustain,
                    EnvStage::Release => {
                        v.env -= v.env.max(0.2) / (p.release.max(0.01) * sr);
                        if v.env <= 0.0005 {
                            v.env = 0.0;
                            v.stage = EnvStage::Off;
                            break;
                        }
                    }
                    EnvStage::Off => break,
                }
                // oscillators (detuned pair)
                let inc = v.freq / sr;
                let inc2 = inc * detune_ratio;
                v.phase = (v.phase + inc) % 1.0;
                v.phase2 = (v.phase2 + inc2) % 1.0;
                let o1 = osc(v.phase, p.waveform);
                let o2 = osc(v.phase2, p.waveform);
                let mut sample = (o1 + o2) * 0.5;
                // one-pole low pass with a hint of resonance
                v.lp_state += lp_coeff * (sample - v.lp_state);
                sample = v.lp_state + (sample - v.lp_state) * p.resonance * 0.08;
                *s += sample * v.env * v.vel * p.gain * 0.5;
            }
        }
        self.voices.retain(|v| v.stage != EnvStage::Off);
        self.active_voices = self.voices.len();
    }
}

#[inline]
fn osc(phase: f32, waveform: u8) -> f32 {
    match waveform {
        0 => saw_polyblep(phase),
        1 => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        2 => (phase * std::f32::consts::TAU).sin(),
        _ => {
            // triangle
            if phase < 0.5 {
                4.0 * phase - 1.0
            } else {
                3.0 - 4.0 * phase
            }
        }
    }
}

/// Band-limited-ish saw via polyblep (reduces aliasing).
#[inline]
fn saw_polyblep(phase: f32) -> f32 {
    let mut y = 2.0 * phase - 1.0;
    // polyblep correction
    let mut t = phase;
    if t < 0.5 {
        let tt = t * 2.0;
        y -= tt * tt - 2.0 * tt + 1.0;
    }
    t = phase + 0.5;
    if t >= 1.0 {
        t -= 1.0;
        let tt = t * 2.0;
        y += tt * tt - 2.0 * tt + 1.0;
    }
    y
}

/// Schedule notes whose start falls inside the clip-local window [win0, win1) beats.
pub fn schedule_notes(
    synth: &mut PolySynth,
    notes: &[Note],
    win0: f64,
    win1: f64,
    tempo: f64,
    sr: f32,
) {
    for n in notes {
        let ns = n.start_beats as f64;
        let ne = ns + n.len_beats as f64;
        if ns >= win0 && ns < win1 {
            let len_samples = ((ne - ns) * (60.0 / tempo) * sr as f64) as f32;
            synth.note_on(n.key, n.vel, len_samples.max(64.0));
        }
    }
}


fn start_voice(v: &mut Voice, key: u8, vel: f32, note_len_samples: f32) {
    v.key = key;
    v.freq = key_freq(key);
    v.vel = vel.clamp(0.0, 1.2);
    v.phase = 0.0;
    v.phase2 = 0.13;
    v.env = 0.0;
    v.stage = EnvStage::Attack;
    v.note_len_samples = note_len_samples.max(1.0);
    v.samples_played = 0.0;
    v.lp_state = 0.0;
}
