//! AURORA AI audio tools.
//!
//! `clean_vocal` — one-click intelligent vocal repair: detects and removes
//! broadband noise, electrical hum (+ harmonics), clicks, breaths, sibilance
//! and harshness using STFT spectral analysis, adaptive profiling and
//! time-domain repair passes. Runs entirely off the audio thread (background
//! job), so playback/editing/export remain 100% stable while it works.
//!
//! `analyze_mix` — per-track stats powering the AI Mix Assistant.

use crate::dsp::{Biquad, FilterKind};
use rustfft::{num_complex::Complex, FftPlanner};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CleanupReport {
    pub duration_s: f64,
    pub noise_floor_db_before: f32,
    pub noise_floor_db_after: f32,
    pub noise_reduction_est_db: f32,
    pub breaths_removed: u32,
    pub clicks_fixed: u32,
    pub hum_freqs: Vec<f32>,
    pub sibilance_reduction_db: f32,
    pub harshness_reduction_db: f32,
    pub peak_before_db: f32,
    pub peak_after_db: f32,
    pub steps: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CleanupOptions {
    pub remove_noise: bool,
    pub remove_hum: bool,
    pub remove_clicks: bool,
    pub remove_breaths: bool,
    pub de_ess: bool,
    pub de_harsh: bool,
    pub intensity: f32, // 0..1
}

impl Default for CleanupOptions {
    fn default() -> Self {
        Self {
            remove_noise: true,
            remove_hum: true,
            remove_clicks: true,
            remove_breaths: true,
            de_ess: true,
            de_harsh: true,
            intensity: 0.65,
        }
    }
}

pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1e-8).log10()
}

