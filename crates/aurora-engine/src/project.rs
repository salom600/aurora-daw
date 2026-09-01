//! AURORA project model — the UI-side source of truth (serializable).
//!
//! The real-time engine holds its own performance snapshot; this model is
//! owned by the UI thread and communicates via commands. Audio payloads are
//! Arc-shared so clips are cheap to duplicate across takes and tracks.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MAX_TRACKS: usize = 4096;
pub const ENGINE_SAMPLE_RATE: u32 = 48_000;

// ---------------------------------------------------------------------------
// Audio payloads
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AudioData {
    /// Interleaved stereo f32 at project sample rate.
    pub samples: Vec<f32>,
    pub channels: u32,
    pub sample_rate: u32,
}

impl AudioData {
    pub fn empty() -> Self {
        Self {
            samples: Vec::new(),
            channels: 2,
            sample_rate: ENGINE_SAMPLE_RATE,
        }
    }
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    pub fn duration_secs(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.max(1) as f64
    }
    pub fn mono(&self) -> Vec<f32> {
        if self.channels <= 1 {
            return self.samples.clone();
        }
        let ch = self.channels as usize;
        self.samples
            .chunks_exact(ch)
            .map(|c| c.iter().sum::<f32>() / ch as f32)
            .collect()
    }
}

pub type SharedAudio = Arc<AudioData>;

/// Precomputed waveform peaks for fast drawing: interleaved (min, max) pairs.
pub fn compute_peaks(audio: &AudioData, buckets: usize) -> Arc<Vec<(f32, f32)>> {
    let mono_len = audio.frames();
    let mut peaks = vec![(0.0f32, 0.0f32); buckets.max(1)];
    if mono_len == 0 {
        return Arc::new(peaks);
    }
    let ch = audio.channels.max(1) as usize;
    let per = (mono_len + buckets - 1) / buckets.max(1);
    for (bi, bucket) in peaks.iter_mut().enumerate() {
        let start = bi * per;
        let end = ((bi + 1) * per).min(mono_len);
        let mut mn = 0.0f32;
        let mut mx = 0.0f32;
        if start < end {
            let s = start * ch;
            let e = end * ch;
            for frame in audio.samples[s..e].chunks_exact(ch) {
                let v = frame.iter().sum::<f32>() / ch as f32;
                mn = mn.min(v);
                mx = mx.max(v);
            }
        }
        *bucket = (mn, mx);
    }
    Arc::new(peaks)
}

// ---------------------------------------------------------------------------
// Clips & notes
// ---------------------------------------------------------------------------

pub type ClipId = u64;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct Note {
    pub start_beats: f32,
    pub len_beats: f32,
    pub key: u8, // 0..=87 piano (C2 = 0)
    pub vel: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub name: String,
    /// Timeline position in seconds.
    pub start: f64,
    /// Playback length in seconds.
    pub length: f64,
    /// Offset into the source audio.
    pub offset: f64,
    /// Decoded/recorded samples. Not serialized directly — pooled into
    /// binary asset sidecars on save (see Project::save_to_path).
    #[serde(skip)]
    pub audio: Option<SharedAudio>,
    /// Asset reference after save/load: "asset:<k>" or an original file path.
    #[serde(default)]
    pub source: Option<String>,
    pub notes: Option<Vec<Note>>,
    /// 0 = main clip, 1.. = take lane number.
    pub take_id: u32,
    pub gain_db: f32,
    /// (fade_in, fade_out) seconds.
    pub fades: (f32, f32),
    /// Drawing cache; recomputed on load.
    #[serde(skip)]
    pub peaks: Option<Arc<Vec<(f32, f32)>>>,
    #[serde(default)]
    pub muted: bool,
}

impl Clip {
    pub fn end(&self) -> f64 {
        self.start + self.length
    }
    pub fn with_audio(id: ClipId, name: &str, start: f64, audio: SharedAudio) -> Self {
        let peaks = compute_peaks(&audio, 1400);
        Self {
            id,
            name: name.to_string(),
            start,
            length: audio.duration_secs(),
            offset: 0.0,
            audio: Some(audio),
            source: None,
            notes: None,
            take_id: 0,
            gain_db: 0.0,
            fades: (0.005, 0.005),
            peaks: Some(peaks),
            muted: false,
        }
    }
    pub fn with_notes(id: ClipId, name: &str, start: f64, length: f64, notes: Vec<Note>) -> Self {
        Self {
            id,
            name: name.to_string(),
            start,
            length,
            offset: 0.0,
            audio: None,
            source: None,
            notes: Some(notes),
            take_id: 0,
            gain_db: 0.0,
            fades: (0.004, 0.004),
            peaks: None,
            muted: false,
        }
    }
    pub fn is_audio(&self) -> bool {
        self.audio.is_some()
    }
}

// ---------------------------------------------------------------------------
// Automation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct AutoPoint {
    pub t: f64,
    pub value: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Automation {
    pub enabled: bool,
    pub points: Vec<AutoPoint>,
}

