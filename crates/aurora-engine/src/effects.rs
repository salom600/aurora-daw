//! Effect rack definitions — serializable instances + parameter metadata
//! used by both the engine (to build DSP) and the UI (to draw controls).

use serde::{Deserialize, Serialize};

use crate::dsp::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectType {
    Eq3,
    Compressor,
    Reverb,
    Delay,
    Chorus,
    Saturation,
    Limiter,
    Gate,
    Flanger,
    Phaser,
    DeEsser,
}

impl EffectType {
    pub fn name(&self) -> &'static str {
        match self {
            EffectType::Eq3 => "EQ",
            EffectType::Compressor => "Compressor",
            EffectType::Reverb => "Reverb",
            EffectType::Delay => "Delay",
            EffectType::Chorus => "Chorus",
            EffectType::Saturation => "Saturation",
            EffectType::Limiter => "Limiter",
            EffectType::Gate => "Gate",
            EffectType::Flanger => "Flanger",
            EffectType::Phaser => "Phaser",
            EffectType::DeEsser => "De-Esser",
        }
    }
    pub fn category(&self) -> &'static str {
        match self {
            EffectType::Eq3 => "Equalizer",
            EffectType::Compressor | EffectType::Limiter | EffectType::Gate => "Dynamics",
            EffectType::Reverb | EffectType::Delay => "Space",
            EffectType::Chorus | EffectType::Flanger | EffectType::Phaser => "Modulation",
            EffectType::Saturation => "Harmonics",
            EffectType::DeEsser => "Dynamics",
        }
    }
}

/// UI-facing parameter descriptor.
#[derive(Clone, Debug)]
pub struct ParamDef {
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    /// Display transform: none, or log for frequencies.
    pub log: bool,
}

#[derive(Clone, Debug)]
pub struct EffectDef {
    pub etype: EffectType,
    pub params: Vec<ParamDef>,
}

