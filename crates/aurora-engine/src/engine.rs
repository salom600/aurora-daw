//! AURORA real-time engine — sample-accurate mixer graph driven by an
//! audio callback (or the synthetic driver fallback).
//!
//! Audio-path rules:
//! - No locks shared with the UI on the render path (commands via lock-free
//!   ring buffer, hot params via atomics, results via bounded channel).
//! - The offline bounce re-uses exactly this graph (`EngineRT::process_block`).

use crate::dsp::*;
use crate::effects::{EffectInstance, FxUnit};
use crate::project::*;
use crate::synth::{schedule_notes, PolySynth};
use rtrb::{Consumer, Producer, RingBuffer};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const MAX_TRACK_SLOTS: usize = 4096;
pub const BLOCK: usize = 512;

// ---------------------------------------------------------------------------
// Cross-thread parameter store (UI writes, audio reads)
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ParamSlot {
    pub volume_db: AtomicU32, // f32 bits
    pub pan: AtomicU32,
    pub flags: AtomicU32, // bit0 mute, bit1 solo, bit2 armed, bit3 monitoring
}

pub struct ParamStore {
    pub slots: Vec<ParamSlot>,
    pub master_volume_db: AtomicU32,
    pub tempo: AtomicU32,
    pub playing: AtomicBool,
    pub loop_enabled: AtomicBool,
    pub loop_start: AtomicU32, // seconds, f32 bits
    pub loop_end: AtomicU32,
}

impl ParamStore {
    pub fn new() -> Arc<Self> {
        let mut slots = Vec::with_capacity(MAX_TRACK_SLOTS);
        for _ in 0..MAX_TRACK_SLOTS {
            slots.push(ParamSlot {
                volume_db: AtomicU32::new(0.0f32.to_bits()),
                pan: AtomicU32::new(0.0f32.to_bits()),
                flags: AtomicU32::new(0),
            });
        }
        Arc::new(Self {
            slots,
            master_volume_db: AtomicU32::new((-0.5f32).to_bits()),
            tempo: AtomicU32::new(124.0f32.to_bits()),
            playing: AtomicBool::new(false),
            loop_enabled: AtomicBool::new(false),
            loop_start: AtomicU32::new(0.0f32.to_bits()),
            loop_end: AtomicU32::new(8.0f32.to_bits()),
        })
    }
    pub fn set_track(&self, slot: usize, t: &Track) {
        if slot >= MAX_TRACK_SLOTS {
            return;
        }
        let s = &self.slots[slot];
        s.volume_db.store(t.volume_db.to_bits(), Ordering::Relaxed);
        s.pan.store(t.pan.to_bits(), Ordering::Relaxed);
        let mut f = 0u32;
        if t.mute {
            f |= 1;
        }
        if t.solo {
            f |= 2;
        }
        if t.armed {
            f |= 4;
        }
        if t.monitoring {
            f |= 8;
        }
        s.flags.store(f, Ordering::Relaxed);
    }
    pub fn set_loop(&self, l: Option<(f64, f64)>) {
        match l {
            Some((a, b)) => {
                self.loop_enabled.store(true, Ordering::Relaxed);
                self.loop_start.store((a as f32).to_bits(), Ordering::Relaxed);
                self.loop_end.store((b as f32).to_bits(), Ordering::Relaxed);
            }
            None => self.loop_enabled.store(false, Ordering::Relaxed),
        }
    }
}

// ---------------------------------------------------------------------------
// Meter store (audio writes, UI reads)
// ---------------------------------------------------------------------------

pub struct MeterStore {
    pub track_peak: Vec<AtomicU32>,
    pub track_rms: Vec<AtomicU32>,
    pub master_peak_l: AtomicU32,
    pub master_peak_r: AtomicU32,
    pub master_rms_l: AtomicU32,
    pub master_rms_r: AtomicU32,
    pub callback_us: AtomicU32,
    pub blocks: AtomicU64,
    pub xruns: AtomicU64,
    /// 0 unknown, 1 cpal device, 2 synthetic
    pub driver_kind: AtomicU32,
    pub input_peak: AtomicU32,
    /// Sample rate of the live capture device (0 = no device feed)
    pub input_rate: AtomicU32,
}

impl MeterStore {
    pub fn new() -> Arc<Self> {
        let mut s = Self {
            track_peak: Vec::with_capacity(MAX_TRACK_SLOTS),
            track_rms: Vec::with_capacity(MAX_TRACK_SLOTS),
            master_peak_l: AtomicU32::new(0),
            master_peak_r: AtomicU32::new(0),
            master_rms_l: AtomicU32::new(0),
            master_rms_r: AtomicU32::new(0),
            callback_us: AtomicU32::new(0),
            blocks: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            driver_kind: AtomicU32::new(0),
            input_peak: AtomicU32::new(0),
            input_rate: AtomicU32::new(0),
        };
        for _ in 0..MAX_TRACK_SLOTS {
            s.track_peak.push(AtomicU32::new(0));
            s.track_rms.push(AtomicU32::new(0));
        }
        Arc::new(s)
    }
    pub fn read_f32(a: &AtomicU32) -> f32 {
        f32::from_bits(a.load(Ordering::Relaxed))
    }
}

// ---------------------------------------------------------------------------
// Spectral tap + loudness (audio writes, UI reads)
// ---------------------------------------------------------------------------

pub struct SpectralTap {
    pub buf: Mutex<Vec<f32>>,
    pub version: AtomicU64,
}

