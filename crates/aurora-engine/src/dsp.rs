//! AURORA DSP core — real-time audio processing units.
//!
//! Every effect in the mixer rack runs through these structs on the audio
//! thread. All units are allocated at effect-sync time (never in the callback
//! where avoidable) and process stereo samples per block.

use serde::{Deserialize, Serialize};

pub const PI: f32 = std::f32::consts::PI;
pub const TAU: f32 = std::f32::consts::TAU;

#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    if db <= -96.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

#[inline]
pub fn lin_to_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-7).log10()
}

#[inline]
pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Biquad filter (RBJ cookbook, CH=1 form)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
    z1: f64,
    z2: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
}

impl Biquad {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn design(kind: FilterKind, sr: f32, freq: f64, gain_db: f64, q: f64) -> Self {
        let mut b = Self::new();
        b.set(kind, sr, freq, gain_db, q);
        b
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set(&mut self, kind: FilterKind, sr: f32, freq: f64, gain_db: f64, q: f64) {
        let f0 = freq.clamp(10.0, sr as f64 * 0.49);
        let w0 = TAU as f64 * f0 / sr as f64;
        let cosw0 = w0.cos();
        let sinw0 = w0.sin();
        let alpha = sinw0 / (2.0 * q);
        let a = 10f64.powf(gain_db / 40.0); // shelf/peak slope factor

        let (b0, b1, b2, a0, a1, a2);
        match kind {
            FilterKind::LowPass => {
                b0 = (1.0 - cosw0) / 2.0;
                b1 = 1.0 - cosw0;
                b2 = (1.0 - cosw0) / 2.0;
                a0 = 1.0 + alpha;
                a1 = -2.0 * cosw0;
                a2 = 1.0 - alpha;
            }
            FilterKind::HighPass => {
                b0 = (1.0 + cosw0) / 2.0;
                b1 = -(1.0 + cosw0);
                b2 = (1.0 + cosw0) / 2.0;
                a0 = 1.0 + alpha;
                a1 = -2.0 * cosw0;
                a2 = 1.0 - alpha;
            }
            FilterKind::BandPass => {
                b0 = alpha;
                b1 = 0.0;
                b2 = -alpha;
                a0 = 1.0 + alpha;
                a1 = -2.0 * cosw0;
                a2 = 1.0 - alpha;
            }
            FilterKind::Notch => {
                b0 = 1.0;
                b1 = -2.0 * cosw0;
                b2 = 1.0;
                a0 = 1.0 + alpha;
                a1 = -2.0 * cosw0;
                a2 = 1.0 - alpha;
            }
            FilterKind::Peak => {
                b0 = 1.0 + alpha * a;
                b1 = -2.0 * cosw0;
                b2 = 1.0 - alpha * a;
                a0 = 1.0 + alpha / a;
                a1 = -2.0 * cosw0;
                a2 = 1.0 - alpha / a;
            }
            FilterKind::LowShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                b0 = a * ((a + 1.0) - (a - 1.0) * cosw0 + sq);
                b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cosw0);
                b2 = a * ((a + 1.0) - (a - 1.0) * cosw0 - sq);
                a0 = (a + 1.0) + (a - 1.0) * cosw0 + sq;
                a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cosw0);
                a2 = (a + 1.0) + (a - 1.0) * cosw0 - sq;
            }
            FilterKind::HighShelf => {
                let sq = 2.0 * a.sqrt() * alpha;
                b0 = a * ((a + 1.0) + (a - 1.0) * cosw0 + sq);
                b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cosw0);
                b2 = a * ((a + 1.0) + (a - 1.0) * cosw0 - sq);
                a0 = (a + 1.0) - (a - 1.0) * cosw0 + sq;
                a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cosw0);
                a2 = (a + 1.0) - (a - 1.0) * cosw0 - sq;
            }
        }
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        // Transposed Direct Form II
        let y = (self.b0 * x as f64) + self.z1;
        self.z1 = (self.b1 * x as f64) - (self.a1 * y) + self.z2;
        self.z2 = (self.b2 * x as f64) - (self.a2 * y);
        y as f32
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    /// Magnitude response (linear) at normalized frequency `f / nyquist`.
    pub fn magnitude(&self, freq: f32, sr: f32) -> f32 {
        let w = TAU * freq / sr;
        let (sw, cw) = w.sin_cos();
        let c2 = (2.0 * w).cos();
        let s2 = (2.0 * w).sin();
        let num_re = self.b0 as f32 + (self.b1 as f32 * cw + self.b2 as f32 * c2);
        let num_im = -(self.b1 as f32 * sw + self.b2 as f32 * s2);
        let den_re = 1.0 + (self.a1 as f32 * cw + self.a2 as f32 * c2);
        let den_im = -(self.a1 as f32 * sw + self.a2 as f32 * s2);
        ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im + 1e-20)).sqrt()
    }
}