impl Automation {
    pub fn eval(&self, t: f64, default: f32) -> f32 {
        if !self.enabled || self.points.is_empty() {
            return default;
        }
        let pts = &self.points;
        if t <= pts[0].t {
            return pts[0].value;
        }
        if t >= pts[pts.len() - 1].t {
            return pts[pts.len() - 1].value;
        }
        // linear search is fine for modest point counts; binary search for larger
        let idx = match pts.binary_search_by(|p| p.t.partial_cmp(&t).unwrap()) {
            Ok(i) => return pts[i].value,
            Err(i) => i,
        };
        let a = &pts[idx - 1];
        let b = &pts[idx];
        let k = ((t - a.t) / (b.t - a.t).max(1e-9)) as f32;
        a.value + (b.value - a.value) * k
    }
}

// ---------------------------------------------------------------------------
// Tracks
// ---------------------------------------------------------------------------

pub type TrackId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackKind {
    Audio,
    Instrument,
    Bus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TakeLane {
    pub name: String,
    pub take_id: u32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub subtitle: String,
    pub kind: TrackKind,
    pub volume_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub armed: bool,
    pub monitoring: bool,
    /// RGBWA
    pub color: [u8; 4],
    pub height: f32,
    pub clips: Vec<Clip>,
    pub fx: Vec<crate::effects::EffectInstance>,
    pub reverb_send: f32,
    pub delay_send: f32,
    pub active_take: u32,
    pub takes: Vec<TakeLane>,
    pub volume_automation: Automation,
    pub pan_automation: Automation,
    /// Route into a Bus track instead of master.
    pub output_bus: Option<TrackId>,
    /// Synth patch for Instrument tracks.
    #[serde(default)]
    pub synth: SynthPatch,
}

impl Track {
    pub fn new(id: TrackId, name: &str, kind: TrackKind, color: [u8; 4]) -> Self {
        Self {
            id,
            name: name.to_string(),
            subtitle: String::new(),
            kind,
            volume_db: 0.0,
            pan: 0.0,
            mute: false,
            solo: false,
            armed: false,
            monitoring: false,
            color,
            height: 44.0,
            clips: Vec::new(),
            fx: Vec::new(),
            reverb_send: 0.0,
            delay_send: 0.0,
            active_take: 0,
            takes: Vec::new(),
            volume_automation: Automation::default(),
            pan_automation: Automation::default(),
            output_bus: None,
            synth: SynthPatch::default(),
        }
    }
    pub fn color32(&self) -> egui_compat::Color {
        let c = self.color;
        [c[0], c[1], c[2], c[3].max(255)]
    }
    pub fn clips_in(&self, t0: f64, t1: f64) -> impl Iterator<Item = &Clip> {
        self.clips
            .iter()
            .filter(move |c| c.take_id == self.active_take && c.start < t1 && c.end() > t0)
    }
    pub fn duration(&self) -> f64 {
        self.clips
            .iter()
            .filter(|c| c.take_id == self.active_take)
            .map(|c| c.end())
            .fold(0.0, f64::max)
    }
}

// minimal color bridge so the engine crate does not depend on egui
pub mod egui_compat {
    pub type Color = [u8; 4];
}

// ---------------------------------------------------------------------------
// Synth patch (instrument tracks)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SynthPatch {
    /// 0 saw, 1 square, 2 sine, 3 triangle
    pub waveform: u8,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub cutoff: f32,
    pub resonance: f32,
    pub detune: f32,
    pub gain: f32,
}