impl SpectralTap {
    pub fn new(n: usize) -> Self {
        Self {
            buf: Mutex::new(vec![0.0; n]),
            version: AtomicU64::new(0),
        }
    }
}

pub struct LoudnessTap {
    pub momentary_lu: AtomicU32,
    pub shortterm_lu: AtomicU32,
    pub integrated_lu: AtomicU32,
    pub true_peak_db: AtomicU32,
}

/// BS.1770-4 style loudness (K-weighted, gated integrated).
pub struct LoudnessMeter {
    k1: [Biquad; 2],
    k2: [Biquad; 2],
    window: Vec<f64>,
    win_idx: usize,
    win_sum: f64,
    blocks_100ms: Vec<f64>,
    cur_block_sum: f64,
    cur_block_n: usize,
    sr: f32,
    true_peak: f32,
}

impl LoudnessMeter {
    pub fn new(sr: f32) -> Self {
        let mut k1l = Biquad::new();
        k1l.b0 = 1.53512485958697;
        k1l.b1 = -2.69169618940638;
        k1l.b2 = 1.19839281085285;
        k1l.a1 = -1.69065929318241;
        k1l.a2 = 0.73248077421585;
        let mut k2l = Biquad::new();
        k2l.b0 = 1.0;
        k2l.b1 = -2.0;
        k2l.b2 = 1.0;
        k2l.a1 = -1.99004745483398;
        k2l.a2 = 0.99007225036621;
        LoudnessMeter {
            k1: [k1l, k1l.clone()],
            k2: [k2l, k2l.clone()],
            window: vec![0.0; (0.4 * sr) as usize],
            win_idx: 0,
            win_sum: 0.0,
            blocks_100ms: Vec::new(),
            cur_block_sum: 0.0,
            cur_block_n: 0,
            sr,
            true_peak: 0.0,
        }
    }
    pub fn push(&mut self, io: &[f32], frames: usize) -> f64 {
        let n = self.window.len();
        for f in 0..frames {
            let l = io[f * 2];
            let r = io[f * 2 + 1];
            let kl = self.k2[0].process(self.k1[0].process(l));
            let kr = self.k2[1].process(self.k1[1].process(r));
            let z = (kl * kl + kr * kr) as f64 * 0.5;
            self.win_sum += z - self.window[self.win_idx];
            self.window[self.win_idx] = z;
            self.win_idx = (self.win_idx + 1) % n;
            self.cur_block_sum += z;
            self.cur_block_n += 1;
            if self.cur_block_n >= (0.1 * self.sr) as usize {
                self.blocks_100ms
                    .push(self.cur_block_sum / self.cur_block_n as f64);
                self.cur_block_sum = 0.0;
                self.cur_block_n = 0;
            }
            if f + 1 < frames {
                let l2 = (l + io[(f + 1) * 2]).abs() * 0.5;
                let r2 = (r + io[(f + 1) * 2 + 1]).abs() * 0.5;
                self.true_peak = self.true_peak.max(l.abs()).max(r.abs()).max(l2).max(r2);
            } else {
                self.true_peak = self.true_peak.max(l.abs()).max(r.abs());
            }
        }
        self.momentary()
    }
    pub fn momentary(&self) -> f64 {
        let ms = self.win_sum / self.window.len() as f64;
        -0.691 + 10.0 * ms.max(1e-12).log10()
    }
    pub fn shortterm(&self) -> f64 {
        let nb = (self.blocks_100ms.len()).min(30);
        if nb == 0 {
            return -70.0;
        }
        let start = self.blocks_100ms.len() - nb;
        let z: f64 = self.blocks_100ms[start..].iter().sum();
        -0.691 + 10.0 * (z / nb as f64).max(1e-12).log10()
    }
    pub fn integrated(&self) -> f64 {
        if self.blocks_100ms.is_empty() {
            return -70.0;
        }
        let vals: Vec<f64> = self
            .blocks_100ms
            .iter()
            .copied()
            .filter(|z| -0.691 + 10.0 * z.max(1e-12).log10() > -70.0)
            .collect();
        if vals.is_empty() {
            return -70.0;
        }
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let l_abs = -0.691 + 10.0 * mean.log10();
        let thresh = l_abs - 10.0;
        let rel: Vec<f64> = vals
            .into_iter()
            .filter(|z| -0.691 + 10.0 * z.max(1e-12).log10() > thresh)
            .collect();
        if rel.is_empty() {
            return l_abs;
        }
        let m = rel.iter().sum::<f64>() / rel.len() as f64;
        -0.691 + 10.0 * m.log10()
    }
    pub fn true_peak_db(&self) -> f32 {
        lin_to_db(self.true_peak.max(1e-7))
    }
}

// ---------------------------------------------------------------------------
// Commands (UI -> audio)
// ---------------------------------------------------------------------------

pub enum Command {
    Play,
    Pause,
    Stop,
    SetPosition(f64),
    SetTempo(f64),
    SetLoop(Option<(f64, f64)>),
    SyncTrack { slot: usize, data: Box<TrackSync> },
    RemoveTrackSlot(usize),
    SyncMaster { fx: Vec<EffectInstance> },
    MoveClip { track: usize, clip: u64, start: f64 },
    DeleteClip { track: usize, clip: u64 },
    StartRecord { position: f64, capacity_frames: usize },
    StopRecord,
    Panic,
    SetSimulatedInput(bool),
}