/// Stereo biquad (independent states per channel).
#[derive(Clone, Debug, Default)]
pub struct StereoBiquad {
    pub l: Biquad,
    pub r: Biquad,
}

impl StereoBiquad {
    pub fn design(kind: FilterKind, sr: f32, freq: f64, gain_db: f64, q: f64) -> Self {
        Self {
            l: Biquad::design(kind, sr, freq, gain_db, q),
            r: Biquad::design(kind, sr, freq, gain_db, q),
        }
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        *l = self.l.process(*l);
        *r = self.r.process(*r);
    }
    pub fn reset(&mut self) {
        self.l.reset();
        self.r.reset();
    }
}

// ---------------------------------------------------------------------------
// Envelope follower
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Envelope {
    coeff_attack: f32,
    coeff_release: f32,
    env: f32,
}

impl Envelope {
    pub fn new(sr: f32, attack_ms: f32, release_ms: f32) -> Self {
        let mut e = Self {
            coeff_attack: 0.0,
            coeff_release: 0.0,
            env: 0.0,
        };
        e.set_times(sr, attack_ms, release_ms);
        e
    }
    pub fn set_times(&mut self, sr: f32, attack_ms: f32, release_ms: f32) {
        let a = (-1.0 / (attack_ms.max(0.01) * 0.001 * sr)).exp();
        let r = (-1.0 / (release_ms.max(0.01) * 0.001 * sr)).exp();
        self.coeff_attack = a;
        self.coeff_release = r;
    }
    #[inline]
    pub fn push(&mut self, x: f32) -> f32 {
        let v = x.abs();
        if v > self.env {
            self.env = self.coeff_attack * self.env + (1.0 - self.coeff_attack) * v;
        } else {
            self.env = self.coeff_release * self.env + (1.0 - self.coeff_release) * v;
        }
        self.env
    }
    pub fn value(&self) -> f32 {
        self.env
    }
    pub fn reset(&mut self) {
        self.env = 0.0;
    }
}

// ---------------------------------------------------------------------------
// 3-band EQ (low shelf, mid peak, high shelf)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Eq3 {
    pub low: StereoBiquad,
    pub mid: StereoBiquad,
    pub high: StereoBiquad,
    pub low_freq: f32,
    pub mid_freq: f32,
    pub high_freq: f32,
}