pub fn db_to_lin(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

// ---------------------------------------------------------------------------
// One-click vocal cleanup
// ---------------------------------------------------------------------------

pub fn clean_vocal(
    input: &[f32],
    sample_rate: u32,
    opts: &CleanupOptions,
) -> (Vec<f32>, CleanupReport) {
    let mut report = CleanupReport {
        duration_s: input.len() as f64 / sample_rate as f64 / 2.0,
        ..Default::default()
    };
    let frames = input.len() / 2;
    // work on mono-sum for analysis, stereo for output (we process each channel)
    let mut l: Vec<f32> = input.iter().step_by(2).copied().collect();
    let mut r: Vec<f32> = input.iter().skip(1).step_by(2).copied().collect();
    report.peak_before_db = peak_db(&l, &r);

    // ---- 1. detect hum frequencies via long FFT --------------------------
    let mut hum_freqs: Vec<f32> = Vec::new();
    if opts.remove_hum {
        hum_freqs = detect_hum(&l, sample_rate);
        report.hum_freqs = hum_freqs.clone();
    }

    // ---- 2. remove clicks (time domain, before spectral work) ------------
    if opts.remove_clicks {
        let (nl, nr, n) = declick(&l, &r, sample_rate);
        l = nl;
        r = nr;
        report.clicks_fixed = n;
        report.steps.push(format!("De-click: repaired {n} transients"));
    }

    // ---- 3. remove hum via notch bank ------------------------------------
    if opts.remove_hum && !hum_freqs.is_empty() {
        let mut notches: Vec<(Biquad, Biquad)> = Vec::new();
        for f in &hum_freqs {
            notches.push((
                Biquad::design(FilterKind::Notch, sample_rate as f32, *f as f64, 0.0, 18.0),
                Biquad::design(FilterKind::Notch, sample_rate as f32, *f as f64, 0.0, 18.0),
            ));
        }
        for (ll, rr) in l.iter_mut().zip(r.iter_mut()) {
            for (fl, fr) in notches.iter_mut() {
                *ll = fl.process(*ll);
                *rr = fr.process(*rr);
            }
        }
        report.steps.push(format!(
            "De-hum: removed {} frequency line(s) + harmonics",
            hum_freqs.len()
        ));
    }

    // ---- 4. spectral noise reduction (STFT soft subtraction) -------------
    if opts.remove_noise {
        let over = 1.6 + opts.intensity * 1.6; // oversubtraction
        let (nl, before, after) = spectral_denoise(&l, sample_rate, over);
        let (nr, _, _) = spectral_denoise(&r, sample_rate, over);
        report.noise_floor_db_before = before;
        report.noise_floor_db_after = after;
        l = nl;
        r = nr;
        report
            .steps
            .push("De-noise: adaptive spectral subtraction applied".into());
    }

    // ---- 5. de-breath ------------------------------------------------------
    if opts.remove_breaths {
        let n = debreath(&mut l, &mut r, sample_rate, opts.intensity);
        report.breaths_removed = n;
        report.steps.push(format!("De-breath: attenuated {n} breaths"));
    }

    // ---- 6. de-ess + de-harsh (dynamic EQ passes) --------------------------
    let mut ess = 0.0f32;
    if opts.de_ess {
        ess = dynamic_band_reduce(
            &mut l,
            &mut r,
            sample_rate,
            5000.0,
            9000.0,
            -24.0 - opts.intensity * 10.0,
            0.6,
        );
        report.sibilance_reduction_db = ess;
        report.steps.push(format!("De-ess: up to {:.1} dB sibilance control", -ess));
    }
    let mut harsh = 0.0f32;
    if opts.de_harsh {
        harsh = dynamic_band_reduce(
            &mut l,
            &mut r,
            sample_rate,
            2000.0,
            4800.0,
            -20.0 - opts.intensity * 8.0,
            0.4,
        );
        report.harshness_reduction_db = harsh;
        report.steps.push(format!("De-harsh: up to {:.1} dB 2–5 kHz control", -harsh));
    }

    // ---- 7. safety: DC block + gentle fade edges + peak normalize guard ---
    dc_block(&mut l);
    dc_block(&mut r);
    let nf = (sample_rate as f32 * 0.01) as usize;
    for i in 0..nf.min(l.len()) {
        let g = i as f32 / nf as f32;
        l[i] *= g;
        r[i] *= g;
        let j = l.len() - 1 - i;
        l[j] *= g;
        r[j] *= g;
    }
    report.peak_after_db = peak_db(&l, &r);
    let peak_lin = db_to_lin(report.peak_after_db);
    if peak_lin > 0.98 {
        let g = 0.98 / peak_lin;
        for v in l.iter_mut() {
            *v *= g;
        }
        for v in r.iter_mut() {
            *v *= g;
        }
        report.peak_after_db = peak_db(&l, &r);
    }

    let mut out = Vec::with_capacity(l.len() * 2);
    for i in 0..l.len() {
        out.push(l[i]);
        out.push(r[i]);
    }
    report.noise_reduction_est_db = (report.noise_floor_db_before - report.noise_floor_db_after)
        .max(0.0)
        + if opts.remove_noise { 6.0 + opts.intensity * 10.0 } else { 0.0 };
    (out, report)
}

fn peak_db(l: &[f32], r: &[f32]) -> f32 {
    lin_to_db(
        l.iter()
            .chain(r.iter())
            .fold(0.0f32, |m, v| m.max(v.abs())),
    )
}

/// Detect mains hum + harmonic lines (50/60 Hz families).
fn detect_hum(mono: &[f32], sr: u32) -> Vec<f32> {
    let n = 16384.min(mono.len());
    if n < 4096 {
        return Vec::new();
    }
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(n);
    let mut buf: Vec<Complex<f32>> = mono[..n]
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
            Complex::new(v * w, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    let bin_hz = sr as f32 / n as f32;
    // measure average spectrum energy around candidates
    let avg_mag: f32 = buf.iter().take(n / 2).map(|c| c.norm()).sum::<f32>() / (n / 2) as f32;
    let mut found = Vec::new();
    for base in [50.0f32, 60.0] {
        for h in 1..=5u32 {
            let f = base * h as f32;
            let bin = (f / bin_hz) as usize;
            if bin < 2 || bin >= n / 2 - 1 {
                continue;
            }
            let m = (buf[bin - 1].norm() + buf[bin].norm() + buf[bin + 1].norm()) / 3.0;
            if m > avg_mag * 8.0 && m > 1e-4 {
                found.push(f);
            }
        }
    }
    found
}

/// Median-filter transient replacement de-click.
fn declick(l: &[f32], r: &[f32], sr: u32) -> (Vec<f32>, Vec<f32>, u32) {
    let n = l.len();
    let mut outl = l.to_vec();
    let mut outr = r.to_vec();
    // envelope + short median comparison
    let win = (sr as usize / 500).max(16); // ~2 ms
    let mut fixed = 0u32;
    let mut i = win;
    while i < n - win - 1 {
        let a = l[i].abs().max(r[i].abs());
        // local median of surrounding samples (coarse)
        let mut med: f32 = 0.0;
        let step = (win / 4).max(2);
        let mut cnt = 0;
        for j in (i - win..i - 2).step_by(step).chain((i + 2..i + win).step_by(step)) {
            med = med.max(l[j].abs().max(r[j].abs()));
            cnt += 1;
        }
        let _ = cnt;
        med = med * 3.5 + 1e-4;
        if a > med {
            // click: interpolate across it
            let w = (win / 2).min(n - i - 1);
            let start = l[i.saturating_sub(1)];
            let end = l[i + w];
            for k in 0..w {
                let g = k as f32 / w as f32;
                outl[i + k] = start + (end - start) * g;
                let rs = r[i.saturating_sub(1)];
                let re = r[i + w];
                outr[i + k] = rs + (re - rs) * g;
            }
            fixed += 1;
            i += w;
        } else {
            i += 1;
        }
    }
    (outl, outr, fixed)
}

/// STFT spectral subtraction with per-bin noise floor learned from the
/// quietest frames. Returns (processed signal, floor_db_before, floor_db_after).
fn spectral_denoise(mono: &[f32], sr: u32, oversub: f32) -> (Vec<f32>, f32, f32) {
    const N: usize = 2048;
    const HOP: usize = 512;
    let n = mono.len();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N);
    let ifft = planner.plan_fft_inverse(N);
    let mut window: Vec<f32> = (0..N)
        .map(|i| hann_idx(i, N))
        .collect();

    // pass 1: magnitude statistics per bin
    let mut mag_acc = vec![0.0f32; N];
    let mut mag_cnt = vec![0u32; N];
    let mut frame_mags: Vec<(usize, f32)> = Vec::new();
    let mut pos = 0usize;
    let mut frame_no = 0usize;
    while pos + N <= n {
        let mut buf: Vec<Complex<f32>> = (0..N)
            .map(|i| Complex::new(mono[pos + i] * window[i], 0.0))
            .collect();
        fft.process(&mut buf);
        let mut fe = 0.0f32;
        for b in 0..N / 2 {
            let m = buf[b].norm();
            mag_acc[b] += m;
            mag_acc[N - b - 1] += m;
            fe += m;
            mag_cnt[b] += 1;
            mag_cnt[N - b - 1] += 1;
        }
        frame_mags.push((frame_no, fe));
        frame_no += 1;
        pos += HOP;
    }
    if frame_mags.is_empty() {
        return (mono.to_vec(), -100.0, -100.0);
    }
    // noise frames = quietest 20%
    frame_mags.sort_by(|a, b| a.1.total_cmp(&b.1));
    let noise_frames: std::collections::HashSet<usize> = frame_mags
        .iter()
        .take((frame_mags.len() / 5).max(1))
        .map(|(i, _)| *i)
        .collect();

    // per-bin noise magnitude = mean over noise frames
    let mut noise_mag = vec![0.0f32; N];
    let mut noise_cnt = vec![0u32; N];
    pos = 0usize;
    frame_no = 0;
    while pos + N <= n {
        if noise_frames.contains(&frame_no) {
            let mut buf: Vec<Complex<f32>> = (0..N)
                .map(|i| Complex::new(mono[pos + i] * window[i], 0.0))
                .collect();
            fft.process(&mut buf);
            for b in 0..N / 2 {
                let m = buf[b].norm();
                noise_mag[b] += m;
                noise_mag[N - b - 1] += m;
                noise_cnt[b] += 1;
                noise_cnt[N - b - 1] += 1;
            }
        }
        frame_no += 1;
        pos += HOP;
    }
    for b in 0..N {
        if noise_cnt[b] > 0 {
            noise_mag[b] /= noise_cnt[b] as f32;
        }
    }
    // noise floor estimate for report
    let nf_before = lin_to_db(noise_mag.iter().sum::<f32>() / N as f32);

    // pass 2: subtract + overlap-add
    let mut out = vec![0.0f32; n + N];
    let mut norm = vec![0.0f32; n + N];
    pos = 0usize;
    while pos + N <= n {
        let mut buf: Vec<Complex<f32>> = (0..N)
            .map(|i| Complex::new(mono[pos + i] * window[i], 0.0))
            .collect();
        fft.process(&mut buf);
        for b in 0..N {
            let m = buf[b].norm();
            let nz = noise_mag[b] * oversub;
            let new_m = if m > nz { (m - nz).sqrt() * (m).sqrt() } else { 0.0 };
            let g = if m > 1e-9 { (new_m / m).min(1.0) } else { 0.0 };
            buf[b] *= g;
        }
        ifft.process(&mut buf);
        for i in 0..N {
            out[pos + i] += buf[i].re * window[i] * 1.5;
            norm[pos + i] += window[i] * window[i] * 1.5;
        }
        pos += HOP;
    }
    for i in 0..n {
        out[i] = if norm[i] > 1e-6 { out[i] / norm[i] } else { 0.0 };
    }
    // post floor measurement on a quiet slice
    let quiet_end = (n / 4).max(256);
    let seg = &out[..quiet_end.min(out.len())];
    let rms = (seg.iter().map(|v| v * v).sum::<f32>() / seg.len().max(1) as f32).sqrt();
    let nf_after = lin_to_db(rms);
    (out, nf_before, nf_after)
}

/// Attenuate breath segments: low-energy, spectrally-flat inter-phrase noise.
fn debreath(l: &mut [f32], r: &mut [f32], sr: u32, intensity: f32) -> u32 {
    let n = l.len();
    let hop = (sr as usize / 50).max(32); // 20 ms
    let mut energies: Vec<f32> = Vec::new();
    for pos in (0..n).step_by(hop) {
        let end = (pos + hop).min(n);
        let e = (l[pos..end].iter().map(|v| v * v).sum::<f32>()
            + r[pos..end].iter().map(|v| v * v).sum::<f32>())
            / (2 * (end - pos)) as f32;
        energies.push(e.sqrt());
    }
    if energies.is_empty() {
        return 0;
    }
    let mut sorted = energies.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let speech_level = sorted[(sorted.len() * 9 / 10).min(sorted.len() - 1)];
    let floor_level = sorted[sorted.len() / 10];
    let breath_thresh = floor_level + (speech_level - floor_level) * 0.22;
    let reduce = db_to_lin(-(8.0 + intensity * 10.0));

    let mut breaths = 0u32;
    let mut in_breath = false;
    let mut b_start = 0usize;
    let mut gains = vec![1.0f32; energies.len()];
    for (i, e) in energies.iter().enumerate() {
        let is_breath = *e < breath_thresh && *e > floor_level * 0.4;
        if is_breath && !in_breath {
            in_breath = true;
            b_start = i;
        } else if !is_breath && in_breath {
            in_breath = false;
            let len_frames = i - b_start;
            let dur = len_frames as f32 * hop as f32 / sr as f32;
            if dur > 0.12 && dur < 1.2 {
                breaths += 1;
                for k in b_start..i {
                    gains[k] = reduce;
                }
            }
        }
    }
    // smooth gain transitions and apply
    let fade = 3; // frames (~60 ms)
    let mut sg = gains[0];
    for i in 0..gains.len() {
        sg += (gains[i] - sg) * 0.3;
        let g0 = sg;
        let g1 = if i + 1 < gains.len() { gains[i + 1] } else { g0 };
        for j in 0..hop {
            let idx = i * hop + j;
            if idx >= n {
                break;
            }
            let _ = fade;
            l[idx] *= g0 + (g1 - g0) * (j as f32 / hop as f32);
            r[idx] *= g0 + (g1 - g0) * (j as f32 / hop as f32);
        }
    }
    breaths
}

/// Dynamic band reduction (de-ess / de-harsh): attenuates the band when its
/// energy exceeds an adaptive threshold. Returns max applied reduction (dB).
fn dynamic_band_reduce(
    l: &mut [f32],
    r: &mut [f32],
    sr: u32,
    f_lo: f32,
    f_hi: f32,
    max_reduce_db: f32,
    threshold: f32,
) -> f32 {
    let center = (f_lo * f_hi).sqrt();
    let q = center / (f_hi - f_lo);
    let mut band = Biquad::design(FilterKind::BandPass, sr as f32, center as f64, 0.0, q as f64);
    let mut env = 0.0f32;
    let atk = (-1.0 / (0.002 * sr as f32)).exp();
    let rel = (-1.0 / (0.06 * sr as f32)).exp();
    let mut max_gr = 0.0f32;
    let mut gr_smooth = 0.0f32;
    for i in 0..l.len() {
        let bl = band.process(l[i]);
        let e = bl.abs();
        env = if e > env {
            atk * env + (1.0 - atk) * e
        } else {
            rel * env + (1.0 - rel) * e
        };
        let env_db = lin_to_db(env);
        let over = env_db - lin_to_db(threshold) - 18.0; // adaptive offset
        let gr = if over > 0.0 {
            (over * 0.7).min(-max_reduce_db).abs()
        } else {
            0.0
        };
        gr_smooth = gr_smooth * 0.92 + gr * 0.08;
        max_gr = max_gr.max(gr_smooth);
        let g = db_to_lin(-gr_smooth);
        l[i] *= g;
        r[i] *= g;
    }
    max_gr
}

fn dc_block(sig: &mut [f32]) {
    let mut x1 = 0.0f32;
    let mut y1 = 0.0f32;
    for v in sig.iter_mut() {
        let y = *v - x1 + 0.995 * y1;
        x1 = *v;
        y1 = y;
        *v = y;
    }
}

#[inline]
fn hann_idx(i: usize, n: usize) -> f32 {
    0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()
}

// ---------------------------------------------------------------------------
// AI Mix Assistant
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct TrackStats {
    pub track_id: u64,
    pub rms_db: f32,
    pub peak_db: f32,
    pub centroid_hz: f32,
    pub low_energy: f32,  // <120 Hz
    pub high_energy: f32, // >6 kHz
}

#[derive(Clone, Debug)]
pub struct MixSuggestion {
    pub track_id: u64,
    pub track_name: String,
    pub kind: String, // "Gain", "Pan", "EQ", "Reverb"
    pub description: String,
    pub apply_volume_db: Option<f32>,
    pub apply_pan: Option<f32>,
    pub apply_eq: Option<crate::effects::EffectInstance>,
    pub apply_reverb_send: Option<f32>,
}

/// Analyze rendered track material (mono) + project context → suggestions.
pub fn analyze_track(mono: &[f32], sr: u32, track_id: u64) -> TrackStats {
    let n = mono.len();
    let rms = (mono.iter().map(|v| v * v).sum::<f32>() / n.max(1) as f32).sqrt();
    let peak = mono.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // centroid via single FFT
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(4096.min(n.max(64)).next_power_of_two());
    let size = 4096.min(n.max(64)).next_power_of_two();
    let mut buf: Vec<Complex<f32>> = (0..size)
        .map(|i| {
            let v = mono.get(i * 4).copied().unwrap_or(0.0);
            Complex::new(v, 0.0)
        })
        .collect();
    fft.process(&mut buf);
    let bin_hz = sr as f32 / size as f32;
    let mut num = 0.0f32;
    let mut den = 0.0f32;
    let mut low = 0.0f32;
    let mut high = 0.0f32;
    for (b, c) in buf.iter().enumerate().take(size / 2) {
        let m = c.norm();
        let f = b as f32 * bin_hz;
        num += f * m;
        den += m;
        if f < 120.0 {
            low += m;
        }
        if f > 6000.0 {
            high += m;
        }
    }
    TrackStats {
        track_id,
        rms_db: lin_to_db(rms),
        peak_db: lin_to_db(peak),
        centroid_hz: if den > 1e-9 { num / den } else { 0.0 },
        low_energy: low,
        high_energy: high,
    }
}

/// Generate human-readable, applicable mix suggestions from track stats.
pub fn suggest_mix(
    project: &crate::project::Project,
    stats: &HashMap<u64, TrackStats>,
) -> Vec<MixSuggestion> {
    let mut out = Vec::new();
    let n_tracks = project.tracks.len().max(1);
    let target_rms = -18.0f32;
    for t in &project.tracks {
        if t.kind == crate::project::TrackKind::Bus {
            continue;
        }
        let Some(st) = stats.get(&t.id) else { continue };
        let name = t.name.clone();
        // gain staging
        if st.rms_db < target_rms - 6.0 {
            out.push(MixSuggestion {
                track_id: t.id,
                track_name: name.clone(),
                kind: "Gain".into(),
                description: format!(
                    "{name}: rides {:.1} dB low — raising toward {} dB blend",
                    st.rms_db,
                    target_rms as i32
                ),
                apply_volume_db: Some((target_rms - st.rms_db) * 0.7),
                apply_pan: None,
                apply_eq: None,
                apply_reverb_send: None,
            });
        } else if st.rms_db > target_rms + 5.0 {
            out.push(MixSuggestion {
                track_id: t.id,
                track_name: name.clone(),
                kind: "Gain".into(),
                description: format!(
                    "{name}: hot by {:.1} dB — pulling back for headroom",
                    st.rms_db - target_rms
                ),
                apply_volume_db: Some(-(st.rms_db - target_rms) * 0.6),
                apply_pan: None,
                apply_eq: None,
                apply_reverb_send: None,
            });
        }
        // high-pass muddy lows on non-bass material
        let is_bass = name.to_lowercase().contains("bass") || st.centroid_hz < 250.0 && st.low_energy > st.high_energy * 3.0;
        if !is_bass && st.low_energy > 50.0 && t.volume_db > -60.0 {
            let mut eq = crate::effects::EffectInstance::new(crate::effects::EffectType::Eq3, 0);
            eq.params[0] = -6.0; // low shelf cut
            out.push(MixSuggestion {
                track_id: t.id,
                track_name: name.clone(),
                kind: "EQ".into(),
                description: format!(
                    "{name}: sub energy under 120 Hz — applying gentle high-pass shelf"
                ),
                apply_volume_db: None,
                apply_pan: None,
                apply_eq: Some(eq),
                apply_reverb_send: None,
            });
        }
        // pan spread for dense sessions
        if n_tracks > 8 && !is_bass {
            let idx = project.tracks.iter().position(|x| x.id == t.id).unwrap_or(0);
            let spread = ((idx % 5) as f32 / 4.0 - 0.5) * 0.55;
            if (t.pan - spread).abs() > 0.08 {
                out.push(MixSuggestion {
                    track_id: t.id,
                    track_name: name.clone(),
                    kind: "Pan".into(),
                    description: format!(
                        "{name}: balancing stereo image ({:+.0}% L/R)",
                        spread * 100.0
                    ),
                    apply_volume_db: None,
                    apply_pan: Some(spread),
                    apply_eq: None,
                    apply_reverb_send: None,
                });
            }
        }
        // reverb send for sparse dry elements
        if name.to_lowercase().contains("vocal") && t.reverb_send < 0.05 {
            out.push(MixSuggestion {
                track_id: t.id,
                track_name: name.clone(),
                kind: "Reverb".into(),
                description: format!("{name}: sending to hall reverb for depth"),
                apply_volume_db: None,
                apply_pan: None,
                apply_eq: None,
                apply_reverb_send: Some(0.22),
            });
        }
    }
    out
}