pub struct TrackSync {
    pub id: TrackId,
    pub kind: TrackKind,
    pub clips: Vec<ClipSync>,
    pub fx: Vec<EffectInstance>,
    pub synth_patch: SynthPatch,
    pub reverb_send: f32,
    pub delay_send: f32,
    pub output_bus_slot: Option<usize>,
    pub vol_automation: Automation,
    pub pan_automation: Automation,
}

pub struct ClipSync {
    pub id: ClipId,
    pub start: f64,
    pub length: f64,
    pub offset: f64,
    pub take_id: u32,
    pub gain: f32,
    pub fades: (f32, f32),
    pub audio: Option<SharedAudio>,
    pub notes: Option<Vec<Note>>,
    pub muted: bool,
}

// ---------------------------------------------------------------------------
// Events (audio -> UI)
// ---------------------------------------------------------------------------

pub enum BackEvent {
    RecordedTake {
        track_id: TrackId,
        position: f64,
        samples: Vec<f32>,
    },
    Notice(String),
}

// ---------------------------------------------------------------------------
// Real-time track snapshot
// ---------------------------------------------------------------------------

pub struct ClipRT {
    pub id: ClipId,
    pub start: f64,
    pub length: f64,
    pub offset: f64,
    pub take_id: u32,
    pub gain: f32,
    pub fades: (f32, f32),
    pub audio: Option<SharedAudio>,
    pub notes: Option<Arc<Vec<Note>>>,
    pub muted: bool,
}

pub struct TrackRT {
    pub id: TrackId,
    pub kind: TrackKind,
    pub clips: Vec<ClipRT>,
    pub fx: Vec<FxUnit>,
    pub synth_patch: SynthPatch,
    pub synths: HashMap<ClipId, PolySynth>,
    pub reverb_send: f32,
    pub delay_send: f32,
    pub output_bus_slot: Option<usize>,
    pub vol_automation: Automation,
    pub pan_automation: Automation,
    pub buf: Vec<f32>,
}

fn empty_track_rt() -> TrackRT {
    TrackRT {
        id: 0,
        kind: TrackKind::Audio,
        clips: Vec::new(),
        fx: Vec::new(),
        synth_patch: SynthPatch::default(),
        synths: HashMap::new(),
        reverb_send: 0.0,
        delay_send: 0.0,
        output_bus_slot: None,
        vol_automation: Automation::default(),
        pan_automation: Automation::default(),
        buf: vec![0.0; BLOCK * 2],
    }
}

impl TrackRT {
    fn from_sync(sr: f32, data: &TrackSync) -> Self {
        let mut t = Self {
            id: data.id,
            kind: data.kind,
            clips: data
                .clips
                .iter()
                .map(|c| ClipRT {
                    id: c.id,
                    start: c.start,
                    length: c.length,
                    offset: c.offset,
                    take_id: c.take_id,
                    gain: db_to_lin(c.gain),
                    fades: c.fades,
                    audio: c.audio.clone(),
                    notes: c.notes.as_ref().map(|n| Arc::new(n.clone())),
                    muted: c.muted,
                })
                .collect(),
            fx: data.fx.iter().map(|i| FxUnit::build(i, sr)).collect(),
            synth_patch: data.synth_patch.clone(),
            synths: HashMap::new(),
            reverb_send: data.reverb_send,
            delay_send: data.delay_send,
            output_bus_slot: data.output_bus_slot,
            vol_automation: data.vol_automation.clone(),
            pan_automation: data.pan_automation.clone(),
            buf: vec![0.0; BLOCK * 2],
        };
        t.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
        for c in &t.clips {
            if c.notes.is_some() {
                let mut s = PolySynth::new(sr);
                s.set_patch(t.synth_patch.clone());
                t.synths.insert(c.id, s);
            }
        }
        t
    }
}

struct RecordBuf {
    track_id: TrackId,
    position: f64,
    samples: Vec<f32>,
    count: usize,
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

pub struct EngineRT {
    pub sr: f32,
    pub pos: f64,
    playing: bool,
    /// True only for bounce engines (never for the live engine).
    offline: bool,
    tracks: Vec<TrackRT>,
    master_fx: Vec<FxUnit>,
    params: Arc<ParamStore>,
    meters: Arc<MeterStore>,
    spectral: Arc<SpectralTap>,
    loudness: Arc<LoudnessTap>,
    loudness_meter: LoudnessMeter,
    cmd_rx: Option<Consumer<Command>>,
    cmd_tx: Option<Producer<Command>>,
    back_tx: Option<crossbeam_channel::Sender<BackEvent>>,
    back_rx: Option<crossbeam_channel::Receiver<BackEvent>>,
    input_rx: Option<Consumer<f32>>,
    input_tx: Option<Producer<f32>>,
    records: Vec<RecordBuf>,
    simulated_input: bool,
    sim_phase: f32,
    reverb_bus: Reverb,
    delay_bus: Delay,
    send_buf: Vec<f32>,
    cost_ema_us: f32,
    scratch: Vec<f32>,
    input_scratch: Vec<f32>,
}

impl EngineRT {
    pub fn new(
        sr: f32,
        params: Arc<ParamStore>,
        meters: Arc<MeterStore>,
        spectral: Arc<SpectralTap>,
        loudness: Arc<LoudnessTap>,
    ) -> Self {
        let (tx, rx) = RingBuffer::new(8192);
        let (itx, irx) = RingBuffer::new(1 << 17);
        let (btx, brx) = crossbeam_channel::bounded(128);
        Self {
            sr,
            pos: 0.0,
            playing: false,
            offline: false,
            tracks: Vec::new(),
            master_fx: Vec::new(),
            params,
            meters,
            spectral,
            loudness,
            loudness_meter: LoudnessMeter::new(sr),
            cmd_rx: Some(rx),
            cmd_tx: Some(tx),
            back_tx: Some(btx),
            back_rx: Some(brx),
            input_rx: Some(irx),
            input_tx: Some(itx),
            records: Vec::new(),
            simulated_input: false,
            sim_phase: 0.0,
            reverb_bus: Reverb::new(sr),
            delay_bus: Delay::new(sr),
            send_buf: vec![0.0; BLOCK * 2],
            cost_ema_us: 0.0,
            scratch: vec![0.0; BLOCK * 2],
            input_scratch: vec![0.0; BLOCK * 2],
        }
    }