impl Eq3 {
    pub fn new(sr: f32) -> Self {
        let mut eq = Self {
            low: StereoBiquad::default(),
            mid: StereoBiquad::default(),
            high: StereoBiquad::default(),
            low_freq: 120.0,
            mid_freq: 1000.0,
            high_freq: 8000.0,
        };
        eq.update(sr, 0.0, 0.0, 0.0, 0.7);
        eq
    }
    pub fn update(&mut self, sr: f32, low_db: f32, mid_db: f32, high_db: f32, mid_q: f32) {
        self.low = StereoBiquad::design(FilterKind::LowShelf, sr, self.low_freq as f64, low_db as f64, 0.7);
        self.mid = StereoBiquad::design(FilterKind::Peak, sr, self.mid_freq as f64, mid_db as f64, mid_q as f64);
        self.high = StereoBiquad::design(FilterKind::HighShelf, sr, self.high_freq as f64, high_db as f64, 0.7);
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        self.low.process(l, r);
        self.mid.process(l, r);
        self.high.process(l, r);
    }
    pub fn reset(&mut self) {
        self.low.reset();
        self.mid.reset();
        self.high.reset();
    }
    /// Combined magnitude response for the UI curve.
    pub fn response(&self, freq: f32, sr: f32) -> f32 {
        let m = self.low.l.magnitude(freq, sr)
            * self.mid.l.magnitude(freq, sr)
            * self.high.l.magnitude(freq, sr);
        m
    }
}

// ---------------------------------------------------------------------------
// Compressor (hard-wired soft knee, envelope follower based)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Compressor {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
    env: Envelope,
    pub gain_reduction: f32, // smoothed, for metering
    gr_smooth: f32,
}

impl Compressor {
    pub fn new(sr: f32) -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 3.0,
            attack_ms: 10.0,
            release_ms: 120.0,
            makeup_db: 0.0,
            env: Envelope::new(sr, 10.0, 120.0),
            gain_reduction: 0.0,
            gr_smooth: 0.0,
        }
    }
    pub fn set_sr(&mut self, sr: f32) {
        self.env.set_times(sr, self.attack_ms, self.release_ms);
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let det = (*l).abs().max((*r).abs());
        let env = self.env.push(det);
        let env_db = lin_to_db(env);
        let over = env_db - self.threshold_db;
        let gr = if over > 0.0 {
            over * (1.0 / self.ratio.max(1.0) - 1.0)
        } else {
            0.0
        };
        self.gr_smooth = self.gr_smooth * 0.85 + gr * 0.15; // fast display smoothing
        let gain = db_to_lin(gr + self.makeup_db);
        *l *= gain;
        *r *= gain;
        self.gain_reduction = -self.gr_smooth;
    }
}

// ---------------------------------------------------------------------------
// Noise gate
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Gate {
    pub threshold_db: f32,
    pub range_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub hold_ms: f32,
    env: Envelope,
    hold_samples: u32,
    hold_left: u32,
    gain_smooth: f32,
}

impl Gate {
    pub fn new(sr: f32) -> Self {
        Self {
            threshold_db: -45.0,
            range_db: -24.0,
            attack_ms: 2.0,
            release_ms: 150.0,
            hold_ms: 40.0,
            env: Envelope::new(sr, 2.0, 150.0),
            hold_samples: (sr * 0.04) as u32,
            hold_left: 0,
            gain_smooth: 0.0,
        }
    }
    pub fn set_sr(&mut self, sr: f32) {
        self.hold_samples = (sr * self.hold_ms * 0.001) as u32;
        self.env.set_times(sr, self.attack_ms, self.release_ms);
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let det = (*l).abs().max((*r).abs());
        let env = self.env.push(det);
        let open = lin_to_db(env) > self.threshold_db;
        let target = if open {
            0.0f32
        } else {
            self.hold_left = self.hold_samples;
            self.range_db
        };
        // ramp toward target range
        let coeff = if target > self.gain_smooth { 0.004 } else { 0.0008 };
        self.gain_smooth += (target - self.gain_smooth) * coeff;
        if open && self.hold_left > 0 {
            self.hold_left -= 1;
        }
        let g = db_to_lin(self.gain_smooth);
        *l *= g;
        *r *= g;
    }
}

// ---------------------------------------------------------------------------
// Freeverb-inspired reverb (8 combs + 4 allpasses per channel)
// ---------------------------------------------------------------------------

struct Comb {
    buf: Vec<f32>,
    idx: usize,
    filter_store: f32,
    damp1: f32,
    feedback: f32,
}