impl Default for SynthPatch {
    fn default() -> Self {
        Self {
            waveform: 0,
            attack: 0.005,
            decay: 0.25,
            sustain: 0.7,
            release: 0.28,
            cutoff: 9000.0,
            resonance: 0.7,
            detune: 0.15,
            gain: 0.55,
        }
    }
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub tempo: f64,
    pub time_sig: (u32, u32),
    pub key: String,
    pub sample_rate: u32,
    pub tracks: Vec<Track>,
    pub master_volume_db: f32,
    pub master_fx: Vec<crate::effects::EffectInstance>,
    pub loop_enabled: bool,
    pub loop_range: (f64, f64),
    pub next_id: u64,
    #[serde(default)]
    pub settings: ProjectSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ProjectSettings {
    pub last_export_dir: Option<String>,
    pub ai_clean_history: Vec<AiCleanSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiCleanSummary {
    pub track_name: String,
    pub noise_reduction_db: f32,
    pub breaths_removed: u32,
    pub clicks_fixed: u32,
    pub hum_detected: Vec<f32>,
    pub timestamp_unix: u64,
}

impl Project {
    pub fn new_empty(name: &str) -> Self {
        Self {
            name: name.to_string(),
            tempo: 124.0,
            time_sig: (4, 4),
            key: "Cmaj".into(),
            sample_rate: ENGINE_SAMPLE_RATE,
            tracks: Vec::new(),
            master_volume_db: -0.5,
            master_fx: Vec::new(),
            loop_enabled: false,
            loop_range: (0.0, 8.0),
            next_id: 1,
            settings: ProjectSettings::default(),
        }
    }

    pub fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn add_track(&mut self, name: &str, kind: TrackKind, color: [u8; 4]) -> &mut Track {
        let id = self.alloc_id();
        self.tracks.push(Track::new(id, name, kind, color));
        self.tracks.last_mut().unwrap()
    }

    pub fn track_by_id(&self, id: TrackId) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }
    pub fn track_by_id_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn duration(&self) -> f64 {
        self.tracks.iter().map(|t| t.duration()).fold(0.0, f64::max)
    }

    pub fn beats_to_secs(&self, beats: f64) -> f64 {
        beats * 60.0 / self.tempo
    }
    pub fn secs_to_beats(&self, s: f64) -> f64 {
        s * self.tempo / 60.0
    }

    /// Any vocal-ish tracks for AI tools (name heuristics + instrument check).
    pub fn vocal_tracks(&self) -> Vec<TrackId> {
        self.tracks
            .iter()
            .filter(|t| {
                t.kind == TrackKind::Audio
                    && (t.name.to_lowercase().contains("vocal")
                        || t.name.to_lowercase().contains("voice"))
            })
            .map(|t| t.id)
            .collect()
    }

    pub fn save_to_path(&self, path: &std::path::Path) -> Result<(), String> {
        // pool unique audio payloads into binary sidecars
        let assets_dir = {
            let stem = path.with_extension("");
            let mut s = stem.as_os_str().to_os_string();
            s.push("_assets");
            std::path::PathBuf::from(s)
        };
        std::fs::create_dir_all(&assets_dir).map_err(|e| e.to_string())?;

        use std::collections::HashMap;
        let mut key_of: HashMap<usize, u64> = HashMap::new();
        let mut next_key = 0u64;
        for t in &self.tracks {
            for c in &t.clips {
                if let Some(a) = &c.audio {
                    let k = Arc::as_ptr(a) as usize;
                    key_of.entry(k).or_insert_with(|| {
                        let key = next_key;
                        next_key += 1;
                        // write sidecar: sample_rate u32, frames u32, f32 data
                        let mut bytes = Vec::with_capacity(8 + a.samples.len() * 4);
                        bytes.extend_from_slice(&a.sample_rate.to_le_bytes());
                        bytes.extend_from_slice(&(a.frames() as u32).to_le_bytes());
                        for v in &a.samples {
                            bytes.extend_from_slice(&v.to_le_bytes());
                        }
                        let p = assets_dir.join(format!("a{key}.pcm"));
                        if let Err(e) = std::fs::write(&p, bytes) {
                            log::warn!("asset write failed: {e}");
                        }
                        key
                    });
                }
            }
        }

        // serialize a shallow copy with audio detached
        let mut clone = self.clone();
        for t in &mut clone.tracks {
            for c in &mut t.clips {
                if c.audio.is_some() {
                    let k = Arc::as_ptr(c.audio.as_ref().unwrap()) as usize;
                    if let Some(key) = key_of.get(&k) {
                        c.source = Some(format!("asset:{key}"));
                    }
                    c.audio = None;
                }
            }
        }
        let s = ron::to_string(&clone).map_err(|e| e.to_string())?;
        std::fs::write(path, s).map_err(|e| e.to_string())
    }

    pub fn load_from_path(path: &std::path::Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut p: Project = ron::from_str(&s).map_err(|e| e.to_string())?;
        let assets_dir = {
            let stem = path.with_extension("");
            let mut s = stem.as_os_str().to_os_string();
            s.push("_assets");
            std::path::PathBuf::from(s)
        };
        for t in &mut p.tracks {
            for c in &mut t.clips {
                let src = c.source.clone();
                if let Some(src) = src {
                    if let Some(key) = src.strip_prefix("asset:") {
                        let ap = assets_dir.join(format!("a{key}.pcm"));
                        if let Ok(bytes) = std::fs::read(&ap) {
                            if bytes.len() > 8 {
                                let sr = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                                let frames = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
                                let mut samples = Vec::with_capacity(frames * 2);
                                for i in 0..(frames * 2).min((bytes.len() - 8) / 4) {
                                    let o = 8 + i * 4;
                                    samples.push(f32::from_le_bytes([
                                        bytes[o],
                                        bytes[o + 1],
                                        bytes[o + 2],
                                        bytes[o + 3],
                                    ]));
                                }
                                let audio = Arc::new(AudioData {
                                    samples,
                                    channels: 2,
                                    sample_rate: sr,
                                });
                                c.peaks = Some(compute_peaks(&audio, 1400));
                                c.audio = Some(audio);
                            }
                        }
                    } else if std::path::Path::new(&src).exists() {
                        // re-import from original file path
                        if let Ok(mut a) = crate::io::decode_file(std::path::Path::new(&src), p.sample_rate) {
                            let _ = &mut a;
                            c.audio = Some(Arc::new(a.clone()));
                            c.peaks = Some(compute_peaks(&a, 1400));
                        }
                    }
                }
            }
        }
        Ok(p)
    }
}