    /// Offline constructor for bounce — same graph, no IO.
    pub fn offline(sr: f32) -> Self {
        let mut e = Self::offline_init(sr);
        e.playing = true;
        e.offline = true;
        e
    }
    fn offline_init(sr: f32) -> Self {
        let params = ParamStore::new();
        let meters = MeterStore::new();
        let spectral = Arc::new(SpectralTap::new(2048));
        let loudness = Arc::new(LoudnessTap {
            momentary_lu: AtomicU32::new(0),
            shortterm_lu: AtomicU32::new(0),
            integrated_lu: AtomicU32::new(0),
            true_peak_db: AtomicU32::new(0),
        });
        Self::new(sr, params, meters, spectral, loudness)
    }

    pub fn take_command_producer(&mut self) -> Option<Producer<Command>> {
        self.cmd_tx.take()
    }
    pub fn take_input_producer(&mut self) -> Option<Producer<f32>> {
        self.input_tx.take()
    }
    pub fn peek_input_producer(&self) -> Option<&Producer<f32>> {
        self.input_tx.as_ref()
    }
    pub fn take_back_receiver(&mut self) -> Option<crossbeam_channel::Receiver<BackEvent>> {
        self.back_rx.take()
    }
    pub fn meters(&self) -> &Arc<MeterStore> {
        &self.meters
    }

    pub fn sync_track(&mut self, slot: usize, data: &TrackSync) {
        while self.tracks.len() <= slot {
            self.tracks.push(empty_track_rt());
        }
        let mut t = TrackRT::from_sync(self.sr, data);
        t.buf = vec![0.0; (BLOCK * 2).max(t.buf.len())];
        self.tracks[slot] = t;
    }

    pub fn params(&self) -> &Arc<ParamStore> {
        &self.params
    }
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    fn handle_commands(&mut self) {
        let mut rx = match self.cmd_rx.take() {
            Some(r) => r,
            None => return,
        };
        let mut n = 0;
        while n < 256 {
            match rx.pop() {
                Ok(cmd) => {
                    self.apply(cmd);
                    n += 1;
                }
                Err(_) => break,
            }
        }
        self.cmd_rx = Some(rx);
    }