impl Comb {
    fn new(size: usize) -> Self {
        Self {
            buf: vec![0.0; size.max(4)],
            idx: 0,
            filter_store: 0.0,
            damp1: 0.2,
            feedback: 0.8,
        }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let out = self.buf[self.idx];
        self.filter_store = out * (1.0 - self.damp1) + self.filter_store * self.damp1;
        self.buf[self.idx] = x + self.filter_store * self.feedback;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

struct Allpass {
    buf: Vec<f32>,
    idx: usize,
}

impl Allpass {
    fn new(size: usize) -> Self {
        Self {
            buf: vec![0.0; size.max(4)],
            idx: 0,
        }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.idx];
        let out = -x + b;
        self.buf[self.idx] = x + b * 0.5;
        self.idx = (self.idx + 1) % self.buf.len();
        out
    }
}

const COMB_TUNING: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_TUNING: [usize; 4] = [556, 441, 341, 225];

pub struct Reverb {
    pub room_size: f32,  // 0..1
    pub damping: f32,    // 0..1
    pub width: f32,      // 0..1
    pub wet: f32,        // 0..1
    pub dry: f32,        // 0..1
    pub predelay_ms: f32,
    combs_l: Vec<Comb>,
    combs_r: Vec<Comb>,
    aps_l: Vec<Allpass>,
    aps_r: Vec<Allpass>,
    predelay: Vec<f32>,
    predelay_idx: usize,
    sr_scale: f32,
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let mut rv = Self {
            room_size: 0.6,
            damping: 0.4,
            width: 1.0,
            wet: 0.3,
            dry: 0.8,
            predelay_ms: 12.0,
            combs_l: Vec::new(),
            combs_r: Vec::new(),
            aps_l: Vec::new(),
            aps_r: Vec::new(),
            predelay: Vec::new(),
            predelay_idx: 0,
            sr_scale: sr / 44100.0,
        };
        rv.build(sr);
        rv
    }
    fn build(&mut self, sr: f32) {
        let s = (sr / 44100.0) as usize;
        self.combs_l = COMB_TUNING.iter().map(|&t| Comb::new(t * s)).collect();
        self.combs_r = COMB_TUNING
            .iter()
            .map(|&t| Comb::new((t + 23) * s))
            .collect();
        self.aps_l = ALLPASS_TUNING.iter().map(|&t| Allpass::new(t * s)).collect();
        self.aps_r = ALLPASS_TUNING
            .iter()
            .map(|&t| Allpass::new((t + 19) * s))
            .collect();
        self.predelay = vec![0.0; ((self.predelay_ms * 0.001 * sr) as usize).max(1)];
        self.sr_scale = sr / 44100.0;
    }
    pub fn set_sr(&mut self, sr: f32) {
        self.build(sr);
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let input = (*l + *r) * 0.5;
        // predelay
        let pd = self.predelay[self.predelay_idx];
        self.predelay[self.predelay_idx] = input;
        self.predelay_idx = (self.predelay_idx + 1) % self.predelay.len();
        let input_mono = pd;

        let damp = self.damping;
        let fb = 0.72 + self.room_size * 0.22;
        let mut out_l = 0.0;
        let mut out_r = 0.0;
        for i in 0..8 {
            self.combs_l[i].damp1 = damp;
            self.combs_l[i].feedback = fb;
            self.combs_r[i].damp1 = damp;
            self.combs_r[i].feedback = fb;
            out_l += self.combs_l[i].process(input_mono);
            out_r += self.combs_r[i].process(input_mono);
        }
        for i in 0..4 {
            out_l = self.aps_l[i].process(out_l);
            out_r = self.aps_r[i].process(out_r);
        }
        // stereo width
        let wet1 = self.wet * (self.width * 0.5 + 0.5);
        let wet2 = self.wet * (1.0 - self.width * 0.5);
        *l = *l * self.dry + out_l * wet1 + out_r * wet2;
        *r = *r * self.dry + out_r * wet1 + out_l * wet2;
        // keep levels sane
        *l *= 0.35;
        *r *= 0.35;
    }
}