pub fn effect_defs(t: EffectType) -> Vec<ParamDef> {
    use ParamDef as P;
    let p = |name: &'static str, min: f32, max: f32, default: f32, unit: &'static str, log: bool| P {
        name,
        min,
        max,
        default,
        unit,
        log,
    };
    match t {
        EffectType::Eq3 => vec![
            p("Low Gain", -18.0, 18.0, 0.0, "dB", false),
            p("Mid Freq", 200.0, 6000.0, 1000.0, "Hz", true),
            p("Mid Gain", -18.0, 18.0, 0.0, "dB", false),
            p("Mid Q", 0.3, 6.0, 0.9, "", false),
            p("High Gain", -18.0, 18.0, 0.0, "dB", false),
        ],
        EffectType::Compressor => vec![
            p("Threshold", -60.0, 0.0, -20.0, "dB", false),
            p("Ratio", 1.0, 20.0, 3.5, ":1", false),
            p("Attack", 0.1, 80.0, 8.0, "ms", true),
            p("Release", 5.0, 600.0, 120.0, "ms", true),
            p("Makeup", -6.0, 24.0, 3.0, "dB", false),
        ],
        EffectType::Reverb => vec![
            p("Room Size", 0.05, 1.0, 0.62, "", false),
            p("Damping", 0.0, 1.0, 0.35, "", false),
            p("Wet", 0.0, 1.0, 0.25, "", false),
            p("Dry", 0.0, 1.0, 0.85, "", false),
            p("Width", 0.0, 1.0, 1.0, "", false),
            p("Predelay", 0.0, 120.0, 12.0, "ms", false),
        ],
        EffectType::Delay => vec![
            p("Time", 5.0, 1500.0, 375.0, "ms", true),
            p("Feedback", 0.0, 0.92, 0.38, "", false),
            p("Mix", 0.0, 1.0, 0.24, "", false),
            p("Width", 0.0, 1.0, 1.0, "", false),
        ],
        EffectType::Chorus => vec![
            p("Rate", 0.05, 6.0, 0.6, "Hz", true),
            p("Depth", 0.2, 8.0, 3.2, "ms", false),
            p("Mix", 0.0, 1.0, 0.32, "", false),
            p("Voices", 1.0, 4.0, 3.0, "", false),
        ],
        EffectType::Saturation => vec![
            p("Drive", 0.0, 1.0, 0.32, "", false),
            p("Mix", 0.0, 1.0, 0.85, "", false),
            p("Trim", -12.0, 6.0, 0.0, "dB", false),
        ],
        EffectType::Limiter => vec![
            p("Threshold", -24.0, 0.0, -1.5, "dB", false),
            p("Ceiling", -12.0, 0.0, -1.0, "dB", false),
            p("Release", 10.0, 300.0, 60.0, "ms", false),
        ],
        EffectType::Gate => vec![
            p("Threshold", -80.0, -10.0, -46.0, "dB", false),
            p("Range", -60.0, 0.0, -26.0, "dB", false),
            p("Attack", 0.1, 20.0, 2.0, "ms", false),
            p("Release", 10.0, 500.0, 150.0, "ms", false),
            p("Hold", 0.0, 300.0, 40.0, "ms", false),
        ],
        EffectType::Flanger => vec![
            p("Rate", 0.02, 3.0, 0.25, "Hz", true),
            p("Depth", 0.5, 8.0, 3.5, "ms", false),
            p("Feedback", 0.0, 0.9, 0.55, "", false),
            p("Mix", 0.0, 1.0, 0.5, "", false),
        ],
        EffectType::Phaser => vec![
            p("Rate", 0.02, 4.0, 0.4, "Hz", true),
            p("Depth", 0.0, 1.0, 0.7, "", false),
            p("Feedback", 0.0, 0.9, 0.4, "", false),
            p("Mix", 0.0, 1.0, 0.5, "", false),
        ],
        EffectType::DeEsser => vec![
            p("Threshold", -50.0, -6.0, -26.0, "dB", false),
            p("Freq", 3000.0, 10000.0, 6500.0, "Hz", true),
            p("Range", -24.0, 0.0, -9.0, "dB", false),
        ],
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EffectInstance {
    pub uid: u64,
    pub etype: EffectType,
    pub enabled: bool,
    /// Real-valued parameters (same order as effect_defs()).
    pub params: Vec<f32>,
}

impl EffectInstance {
    pub fn new(etype: EffectType, uid: u64) -> Self {
        Self {
            uid,
            etype,
            enabled: true,
            params: effect_defs(etype).iter().map(|p| p.default).collect(),
        }
    }
}

/// Runtime DSP unit built from an EffectInstance. Lives on the audio side.
pub enum FxUnit {
    Eq3(Box<Eq3>),
    Comp(Box<Compressor>),
    Reverb(Box<Reverb>),
    Delay(Box<Delay>),
    Chorus(Box<Chorus>),
    Sat(Box<Saturator>),
    Limiter(Box<Limiter>),
    Gate(Box<Gate>),
    Flanger(Box<Flanger>),
    Phaser(Box<Phaser>),
    DeEsser(Box<DeEsser>),
    Bypassed,
}

impl FxUnit {
    pub fn build(inst: &EffectInstance, sr: f32) -> Self {
        if !inst.enabled {
            return FxUnit::Bypassed;
        }
        let g = |i: usize, d: f32| -> f32 { inst.params.get(i).copied().unwrap_or(d) };
        match inst.etype {
            EffectType::Eq3 => {
                let mut eq = Eq3::new(sr);
                eq.mid_freq = g(1, 1000.0);
                eq.update(sr, g(0, 0.0), g(2, 0.0), g(4, 0.0), g(3, 0.9));
                FxUnit::Eq3(Box::new(eq))
            }
            EffectType::Compressor => {
                let mut c = Compressor::new(sr);
                c.threshold_db = g(0, -20.0);
                c.ratio = g(1, 3.5);
                c.attack_ms = g(2, 8.0);
                c.release_ms = g(3, 120.0);
                c.makeup_db = g(4, 3.0);
                c.set_sr(sr);
                FxUnit::Comp(Box::new(c))
            }
            EffectType::Reverb => {
                let mut r = Reverb::new(sr);
                r.room_size = g(0, 0.62);
                r.damping = g(1, 0.35);
                r.wet = g(2, 0.25);
                r.dry = g(3, 0.85);
                r.width = g(4, 1.0);
                r.predelay_ms = g(5, 12.0);
                r.set_sr(sr);
                FxUnit::Reverb(Box::new(r))
            }
            EffectType::Delay => {
                let mut d = Delay::new(sr);
                d.set_time(g(0, 375.0));
                d.feedback = g(1, 0.38);
                d.mix = g(2, 0.24);
                d.width = g(3, 1.0);
                FxUnit::Delay(Box::new(d))
            }
            EffectType::Chorus => {
                let mut c = Chorus::new(sr);
                c.rate_hz = g(0, 0.6);
                c.depth_ms = g(1, 3.2);
                c.mix = g(2, 0.32);
                c.voices = g(3, 3.0) as u32;
                FxUnit::Chorus(Box::new(c))
            }
            EffectType::Saturation => {
                let mut s = Saturator::new();
                s.drive = g(0, 0.32);
                s.mix = g(1, 0.85);
                s.output_trim_db = g(2, 0.0);
                FxUnit::Sat(Box::new(s))
            }
            EffectType::Limiter => {
                let mut l = Limiter::new(sr);
                l.threshold_db = g(0, -1.5);
                l.ceiling_db = g(1, -1.0);
                l.release_ms = g(2, 60.0);
                l.set_sr(sr);
                FxUnit::Limiter(Box::new(l))
            }
            EffectType::Gate => {
                let mut gt = Gate::new(sr);
                gt.threshold_db = g(0, -46.0);
                gt.range_db = g(1, -26.0);
                gt.attack_ms = g(2, 2.0);
                gt.release_ms = g(3, 150.0);
                gt.hold_ms = g(4, 40.0);
                gt.set_sr(sr);
                FxUnit::Gate(Box::new(gt))
            }
            EffectType::Flanger => {
                let mut f = Flanger::new(sr);
                f.rate_hz = g(0, 0.25);
                f.depth_ms = g(1, 3.5);
                f.feedback = g(2, 0.55);
                f.mix = g(3, 0.5);
                FxUnit::Flanger(Box::new(f))
            }
            EffectType::Phaser => {
                let mut p = Phaser::new(sr);
                p.rate_hz = g(0, 0.4);
                p.depth = g(1, 0.7);
                p.feedback = g(2, 0.4);
                p.mix = g(3, 0.5);
                FxUnit::Phaser(Box::new(p))
            }
            EffectType::DeEsser => {
                let mut d = DeEsser::new(sr);
                d.threshold_db = g(0, -26.0);
                d.freq_hz = g(1, 6500.0);
                d.range_db = g(2, -9.0);
                d.set_sr(sr);
                FxUnit::DeEsser(Box::new(d))
            }
        }
    }

    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        match self {
            FxUnit::Eq3(u) => u.process(l, r),
            FxUnit::Comp(u) => u.process(l, r),
            FxUnit::Reverb(u) => u.process(l, r),
            FxUnit::Delay(u) => u.process(l, r),
            FxUnit::Chorus(u) => u.process(l, r),
            FxUnit::Sat(u) => u.process(l, r),
            FxUnit::Limiter(u) => u.process(l, r),
            FxUnit::Gate(u) => u.process(l, r),
            FxUnit::Flanger(u) => u.process(l, r),
            FxUnit::Phaser(u) => u.process(l, r),
            FxUnit::DeEsser(u) => u.process(l, r),
            FxUnit::Bypassed => {}
        }
    }

    pub fn gain_reduction_db(&self) -> f32 {
        match self {
            FxUnit::Comp(u) => u.gain_reduction,
            FxUnit::Limiter(u) => u.gain_reduction,
            FxUnit::DeEsser(u) => u.gain_reduction,
            _ => 0.0,
        }
    }
}

/// Default factory presets for one-click chains.
pub fn preset_chain_vocal() -> Vec<EffectType> {
    vec![
        EffectType::Eq3,
        EffectType::Compressor,
        EffectType::DeEsser,
        EffectType::Reverb,
    ]
}

pub fn preset_chain_master() -> Vec<EffectType> {
    vec![EffectType::Eq3, EffectType::Compressor, EffectType::Limiter]
}