    pub fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Play => {
                self.playing = true;
                self.params.playing.store(true, Ordering::Relaxed);
            }
            Command::Pause | Command::Stop => {
                self.playing = false;
                self.params.playing.store(false, Ordering::Relaxed);
                for t in &mut self.tracks {
                    for s in t.synths.values_mut() {
                        s.all_notes_off();
                    }
                }
            }
            Command::SetPosition(p) => self.pos = p.max(0.0),
            Command::SetTempo(bpm) => {
                self.params.tempo.store((bpm as f32).to_bits(), Ordering::Relaxed)
            }
            Command::SetLoop(l) => self.params.set_loop(l),
            Command::SyncTrack { slot, data } => self.sync_track(slot, &data),
            Command::RemoveTrackSlot(slot) => {
                if slot < self.tracks.len() {
                    self.tracks.remove(slot);
                }
            }
            Command::SyncMaster { fx } => {
                self.master_fx = fx.iter().map(|i| FxUnit::build(i, self.sr)).collect();
            }
            Command::MoveClip { track, clip, start } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    if let Some(c) = t.clips.iter_mut().find(|c| c.id == clip) {
                        c.start = start;
                    }
                    t.clips.sort_by(|a, b| a.start.total_cmp(&b.start));
                }
            }
            Command::DeleteClip { track, clip } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.clips.retain(|c| c.id != clip);
                }
            }
            Command::StartRecord {
                position,
                capacity_frames,
            } => {
                self.records.clear();
                for (slot, t) in self.tracks.iter().enumerate() {
                    let flags = self.params.slots[slot].flags.load(Ordering::Relaxed);
                    if flags & 4 != 0 {
                        self.records.push(RecordBuf {
                            track_id: t.id,
                            position,
                            samples: vec![0.0; capacity_frames.max(2) * 2],
                            count: 0,
                        });
                    }
                }
                #[cfg(feature = "debug_record")]
                eprintln!("[eng] StartRecord: {} armed record buffers (sim={})", self.records.len(), self.simulated_input);
                if let Some(b) = &self.back_tx {
                    let _ = b.send(BackEvent::Notice(format!(
                        "Recording armed on {} track(s) @ {:.2}s",
                        self.records.len(),
                        position
                    )));
                }
            }
            Command::StopRecord => {
                let recs = std::mem::take(&mut self.records);
                for r in recs {
                    let mut samples = r.samples;
                    samples.truncate(r.count);
                    if let Some(b) = &self.back_tx {
                        let _ = b.send(BackEvent::RecordedTake {
                            track_id: r.track_id,
                            position: r.position,
                            samples,
                        });
                    }
                }
            }
            Command::Panic => {
                for t in &mut self.tracks {
                    for s in t.synths.values_mut() {
                        s.all_notes_off();
                    }
                }
            }
            Command::SetSimulatedInput(v) => self.simulated_input = v,
        }
    }

    /// Render one block of interleaved stereo into `io` (len >= frames*2).
    pub fn process_block(&mut self, io: &mut [f32], frames: usize) {
        let t0 = Some(std::time::Instant::now());
        self.handle_commands();

        let sr = self.sr;
        let dt = frames as f64 / sr as f64;
        let tempo = f32::from_bits(self.params.tempo.load(Ordering::Relaxed)) as f64;
        let bps = tempo / 60.0;
        let mut solo_any = false;
        for slot in 0..self.tracks.len() {
            if self.params.slots[slot].flags.load(Ordering::Relaxed) & 2 != 0 {
                solo_any = true;
                break;
            }
        }

        io[..frames * 2].fill(0.0);
        if self.send_buf.len() < frames * 2 {
            self.send_buf.resize(frames * 2, 0.0);
        }
        self.send_buf[..frames * 2].fill(0.0);
        let offline = self.offline;

        // ---- live input flow -------------------------------------------
        // The input ring is drained on EVERY live block so an external
        // device feed (real microphone via cpal) is never backlogged, the
        // input meter is always live, and monitoring works even before
        // recording starts. When no external feed exists, the simulated
        // source (CI/headless) can push into the same ring.
        if !offline {
            if self.simulated_input {
                self.generate_simulated_input(frames);
            }
            if self.input_scratch.len() < frames * 2 {
                self.input_scratch.resize(frames * 2, 0.0);
            }
            let mut got = 0usize;
            if let Some(rx) = &mut self.input_rx {
                while got < frames * 2 {
                    match rx.pop() {
                        Ok(v) => {
                            self.input_scratch[got] = v;
                            got += 1;
                        }
                        Err(_) => break,
                    }
                }
            }
            for v in &mut self.input_scratch[got..frames * 2] {
                *v = 0.0;
            }
            let pk = self.input_scratch[..frames * 2]
                .iter()
                .fold(0.0f32, |m, v| m.max(v.abs()));
            let prev = f32::from_bits(self.meters.input_peak.load(Ordering::Relaxed));
            self.meters
                .input_peak
                .store(pk.max(prev * 0.82).to_bits(), Ordering::Relaxed);
            #[cfg(feature = "debug_record")]
            if self.meters.blocks.load(Ordering::Relaxed) % 100 == 0 {
                eprintln!("[eng] input: got={got} pk={pk:.3} recs={}", self.records.len());
            }
            if !self.records.is_empty() {
                let full = self.input_scratch[..frames * 2].to_vec();
                let mut stop_all = false;
                for rec in &mut self.records {
                    if rec.count + frames * 2 <= rec.samples.len() {
                        rec.samples[rec.count..rec.count + frames * 2].copy_from_slice(&full);
                        rec.count += frames * 2;
                    } else {
                        stop_all = true;
                    }
                }
                if stop_all {
                    let recs = std::mem::take(&mut self.records);
                    for r in recs {
                        let mut samples = r.samples;
                        samples.truncate(r.count);
                        if let Some(b) = &self.back_tx {
                            let _ = b.send(BackEvent::RecordedTake {
                                track_id: r.track_id,
                                position: r.position,
                                samples,
                            });
                        }
                    }
                }
            }
        }

        let n_tracks = self.tracks.len();
        let is_bus: Vec<bool> = self
            .tracks
            .iter()
            .map(|t| t.kind == TrackKind::Bus)
            .collect();
        for t in self.tracks.iter_mut() {
            if t.kind == TrackKind::Bus && t.buf.len() >= frames * 2 {
                t.buf[..frames * 2].fill(0.0);
            }
        }

        for slot in 0..n_tracks {
            // --- read hot params (atomics) before taking the track borrow ---
            let ps = &self.params.slots[slot];
            let vol_db = f32::from_bits(ps.volume_db.load(Ordering::Relaxed));
            let mut pan = f32::from_bits(ps.pan.load(Ordering::Relaxed));
            let flags = ps.flags.load(Ordering::Relaxed);
            let mute = flags & 1 != 0;
            let solo = flags & 2 != 0;
            let _armed = flags & 4 != 0;
            let monitoring = flags & 8 != 0;
            if mute || (solo_any && !solo) {
                continue;
            }
            let t = &mut self.tracks[slot];
            let bus_mode = t.kind == TrackKind::Bus;
            if t.id == 0 && t.clips.is_empty() {
                continue;
            }
            if t.buf.len() < frames * 2 {
                t.buf.resize(frames * 2, 0.0);
            }
            if !bus_mode {
                t.buf[..frames * 2].fill(0.0);
            }
            let mut routed_to_bus: Option<usize> = None;
            if bus_mode {
                routed_to_bus = None; // bus outputs straight to master below
            } else if let Some(bs) = t.output_bus_slot {
                if bs < n_tracks && bs != slot && is_bus[bs] {
                    routed_to_bus = Some(bs);
                }
            }

            // ---------- source render ----------
            if self.playing && !bus_mode {
                let pos0 = self.pos;
                let pos1 = self.pos + dt;
                for ci in 0..t.clips.len() {
                    let c = &t.clips[ci];
                    if c.muted || !(c.start < pos1 && c.start + c.length > pos0) {
                        continue;
                    }
                    let clip_frames = (c.length * sr as f64) as usize;
                    if let Some(audio) = &c.audio {
                        let ch = audio.channels.max(1) as usize;
                        let src_frames = audio.frames();
                        let rate_ratio = audio.sample_rate as f64 / sr as f64;
                        let mut out_off = 0usize;
                        if pos0 < c.start {
                            out_off = ((c.start - pos0) * sr as f64) as usize;
                        }
                        let mut fi = (c.offset + (pos0 - c.start).max(0.0)) * rate_ratio;
                        let fin = (c.fades.0 as f64 * sr as f64) as f32;
                        let fout = (c.fades.1 as f64 * sr as f64) as f32;
                        for i in out_off..frames {
                            let s0 = fi as usize;
                            if s0 >= clip_frames || s0 >= src_frames {
                                break;
                            }
                            if i * 2 + 1 >= t.buf.len() {
                                break;
                            }
                            let frac = (fi - s0 as f64) as f32;
                            let base_src = s0 * ch;
                            if base_src >= audio.samples.len() {
                                break;
                            }
                            let mut l = audio.samples[base_src];
                            let mut r = if ch > 1 && base_src + 1 < audio.samples.len() {
                                audio.samples[base_src + 1]
                            } else {
                                l
                            };
                            if base_src + ch < audio.samples.len() {
                                let nl = audio.samples[base_src + ch];
                                let nr = if ch > 1 && base_src + ch + 1 < audio.samples.len() {
                                    audio.samples[base_src + ch + 1]
                                } else {
                                    nl
                                };
                                l += (nl - l) * frac;
                                r += (nr - r) * frac;
                            }
                            let rel = fi as f32;
                            let mut g = c.gain;
                            if rel < fin {
                                g *= rel / fin.max(1.0);
                            }
                            let rem = (clip_frames - s0) as f32;
                            if rem < fout {
                                g *= rem / fout.max(1.0);
                            }
                            let idx = i * 2;
                            t.buf[idx] += l * g;
                            t.buf[idx + 1] += r * g;
                            fi += rate_ratio;
                        }
                    } else if let Some(notes) = &c.notes {
                        let s = t.synths.entry(c.id).or_insert_with(|| {
                            let mut s = PolySynth::new(sr);
                            s.set_patch(t.synth_patch.clone());
                            s
                        });
                        let local0 = ((pos0 - c.start) * bps).max(0.0);
                        let local1 = ((pos1 - c.start) * bps).max(0.0);
                        schedule_notes(s, notes, local0, local1, tempo, sr);
                        let mut tmp = vec![0.0f32; frames];
                        s.render_mono(&mut tmp);
                        let base = ((pos0 - c.start).max(0.0) * sr as f64) as usize;
                        let fin = (c.fades.0 as f64 * sr as f64) as f32;
                        let fout = (c.fades.1 as f64 * sr as f64) as f32;
                        for (i, v) in tmp.iter().enumerate() {
                            let fi2 = base + i;
                            if fi2 < clip_frames && i * 2 + 1 < t.buf.len() {
                                let mut g = c.gain;
                                if (fi2 as f32) < fin {
                                    g *= fi2 as f32 / fin.max(1.0);
                                }
                                let rem = (clip_frames - fi2) as f32;
                                if rem < fout {
                                    g *= rem / fout.max(1.0);
                                }
                                t.buf[i * 2] += v * g;
                                t.buf[i * 2 + 1] += v * g;
                            }
                        }
                    }
                }
            }

            // ---------- low-latency monitoring ----------
            // Monitor whenever the track requests it (independent of record
            // arming) so a singer can warm up with the full chain live.
            if monitoring && !offline && !self.input_scratch.is_empty() {
                for i in 0..frames {
                    let idx = i * 2;
                    if idx + 1 < t.buf.len() && idx + 1 < self.input_scratch.len() {
                        t.buf[idx] += self.input_scratch[idx] * 0.9;
                        t.buf[idx + 1] += self.input_scratch[idx + 1] * 0.9;
                    }
                }
            }

            // ---------- volume / pan / automation ----------
            let mut vol = db_to_lin(vol_db);
            if t.vol_automation.enabled && !t.vol_automation.points.is_empty() && self.playing {
                vol = db_to_lin(t.vol_automation.eval(self.pos, vol_db));
            }
            if t.pan_automation.enabled && !t.pan_automation.points.is_empty() && self.playing {
                pan = t.pan_automation.eval(self.pos, pan).clamp(-1.0, 1.0);
            }
            let (gl, gr) = if pan >= 0.0 {
                ((1.0 - pan).sqrt(), 1.0)
            } else {
                (1.0, (1.0 + pan).sqrt())
            };

            // ---------- fx chain ----------
            for fx in t.fx.iter_mut() {
                for f in 0..frames {
                    let (mut l, mut r) = (t.buf[f * 2], t.buf[f * 2 + 1]);
                    fx.process(&mut l, &mut r);
                    t.buf[f * 2] = l;
                    t.buf[f * 2 + 1] = r;
                }
            }

            // ---------- mix into scratch, sends, meters ----------
            let rs = t.reverb_send;
            let ds = t.delay_send;
            let sb = &mut self.send_buf[..frames * 2];
            let scr = if frames * 2 <= self.scratch.len() {
                &mut self.scratch[..frames * 2]
            } else {
                self.scratch.resize(frames * 2, 0.0);
                &mut self.scratch[..frames * 2]
            };
            let mut peak = 0.0f32;
            let mut acc = 0.0f32;
            for f in 0..frames {
                let l = t.buf[f * 2] * vol * gl;
                let r = t.buf[f * 2 + 1] * vol * gr;
                if rs > 0.0 || ds > 0.0 {
                    sb[f * 2] += l * rs + r * ds * 0.5;
                    sb[f * 2 + 1] += r * rs + l * ds * 0.5;
                }
                scr[f * 2] = l;
                scr[f * 2 + 1] = r;
                let pa = l.abs().max(r.abs());
                if pa > peak {
                    peak = pa;
                }
                acc += l * l + r * r;
            }
            // meters
            let mprev = f32::from_bits(self.meters.track_peak[slot].load(Ordering::Relaxed));
            self.meters.track_peak[slot].store(
                peak.max(mprev * 0.85).to_bits(),
                Ordering::Relaxed,
            );
            let rms = (acc / frames as f32 / 2.0).sqrt();
            let rprev = f32::from_bits(self.meters.track_rms[slot].load(Ordering::Relaxed));
            self.meters.track_rms[slot].store(
                rms.max(rprev * 0.9).to_bits(),
                Ordering::Relaxed,
            );

            // ---------- routing (track borrow released conceptually) ----------
            // We cannot touch self.tracks[bslot] while `t` is borrowed, so we
            // staged the output in `self.scratch`; apply it now via indices.
            match routed_to_bus {
                Some(bs) => {
                    let bb = &mut self.tracks[bs].buf;
                    for f in 0..frames {
                        bb[f * 2] += scr[f * 2];
                        bb[f * 2 + 1] += scr[f * 2 + 1];
                    }
                }
                None => {
                    for f in 0..frames {
                        io[f * 2] += scr[f * 2];
                        io[f * 2 + 1] += scr[f * 2 + 1];
                    }
                }
            }
        }

        // ---------- send buses ----------
        {
            for f in 0..frames {
                let (mut l, mut r) = (self.send_buf[f * 2], self.send_buf[f * 2 + 1]);
                self.reverb_bus.process(&mut l, &mut r);
                self.send_buf[f * 2] = l * 0.5;
                self.send_buf[f * 2 + 1] = r * 0.5;
            }
            for f in 0..frames {
                let (mut l, mut r) = (self.send_buf[f * 2], self.send_buf[f * 2 + 1]);
                self.delay_bus.process(&mut l, &mut r);
                self.send_buf[f * 2] = l * 0.5;
                self.send_buf[f * 2 + 1] = r * 0.5;
            }
            for f in 0..frames {
                io[f * 2] += self.send_buf[f * 2];
                io[f * 2 + 1] += self.send_buf[f * 2 + 1];
            }
        }

        // ---------- master chain ----------
        for fx in self.master_fx.iter_mut() {
            for f in 0..frames {
                let (mut l, mut r) = (io[f * 2], io[f * 2 + 1]);
                fx.process(&mut l, &mut r);
                io[f * 2] = l;
                io[f * 2 + 1] = r;
            }
        }
        let mvol = db_to_lin(f32::from_bits(
            self.params.master_volume_db.load(Ordering::Relaxed),
        ));
        for v in io[..frames * 2].iter_mut() {
            *v *= mvol;
        }

        // ---------- metrics / analysis ----------
        if !offline {
            let mut pl = 0.0f32;
            let mut pr = 0.0f32;
            let mut al = 0.0f32;
            let mut ar = 0.0f32;
            for f in 0..frames {
                let (l, r) = (io[f * 2], io[f * 2 + 1]);
                pl = pl.max(l.abs());
                pr = pr.max(r.abs());
                al += l * l;
                ar += r * r;
            }
            let m = &self.meters;
            let prevl = f32::from_bits(m.master_peak_l.load(Ordering::Relaxed));
            m.master_peak_l
                .store(pl.max(prevl * 0.85).to_bits(), Ordering::Relaxed);
            let prevr = f32::from_bits(m.master_peak_r.load(Ordering::Relaxed));
            m.master_peak_r
                .store(pr.max(prevr * 0.85).to_bits(), Ordering::Relaxed);
            let rl = (al / frames as f32).sqrt();
            let rr = (ar / frames as f32).sqrt();
            let prevrl = f32::from_bits(m.master_rms_l.load(Ordering::Relaxed));
            m.master_rms_l
                .store(rl.max(prevrl * 0.9).to_bits(), Ordering::Relaxed);
            let prevrr = f32::from_bits(m.master_rms_r.load(Ordering::Relaxed));
            m.master_rms_r
                .store(rr.max(prevrr * 0.9).to_bits(), Ordering::Relaxed);

            let lu = self.loudness_meter.push(io, frames);
            self.loudness.momentary_lu.store((lu as f32).to_bits(), Ordering::Relaxed);
            let st = self.loudness_meter.shortterm();
            self.loudness.shortterm_lu.store((st as f32).to_bits(), Ordering::Relaxed);
            let it = self.loudness_meter.integrated();
            self.loudness.integrated_lu.store((it as f32).to_bits(), Ordering::Relaxed);
            self.loudness
                .true_peak_db
                .store(self.loudness_meter.true_peak_db().to_bits(), Ordering::Relaxed);

            if self.meters.blocks.load(Ordering::Relaxed) % 4 == 0 {
                if let Ok(mut b) = self.spectral.buf.try_lock() {
                    let need = 2048;
                    if b.len() != need {
                        b.resize(need, 0.0);
                    }
                    let take = frames.min(need / 2);
                    b.copy_within(take.., 0);
                    for i in 0..take {
                        b[need - take + i] = (io[i * 2] + io[i * 2 + 1]) * 0.5;
                    }
                    self.spectral.version.fetch_add(1, Ordering::Relaxed);
                }
            }
            self.meters.blocks.fetch_add(1, Ordering::Relaxed);

            if let Some(t0) = t0 {
                let us = t0.elapsed().as_secs_f32() * 1e6;
                self.cost_ema_us = self.cost_ema_us * 0.95 + us * 0.05;
                self.meters
                    .callback_us
                    .store(self.cost_ema_us as u32, Ordering::Relaxed);
            }
        }

        // ---------- transport advance + loop ----------
        if self.playing {
            self.pos += dt;
            if self.params.loop_enabled.load(Ordering::Relaxed) {
                let ls = f32::from_bits(self.params.loop_start.load(Ordering::Relaxed)) as f64;
                let le = f32::from_bits(self.params.loop_end.load(Ordering::Relaxed)) as f64;
                if le > ls + 0.01 && self.pos >= le {
                    self.pos = ls + (self.pos - le);
                }
            }
        }
    }

    #[inline]
    fn offline_mode(&self) -> bool {
        self.cmd_tx.is_none()
    }

    /// Feed the simulated input source (used when no physical mic exists).
    /// Generates a breathy formant "vocal" phrase with noise + hum so the AI
    /// cleanup pipeline can be genuinely exercised headlessly.
    pub fn generate_simulated_input(&mut self, frames: usize) {
        if !self.simulated_input {
            return;
        }
        let sr = self.sr;
        let mut peak = 0.0f32;
        if let Some(tx) = &mut self.input_tx {
            for i in 0..frames {
                let t = self.sim_phase / sr;
                self.sim_phase += 1.0;
                // phrase envelope: 0.8s sing, 0.5s rest (breath), repeating
                let cycle = t % 3.0;
                let env = if cycle < 0.8 {
                    ((cycle / 0.15).min(1.0) * ((0.8 - cycle) / 0.1).min(1.0)).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let f0 = 165.0 + (t * 2.1).sin() * 8.0;
                let phase = f0 * t;
                let glottal = (phase * std::f32::consts::TAU).sin() * 0.5
                    + ((phase * 2.0) % 1.0) * 0.25
                    + ((phase * 3.0) % 1.0) * 0.12;
                // breath noise during rests
                let breath = if cycle >= 0.8 && cycle < 1.3 {
                    let be = ((cycle - 0.8) / 0.1).min(1.0) * ((1.3 - cycle) / 0.15).min(1.0);
                    (rand_f32() - 0.5) * 0.24 * be.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                // background noise + 50 Hz hum (the stuff AI cleanup removes)
                let noise = (rand_f32() - 0.5) * 0.035;
                let hum = (t * std::f32::consts::TAU * 50.0).sin() * 0.02;
                let s = glottal * 0.22 * env + breath + noise + hum;
                peak = peak.max(s.abs());
                let _ = tx.push(s * 0.8);
                let _ = tx.push(s * 0.8);
            }
        }
        self.meters
            .input_peak
            .store(peak.to_bits(), Ordering::Relaxed);
    }

    /// Build the engine graph from a project.
    pub fn load_project(&mut self, project: &Project) {
        self.tracks.clear();
        let bus_slot: HashMap<TrackId, usize> = project
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == TrackKind::Bus)
            .map(|(i, t)| (t.id, i))
            .collect();
        for (slot, t) in project.tracks.iter().enumerate() {
            let out = t
                .output_bus
                .and_then(|id| bus_slot.get(&id).copied())
                .filter(|bs| *bs != slot);
            let sync = TrackSync {
                id: t.id,
                kind: t.kind,
                clips: t
                    .clips
                    .iter()
                    .map(|c| ClipSync {
                        id: c.id,
                        start: c.start,
                        length: c.length,
                        offset: c.offset,
                        take_id: c.take_id,
                        gain: c.gain_db,
                        fades: c.fades,
                        audio: c.audio.clone(),
                        notes: c.notes.clone(),
                        muted: c.muted,
                    })
                    .collect(),
                fx: t.fx.clone(),
                synth_patch: t.synth.clone(),
                reverb_send: t.reverb_send,
                delay_send: t.delay_send,
                output_bus_slot: out,
                vol_automation: t.volume_automation.clone(),
                pan_automation: t.pan_automation.clone(),
            };
            self.sync_track(slot, &sync);
            self.params.set_track(slot, t);
        }
        self.params
            .tempo
            .store((project.tempo as f32).to_bits(), Ordering::Relaxed);
        self.master_fx = project
            .master_fx
            .iter()
            .map(|i| FxUnit::build(i, self.sr))
            .collect();
        self.pos = 0.0;
    }
}

/// Cheap deterministic-ish noise for the simulated input source.
#[inline]
fn rand_f32() -> f32 {
    // xorshift-ish LCG on a thread-local
    use std::cell::Cell;
    thread_local! {
        static S: Cell<u64> = Cell::new(0x9E3779B97F4A7C15);
    }
    S.with(|s| {
        let mut x = s.get();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        s.set(x);
        (x >> 40) as f32 / (1u64 << 24) as f32
    })
}