// ---------------------------------------------------------------------------
// Stereo ping-pong delay
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Delay {
    pub time_ms: f32,
    pub feedback: f32,
    pub mix: f32,
    pub width: f32, // 0=mono, 1=full ping-pong
    buf_l: Vec<f32>,
    buf_r: Vec<f32>,
    idx_l: usize,
    idx_r: usize,
    len: usize,
    sr: f32,
    lp: StereoBiquad,
}

impl Delay {
    pub fn new(sr: f32) -> Self {
        let max = (sr * 2.0) as usize;
        let mut d = Self {
            time_ms: 375.0,
            feedback: 0.38,
            mix: 0.28,
            width: 1.0,
            buf_l: vec![0.0; max],
            buf_r: vec![0.0; max],
            idx_l: 0,
            idx_r: 0,
            len: 1024,
            sr,
            lp: StereoBiquad::design(FilterKind::LowPass, sr, 9000.0, 0.0, 0.707),
        };
        d.set_time(d.time_ms);
        d
    }
    pub fn set_time(&mut self, ms: f32) {
        self.time_ms = ms.clamp(1.0, 1900.0);
        let n = ((self.time_ms * 0.001 * self.sr) as usize).clamp(8, self.buf_l.len() - 1);
        self.idx_l = 0;
        self.idx_r = n / 2;
        self.len = n;
    }
    // real length storage
    // (kept simple: we recompute indices on set_time)
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let dl = self.buf_l[self.idx_r]; // cross-fed ping-pong
        let dr = self.buf_r[self.idx_l];
        self.buf_l[self.idx_l] = *l + dr * self.feedback;
        self.buf_r[self.idx_r] = *r + dl * self.feedback;
        let n = self.len;
        self.idx_l = (self.idx_l + 1) % n;
        self.idx_r = (self.idx_r + 1) % n;
        let (mut el, mut er) = (dl, dr);
        self.lp.process(&mut el, &mut er);
        let mix = self.mix;
        let w = self.width;
        *l = *l * (1.0 - mix) + (el * w + er * (1.0 - w)) * mix;
        *r = *r * (1.0 - mix) + (er * w + el * (1.0 - w)) * mix;
    }
}

// ---------------------------------------------------------------------------
// Chorus
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Chorus {
    pub rate_hz: f32,
    pub depth_ms: f32,
    pub mix: f32,
    pub voices: u32,
    buf: Vec<f32>,
    write_idx: usize,
    lfo_phase: f32,
    sr: f32,
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        Self {
            rate_hz: 0.6,
            depth_ms: 3.5,
            mix: 0.35,
            voices: 3,
            buf: vec![0.0; (sr * 0.1) as usize],
            write_idx: 0,
            lfo_phase: 0.0,
            sr,
        }
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let input = (*l + *r) * 0.5;
        self.buf[self.write_idx] = input;
        let n = self.buf.len();
        let mut wet = 0.0;
        let voices = self.voices.max(1) as f32;
        for v in 0..self.voices.max(1) {
            let phase = self.lfo_phase + v as f32 * TAU / voices;
            let lfo = (phase.sin() * 0.5 + 0.5) * self.depth_ms * 0.001 * self.sr;
            let read = (self.write_idx as f32 - lfo - 2.0).rem_euclid(n as f32);
            let i0 = read as usize;
            let frac = read - i0 as f32;
            let s0 = self.buf[i0 % n];
            let s1 = self.buf[(i0 + 1) % n];
            wet += s0 + (s1 - s0) * frac;
        }
        wet /= voices;
        self.write_idx = (self.write_idx + 1) % n;
        self.lfo_phase = (self.lfo_phase + TAU * self.rate_hz / self.sr) % TAU;
        *l = *l * (1.0 - self.mix) + wet * self.mix;
        *r = *r * (1.0 - self.mix) + wet * self.mix * 0.96;
    }
}

// ---------------------------------------------------------------------------
// Flanger
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Flanger {
    pub rate_hz: f32,
    pub depth_ms: f32,
    pub feedback: f32,
    pub mix: f32,
    buf: Vec<f32>,
    write_idx: usize,
    lfo_phase: f32,
    sr: f32,
}

impl Flanger {
    pub fn new(sr: f32) -> Self {
        Self {
            rate_hz: 0.25,
            depth_ms: 4.0,
            feedback: 0.55,
            mix: 0.5,
            buf: vec![0.0; (sr * 0.02) as usize + 4],
            write_idx: 0,
            lfo_phase: 0.0,
            sr,
        }
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let input = (*l + *r) * 0.5;
        let n = self.buf.len();
        let delay = (self.lfo_phase.sin() * 0.5 + 0.5) * self.depth_ms * 0.001 * self.sr + 1.5;
        let read = (self.write_idx as f32 - delay).rem_euclid(n as f32);
        let i0 = read as usize;
        let frac = read - i0 as f32;
        let s = self.buf[i0 % n] + (self.buf[(i0 + 1) % n] - self.buf[i0 % n]) * frac;
        self.buf[self.write_idx] = input + s * self.feedback.clamp(-0.9, 0.9);
        self.write_idx = (self.write_idx + 1) % n;
        self.lfo_phase = (self.lfo_phase + TAU * self.rate_hz / self.sr) % TAU;
        *l = *l * (1.0 - self.mix) + s * self.mix;
        *r = *r * (1.0 - self.mix) + s * self.mix;
    }
}

// ---------------------------------------------------------------------------
// Phaser (4-stage allpass)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Phaser {
    pub rate_hz: f32,
    pub depth: f32,
    pub feedback: f32,
    pub mix: f32,
    stages: [f32; 4],
    lfo_phase: f32,
    fb_store: f32,
    sr: f32,
}

impl Phaser {
    pub fn new(_sr: f32) -> Self {
        Self {
            rate_hz: 0.4,
            depth: 0.7,
            feedback: 0.4,
            mix: 0.5,
            stages: [0.0; 4],
            lfo_phase: 0.0,
            fb_store: 0.0,
            sr: _sr,
        }
    }
    #[inline]
    fn allpass(&mut self, stage: usize, x: f32, g: f32) -> f32 {
        let y = self.stages[stage];
        self.stages[stage] = g * y + x * (1.0 - g);
        y - g * self.stages[stage]
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let input = (*l + *r) * 0.5;
        let lfo = self.lfo_phase.sin();
        let g = (0.5 - 0.5 * lfo * self.depth).clamp(0.05, 0.95);
        let mut x = input + self.fb_store * self.feedback.clamp(-0.9, 0.9);
        for s in 0..4 {
            x = self.allpass(s, x, g);
        }
        self.fb_store = x;
        self.lfo_phase = (self.lfo_phase + TAU * self.rate_hz / self.sr) % TAU;
        *l = *l * (1.0 - self.mix) + x * self.mix;
        *r = *r * (1.0 - self.mix) + x * self.mix;
    }
}

// ---------------------------------------------------------------------------
// Saturation (tanh waveshaper + harmonic tone)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Saturator {
    pub drive: f32, // 0..1
    pub mix: f32,
    pub output_trim_db: f32,
}

impl Saturator {
    pub fn new() -> Self {
        Self {
            drive: 0.35,
            mix: 1.0,
            output_trim_db: 0.0,
        }
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let d = 1.0 + self.drive * 14.0;
        let wet_l = (d * *l).tanh() / d.tanh();
        let wet_r = (d * *r).tanh() / d.tanh();
        let g = db_to_lin(self.output_trim_db);
        *l = (*l * (1.0 - self.mix) + wet_l * self.mix) * g;
        *r = (*r * (1.0 - self.mix) + wet_r * self.mix) * g;
    }
}

// ---------------------------------------------------------------------------
// Brickwall limiter (fast release, ceiling)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Limiter {
    pub ceiling_db: f32,
    pub threshold_db: f32,
    pub release_ms: f32,
    env: Envelope,
    gr_smooth: f32,
    pub gain_reduction: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        Self {
            ceiling_db: -1.0,
            threshold_db: -1.5,
            release_ms: 60.0,
            env: Envelope::new(sr, 0.2, 60.0),
            gr_smooth: 0.0,
            gain_reduction: 0.0,
        }
    }
    pub fn set_sr(&mut self, sr: f32) {
        self.env.set_times(sr, 0.2, self.release_ms);
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let det = (*l).abs().max((*r).abs());
        let env = self.env.push(det);
        let env_db = lin_to_db(env);
        let over = env_db - self.threshold_db;
        let gr = if over > 0.0 { over * (1.0 - 1.0 / 20.0) } else { 0.0 };
        self.gr_smooth = self.gr_smooth.max(gr) * 0.995 + gr * 0.005;
        let gain = db_to_lin(-self.gr_smooth);
        *l *= gain;
        *r *= gain;
        // safety clip at ceiling
        let ceil = db_to_lin(self.ceiling_db);
        *l = l.clamp(-ceil, ceil);
        *r = r.clamp(-ceil, ceil);
        self.gain_reduction = -self.gr_smooth;
    }
}

// ---------------------------------------------------------------------------
// De-Esser (dynamic high shelf around sibilance band)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DeEsser {
    pub threshold_db: f32,
    pub freq_hz: f32,
    pub range_db: f32,
    band: StereoBiquad,
    hp: StereoBiquad,
    env: Envelope,
    gr_smooth: f32,
    pub gain_reduction: f32,
}

impl DeEsser {
    pub fn new(sr: f32) -> Self {
        Self {
            threshold_db: -26.0,
            freq_hz: 6500.0,
            range_db: -9.0,
            band: StereoBiquad::design(FilterKind::BandPass, sr, 6500.0, 0.0, 1.2),
            hp: StereoBiquad::design(FilterKind::HighShelf, sr, 6500.0, 0.0, 0.7),
            env: Envelope::new(sr, 1.0, 80.0),
            gr_smooth: 0.0,
            gain_reduction: 0.0,
        }
    }
    pub fn set_sr(&mut self, sr: f32) {
        self.band = StereoBiquad::design(FilterKind::BandPass, sr, self.freq_hz as f64, 0.0, 1.2);
        self.hp = StereoBiquad::design(FilterKind::HighShelf, sr, self.freq_hz as f64, 0.0, 0.7);
        self.env.set_times(sr, 1.0, 80.0);
    }
    #[inline]
    pub fn process(&mut self, l: &mut f32, r: &mut f32) {
        let (mut bl, mut br) = (*l, *r);
        self.band.process(&mut bl, &mut br);
        let det = bl.abs().max(br.abs());
        let env = self.env.push(det);
        let over = lin_to_db(env) - self.threshold_db;
        let gr = if over > 0.0 {
            (over * 0.8).min(self.range_db.abs())
        } else {
            0.0
        };
        self.gr_smooth = self.gr_smooth * 0.9 + gr * 0.1;
        // dynamically blend between dry and high-shelved (reduced) signal
        let (mut sl, mut sr_) = (*l, *r);
        self.hp.process(&mut sl, &mut sr_);
        let k = (-self.gr_smooth / self.range_db.abs().max(0.1)).clamp(0.0, 1.0);
        *l = *l * (1.0 - k) + sl * k;
        *r = *r * (1.0 - k) + sr_ * k;
        self.gain_reduction = -self.gr_smooth;
    }
}
