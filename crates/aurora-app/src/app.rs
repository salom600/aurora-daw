//! AURORA application core — owns the project, the engine bridge and all
//! workflow actions. Panels are implemented as `impl AuroraApp` blocks in
//! the `panels` module files.

use aurora_engine::ai::CleanupOptions;
use aurora_engine::audio_io::{create_engine_parts, AudioIO, EngineParts};
use aurora_engine::engine::{BackEvent, Command, EngineRT, MeterStore, ParamStore, LoudnessTap, SpectralTap};
use aurora_engine::effects::{EffectInstance, EffectType};
use aurora_engine::io::ExportFormat;
use aurora_engine::jobs::{JobHandle, JobKind, JobManager, JobOutcome};
use aurora_engine::project::*;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// UI state helpers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum Tool {
    Select,
    Cut,
    Draw,
}

#[derive(Clone)]
pub struct ExportDlg {
    pub format: ExportFormat,
    pub sample_rate: u32,
    pub stems: bool,
    pub dir: String,
    pub name: String,
    pub range_full: bool,
    pub from: f64,
    pub to: f64,
}

pub struct AiMixState {
    pub analyzing: bool,
    pub analyzed: bool,
    pub suggestions: Vec<aurora_engine::ai::MixSuggestion>,
    pub confidence: f32,
    pub applied: bool,
}

pub struct CleanerState {
    pub options: CleanupOptions,
    pub last_reports: Vec<(String, aurora_engine::ai::CleanupReport)>,
}

pub struct AuroraApp {
    pub project: Project,
    // engine bridge
    pub cmd_tx: Option<rtrb::Producer<Command>>,
    pub back_rx: Option<crossbeam_channel::Receiver<BackEvent>>,
    pub parts: EngineParts,
    pub audio: Option<AudioIO>,
    pub playing: bool,
    pub play_wall_start: Instant,
    pub play_pos_start: f64,
    pub recording: bool,
    pub rec_start: f64,
    // jobs
    pub jobs: JobManager,
    pub active_jobs: Vec<(String, JobHandle)>,
    pub last_export: Option<String>,
    // view
    pub selected_track: Option<TrackId>,
    pub selected_clip: Option<ClipId>,
    pub zoom: f32,
    pub h_scroll: f64,
    pub v_scroll: f32,
    pub tool: Tool,
    pub snap: bool,
    pub browser_tab: usize,
    pub mixer_open: bool,
    pub arranger_share: f32,
    // dialogs & windows
    pub export_dlg: Option<ExportDlg>,
    pub fx_windows: Vec<u64>,
    pub fx_selected: std::collections::HashMap<u64, usize>,
    pub piano_roll: Option<(TrackId, ClipId)>,
    pub about_open: bool,
    // ai
    pub ai_mix: AiMixState,
    pub cleaner: CleanerState,
    // infra
    pub graph_dirty: bool,
    pub toasts: VecDeque<(String, Instant)>,
    pub start_time: Instant,
    pub ram_mb: f32,
    pub boot_ms: f64,
    pub status: String,
    pub import_path: String,
    pub spec_smooth: Vec<f32>,
    pub lufs_history: Vec<f32>,
    pub drag_clip: Option<(TrackId, ClipId, f32)>, // grab x anchor
    pub drag_lane: Option<(TrackId, f64)>,
    pub shots_dir: Option<String>,
    pub autotest: Option<crate::autotest::AutoTest>,
    pub stress_count: usize,
}

impl AuroraApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: AppOptions) -> Self {
        crate::theme::Theme::apply(&cc.egui_ctx);
        let (mut engine, parts) = create_engine_parts(48000.0);
        let project = if opts.stress > 0 {
            aurora_engine::demo::build_stress_project(opts.stress, 0.08)
        } else if opts.empty {
            let mut p = Project::new_empty("Untitled Session");
            p.add_track("AUDIO 1", TrackKind::Audio, [94, 234, 212, 255]);
            p
        } else {
            aurora_engine::demo::build_demo_project()
        };
        engine.load_project(&project);
        let cmd_tx = engine.take_command_producer();
        let back_rx = engine.take_back_receiver();
        let audio = AudioIO::start(engine);
        parts
            .meters
            .driver_kind
            .store(match audio.kind {
                aurora_engine::audio_io::DriverKind::RealDevice => 1,
                aurora_engine::audio_io::DriverKind::Synthetic => 2,
            }, Ordering::Relaxed);

        let selected = project.tracks.first().map(|t| t.id);
        let app = Self {
            boot_ms: 0.0,
            project,
            cmd_tx,
            back_rx,
            parts,
            audio: Some(audio),
            playing: false,
            play_wall_start: Instant::now(),
            play_pos_start: 0.0,
            recording: false,
            rec_start: 0.0,
            jobs: JobManager::new(),
            active_jobs: Vec::new(),
            last_export: None,
            selected_track: selected,
            selected_clip: None,
            zoom: 42.0,
            h_scroll: 0.0,
            v_scroll: 0.0,
            tool: Tool::Select,
            snap: true,
            browser_tab: 0,
            mixer_open: true,
            arranger_share: 0.62,
            export_dlg: None,
            fx_windows: Vec::new(),
            fx_selected: std::collections::HashMap::new(),
            piano_roll: None,
            about_open: false,
            ai_mix: AiMixState {
                analyzing: false,
                analyzed: false,
                suggestions: Vec::new(),
                confidence: 0.0,
                applied: false,
            },
            cleaner: CleanerState {
                options: CleanupOptions::default(),
                last_reports: Vec::new(),
            },
            graph_dirty: false,
            toasts: VecDeque::new(),
            start_time: Instant::now(),
            ram_mb: 0.0,
            status: "Ready".into(),
            import_path: String::new(),
            spec_smooth: vec![0.0; 48],
            lufs_history: Vec::new(),
            drag_clip: None,
            drag_lane: None,
            shots_dir: opts.shots_dir.clone(),
            autotest: opts.autotest.then(crate::autotest::AutoTest::new),
            stress_count: opts.stress,
        };
        app
    }

    // ------------------------------------------------------------------
    // Engine bridge
    // ------------------------------------------------------------------

    pub fn send(&mut self, cmd: Command) {
        if let Some(tx) = &mut self.cmd_tx {
            if tx.push(cmd).is_err() {
                self.status = "Command queue full — engine busy".into();
            }
        }
    }

    /// Push all hot params to the engine atomics (cheap, every frame).
    pub fn sync_params(&mut self) {
        let p = &self.project;
        let store: &ParamStore = &self.parts.params;
        for (slot, t) in p.tracks.iter().enumerate() {
            store.set_track(slot, t);
        }
        store
            .master_volume_db
            .store(p.master_volume_db.to_bits(), Ordering::Relaxed);
        store.tempo.store((p.tempo as f32).to_bits(), Ordering::Relaxed);
        let loop_range = if p.loop_enabled {
            Some(p.loop_range)
        } else {
            None
        };
        store.set_loop(loop_range);
    }

    /// Rebuild engine graph after structural edits.
    pub fn mark_graph_dirty(&mut self) {
        self.graph_dirty = true;
    }

    fn push_graph(&mut self) {
        let bus_slot: std::collections::HashMap<TrackId, usize> = self
            .project
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind == TrackKind::Bus)
            .map(|(i, t)| (t.id, i))
            .collect();
        let syncs: Vec<(usize, aurora_engine::engine::TrackSync)> = self.project.tracks.iter().enumerate().map(|(slot, t)| {
            let out = t
                .output_bus
                .and_then(|id| bus_slot.get(&id).copied())
                .filter(|bs| *bs != slot);
            let sync = aurora_engine::engine::TrackSync {
                id: t.id,
                kind: t.kind,
                clips: t
                    .clips
                    .iter()
                    .map(|c| aurora_engine::engine::ClipSync {
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
            (slot, sync)
        }).collect();
        for (slot, sync) in syncs {
            self.send(Command::SyncTrack {
                slot,
                data: Box::new(sync),
            });
        }
        let master_fx = self.project.master_fx.clone();
        self.send(Command::SyncMaster { fx: master_fx });
        self.graph_dirty = false;
    }

    // ------------------------------------------------------------------
    // Transport
    // ------------------------------------------------------------------

    pub fn play(&mut self) {
        if !self.playing {
            self.playing = true;
            self.play_wall_start = Instant::now();
            self.play_pos_start = self.engine_pos();
            self.send(Command::SetPosition(self.play_pos_start));
            self.send(Command::Play);
            self.status = "Playing".into();
        }
    }

    pub fn pause(&mut self) {
        if self.playing {
            self.playing = false;
            let pos = self.engine_pos();
            self.send(Command::Pause);
            self.send(Command::SetPosition(pos));
            self.status = "Paused".into();
        }
    }

    pub fn stop(&mut self) {
        if self.recording {
            self.record_stop();
        }
        self.playing = false;
        self.play_pos_start = 0.0;
        self.send(Command::Stop);
        self.send(Command::SetPosition(0.0));
        self.status = "Stopped".into();
    }

    pub fn seek(&mut self, pos: f64) {
        let pos = pos.max(0.0);
        self.send(Command::SetPosition(pos));
        self.play_pos_start = pos;
        self.play_wall_start = Instant::now();
    }

    pub fn engine_pos(&self) -> f64 {
        if self.playing {
            let el = self.play_wall_start.elapsed().as_secs_f64();
            let mut pos = self.play_pos_start + el;
            if self.project.loop_enabled {
                let (a, b) = self.project.loop_range;
                if b > a + 0.01 && pos >= b {
                    pos = a + (pos - b) % (b - a);
                }
            }
            pos
        } else {
            self.play_pos_start
        }
    }

    pub fn toggle_loop(&mut self) {
        self.project.loop_enabled = !self.project.loop_enabled;
        let l = if self.project.loop_enabled {
            Some(self.project.loop_range)
        } else {
            None
        };
        self.send(Command::SetLoop(l));
    }

    // ------------------------------------------------------------------
    // Recording
    // ------------------------------------------------------------------

    pub fn record_start(&mut self) {
        if self.recording {
            return;
        }
        let pos = self.engine_pos();
        // push armed/monitor flags to the engine atomics BEFORE the start
        // command, so the audio side sees them when it applies the command
        self.sync_params();
        let capacity = 48000 * 60; // 60 s preallocated capture window per track
        self.send(Command::StartRecord {
            position: pos,
            capacity_frames: capacity,
        });
        self.recording = true;
        self.rec_start = pos;
        self.play();
        self.status = format!("Recording from {pos:.2}s — armed tracks capturing");
    }

    pub fn record_stop(&mut self) {
        if !self.recording {
            return;
        }
        self.send(Command::StopRecord);
        self.recording = false;
        self.status = "Recording finished — take placed on timeline".into();
    }

    // ------------------------------------------------------------------
    // Editing
    // ------------------------------------------------------------------

    pub fn add_audio_track(&mut self) {
        let colors = [
            [45, 212, 191, 255],
            [56, 189, 248, 255],
            [167, 139, 250, 255],
            [251, 146, 60, 255],
            [74, 222, 128, 255],
            [248, 113, 113, 255],
        ];
        let i = self.project.tracks.len();
        let (id, name) = {
            let t = self.project.add_track(
                &format!("AUDIO {}", i + 1),
                TrackKind::Audio,
                colors[i % colors.len()],
            );
            (t.id, t.name.clone())
        };
        self.selected_track = Some(id);
        self.mark_graph_dirty();
        self.status = format!("Track {name} added");
    }

    pub fn add_instrument_track(&mut self) {
        let i = self.project.tracks.iter().filter(|t| t.kind == TrackKind::Instrument).count();
        let id = {
            let t = self.project.add_track(
                &format!("INSTRUMENT {}", i + 1),
                TrackKind::Instrument,
                [129, 140, 248, 255],
            );
            t.id
        };
        self.selected_track = Some(id);
        self.mark_graph_dirty();
        self.status = "Instrument track added — double-click a MIDI clip to edit notes".into();
    }

    pub fn add_bus_track(&mut self) {
        let i = self.project.tracks.iter().filter(|t| t.kind == TrackKind::Bus).count();
        let id = {
            let t = self
                .project
                .add_track(&format!("BUS {}", i + 1), TrackKind::Bus, [244, 114, 182, 255]);
            t.id
        };
        self.selected_track = Some(id);
        self.mark_graph_dirty();
        self.status = "Bus added".into();
    }

    pub fn delete_selected_track(&mut self) {
        if let Some(id) = self.selected_track {
            if let Some(slot) = self.project.tracks.iter().position(|t| t.id == id) {
                self.project.tracks.remove(slot);
                self.send(Command::RemoveTrackSlot(slot));
                self.selected_track = self.project.tracks.first().map(|t| t.id);
                // resync all slots after removal
                self.graph_dirty = true;
                self.status = "Track deleted".into();
            }
        }
    }

    pub fn split_clip_at(&mut self, track_id: TrackId, clip_id: ClipId, at: f64) {
        let new_id = self.project.alloc_id();
        let mut did = false;
        if let Some(t) = self.project.track_by_id_mut(track_id) {
            if let Some(idx) = t.clips.iter().position(|c| c.id == clip_id) {
                let c = t.clips[idx].clone();
                if at > c.start + 0.01 && at < c.end() - 0.01 {
                    let mut right = c.clone();
                    right.id = new_id;
                    right.start = at;
                    right.length = c.end() - at;
                    right.offset = c.offset + (at - c.start);
                    let mut left = c;
                    left.length = at - left.start;
                    t.clips[idx] = left;
                    t.clips.push(right);
                    did = true;
                }
            }
        }
        if did {
            self.mark_graph_dirty();
        }
    }

    pub fn duplicate_selected_clip(&mut self) {
        let (tid, cid) = match (self.selected_track, self.selected_clip) {
            (Some(t), Some(c)) => (t, c),
            _ => return,
        };
        let new_id = self.project.alloc_id();
        let mut dup: Option<Clip> = None;
        if let Some(t) = self.project.track_by_id_mut(tid) {
            if let Some(c) = t.clips.iter().find(|c| c.id == cid) {
                let mut nc = c.clone();
                nc.id = new_id;
                nc.start = c.end();
                dup = Some(nc);
            } else {
                #[cfg(feature = "debug_record")]
                eprintln!("[dup] clip {cid} not found on track {tid}");
            }
        } else {
            #[cfg(feature = "debug_record")]
            eprintln!("[dup] track {tid} not found");
        }
        if let Some(nc) = dup {
            if let Some(t) = self.project.track_by_id_mut(tid) {
                t.clips.push(nc);
            }
        }
        self.selected_clip = Some(new_id);
        self.mark_graph_dirty();
    }

    pub fn delete_selected_clip(&mut self) {
        let (tid, cid) = match (self.selected_track, self.selected_clip) {
            (Some(t), Some(c)) => (t, c),
            _ => return,
        };
        if let Some(t) = self.project.track_by_id_mut(tid) {
            t.clips.retain(|c| c.id != cid);
        }
        self.selected_clip = None;
        self.mark_graph_dirty();
    }

    // ------------------------------------------------------------------
    // AI workflows
    // ------------------------------------------------------------------

    pub fn ai_clean_vocals(&mut self) {
        let opts = self.cleaner.options.clone();
        let mut started = 0;
        let track_ids: Vec<TrackId> = self
            .project
            .tracks
            .iter()
            .filter(|t| t.kind == TrackKind::Audio && (t.name.to_lowercase().contains("vocal") || t.name.to_lowercase().contains("voice")))
            .map(|t| t.id)
            .collect();
        let ids = if track_ids.is_empty() {
            // fall back: any track with audio
            self.project
                .tracks
                .iter()
                .filter(|t| t.clips.iter().any(|c| c.audio.is_some()))
                .map(|t| t.id)
                .take(1)
                .collect::<Vec<_>>()
        } else {
            track_ids
        };
        for tid in ids {
            let clip_ids: Vec<ClipId> = self
                .project
                .track_by_id(tid)
                .map(|t| t.clips.iter().filter(|c| c.audio.is_some()).map(|c| c.id).collect())
                .unwrap_or_default();
            for cid in clip_ids {
                let (audio, name) = {
                    let t = self.project.track_by_id(tid).unwrap();
                    let c = t.clips.iter().find(|c| c.id == cid).unwrap();
                    (c.audio.clone().unwrap(), t.name.clone())
                };
                let h = self.jobs.start_cleanup(tid, cid, name, audio, opts.clone(), self.project.sample_rate);
                self.active_jobs.push(("AI Vocal Cleanup".into(), h));
                started += 1;
            }
        }
        self.status = if started > 0 {
            format!("AI cleanup running on {started} clip(s)…")
        } else {
            "No audio clips found to clean".into()
        };
    }

    pub fn ai_mix_analyze(&mut self) {
        self.ai_mix.analyzing = true;
        self.ai_mix.applied = false;
        let project = self.project.clone();
        let handle = self.jobs.start_mix_analysis(project, self.project.sample_rate);
        self.active_jobs.push(("AI Mix Analysis".into(), handle));
    }

    pub fn ai_mix_apply(&mut self) {
        let suggestions = self.ai_mix.suggestions.clone();
        let n = suggestions.len();
        for s in suggestions {
            let mut eq_clone = s.apply_eq.clone();
            if let Some(eq) = eq_clone.as_mut() {
                eq.uid = self.project.alloc_id();
            }
            if let Some(t) = self.project.track_by_id_mut(s.track_id) {
                if let Some(v) = s.apply_volume_db {
                    t.volume_db = (t.volume_db + v).clamp(-60.0, 6.0);
                }
                if let Some(p) = s.apply_pan {
                    t.pan = p;
                }
                if let Some(send) = s.apply_reverb_send {
                    t.reverb_send = send;
                }
                if let Some(eq) = eq_clone {
                    t.fx.push(eq);
                }
            }
        }
        self.ai_mix.applied = true;
        self.mark_graph_dirty();
        self.status = format!("AI Mix applied {n} adjustments");
        self.toast(format!("AI Mix Assistant: {n} adjustments applied"));
    }

    pub fn toast(&mut self, msg: String) {
        self.toasts.push_back((msg, Instant::now()));
        if self.toasts.len() > 6 {
            self.toasts.pop_front();
        }
    }

    // ------------------------------------------------------------------
    // File workflows
    // ------------------------------------------------------------------

    pub fn new_project(&mut self) {
        let mut p = Project::new_empty("Untitled Session");
        p.add_track("AUDIO 1", TrackKind::Audio, [94, 234, 212, 255]);
        self.load_project_internal(p);
        self.status = "New project created".into();
    }

    pub fn load_demo(&mut self) {
        let p = aurora_engine::demo::build_demo_project();
        self.load_project_internal(p);
        self.status = "Aurora Session demo loaded".into();
    }

    pub fn load_project_internal(&mut self, p: Project) {
        // clear engine tracks
        let old = self.project.tracks.len();
        for slot in (0..old).rev() {
            self.send(Command::RemoveTrackSlot(slot));
        }
        self.project = p;
        self.selected_track = self.project.tracks.first().map(|t| t.id);
        self.selected_clip = None;
        self.playing = false;
        self.recording = false;
        self.send(Command::Stop);
        self.send(Command::SetPosition(0.0));
        self.mark_graph_dirty();
    }

    pub fn save_project(&mut self) {
        let dir = dirs_music();
        std::fs::create_dir_all(&dir).ok();
        let safe = self.project.name.replace([' ', '/'], "_");
        let path = dir.join(format!("{safe}.aurora"));
        match self.project.save_to_path(&path) {
            Ok(()) => {
                self.status = format!("Saved to {}", path.display());
                self.toast(format!("Project saved: {}", path.display()));
            }
            Err(e) => {
                self.status = format!("Save failed: {e}");
            }
        }
    }

    pub fn open_project(&mut self) {
        // headless-friendly: scan default music dir for latest .aurora
        let dir = dirs_music();
        let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|e| e.to_str()) == Some("aurora") {
                    let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                    if best.as_ref().map(|(t, _)| mtime > *t).unwrap_or(true) {
                        best = Some((mtime, p));
                    }
                }
            }
        }
        if let Some((_, p)) = best {
            match Project::load_from_path(&p) {
                Ok(proj) => {
                    self.load_project_internal(proj);
                    self.status = format!("Opened {}", p.display());
                    self.toast(format!("Project opened: {}", p.display()));
                }
                Err(e) => self.status = format!("Open failed: {e}"),
            }
        } else {
            self.status = "No saved projects found".into();
        }
    }

    pub fn import_audio(&mut self, path: &str) {
        let p = PathBuf::from(path);
        if !p.exists() {
            self.status = format!("File not found: {path}");
            return;
        }
        let name = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("import")
            .to_string();
        let h = self.jobs.start_decode(p, self.project.sample_rate, name);
        self.active_jobs.push(("Import".into(), h));
        self.status = "Importing audio…".into();
    }

    pub fn generate_stress(&mut self, n: usize) {
        let p = aurora_engine::demo::build_stress_project(n, 0.08);
        self.load_project_internal(p);
        self.stress_count = n;
        self.status = format!("Stress project generated: {n} tracks");
        self.toast(format!("Stress test: {n} tracks loaded"));
    }

    pub fn start_export(&mut self, dlg: &ExportDlg) {
        let project = self.project.clone();
        let dir = dlg.dir.clone();
        std::fs::create_dir_all(&dir).ok();
        let name = dlg.name.clone();
        let fmt = dlg.format;
        let sr = dlg.sample_rate;
        let (from, to) = if dlg.range_full {
            (0.0, self.project.duration().max(1.0))
        } else {
            (dlg.from, dlg.to.max(dlg.from + 0.5))
        };
        let h = if dlg.stems {
            let d = PathBuf::from(&dir).join(format!("{name}_stems"));
            self.jobs
                .start_stems(project, from, to, d, fmt, sr)
        } else {
            let ext = fmt.extension();
            let path = PathBuf::from(&dir).join(format!("{name}.{ext}"));
            self.jobs.start_bounce(project, from, to, path, fmt, sr)
        };
        self.active_jobs.push(("Export".into(), h));
        self.export_dlg = None;
        self.status = "Bouncing…".into();
    }

    // ------------------------------------------------------------------
    // Frame pump
    // ------------------------------------------------------------------

    pub fn pump_jobs(&mut self) {
        while let Ok((_kind, outcome)) = self.jobs.results_rx.try_recv() {
            match outcome {
                JobOutcome::CleanupDone {
                    track_id,
                    clip_id,
                    new_audio,
                    report,
                } => {
                    if let Some(t) = self.project.track_by_id_mut(track_id) {
                        if let Some(c) = t.clips.iter_mut().find(|c| c.id == clip_id) {
                            c.peaks = Some(aurora_engine::project::compute_peaks(&new_audio, 1400));
                            c.audio = Some(new_audio.clone());
                        }
                    }
                    let name = self
                        .project
                        .track_by_id(track_id)
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    self.cleaner.last_reports.push((name.clone(), report.clone()));
                    self.mark_graph_dirty();
                    self.status = format!(
                        "AI cleaned '{}': noise −{:.0} dB, {} clicks, {} breaths",
                        name,
                        report.noise_reduction_est_db,
                        report.clicks_fixed,
                        report.breaths_removed
                    );
                    self.toast(self.status.clone());
                }
                JobOutcome::BounceDone {
                    path,
                    duration_s,
                    peak_db,
                    lufs,
                } => {
                    self.last_export = Some(path.clone());
                    self.status = format!("Exported {} ({:.1}s, peak {:.1} dB, {:.1} LUFS)", path, duration_s, peak_db, lufs);
                    self.toast(self.status.clone());
                }
                JobOutcome::StemsDone { paths } => {
                    self.status = format!("Exported {} stems", paths.len());
                    self.toast(self.status.clone());
                    self.last_export = Some(paths.first().cloned().unwrap_or_default());
                }
                JobOutcome::DecodeDone { audio, suggested_name } => {
                    let name_for_status = suggested_name.clone();
                    let tid = match self.selected_track {
                        Some(id) if self.project.track_by_id(id).map(|t| t.kind) == Some(TrackKind::Audio) => id,
                        _ => {
                            let colors: [u8; 4] = [45, 212, 191, 255];
                            let i = self.project.tracks.len();
                            self.project
                                .add_track(&format!("AUDIO {}", i + 1), TrackKind::Audio, colors)
                                .id
                        }
                    };
                    let cid = self.project.alloc_id();
                    let clip = Clip::with_audio(cid, &suggested_name, self.engine_pos(), Arc::new(audio));
                    if let Some(t) = self.project.track_by_id_mut(tid) {
                        t.clips.push(clip);
                    }
                    self.mark_graph_dirty();
                    self.status = format!("Imported '{name_for_status}' onto timeline");
                    self.toast(self.status.clone());
                }
                JobOutcome::MixAnalyzed { text, suggestions } => {
                    self.ai_mix.suggestions = suggestions;
                    self.ai_mix.analyzed = true;
                    self.ai_mix.analyzing = false;
                    let n = self.ai_mix.suggestions.len() as f32;
                    self.ai_mix.confidence = (0.62 + n * 0.012).min(0.97);
                    self.status = format!("AI mix analysis complete: {} suggestions — {}", n as i32, text);
                    self.toast("AI Mix analysis complete".into());
                }
                JobOutcome::Failed { kind, error } => {
                    self.status = format!("{kind} failed: {error}");
                    self.toast(format!("{kind} failed: {error}"));
                    self.ai_mix.analyzing = false;
                }
            }
        }
        // drop finished handles
        self.active_jobs.retain(|(_, h)| h.percent() < 100);
    }

    pub fn pump_back_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.back_rx {
            while let Ok(ev) = rx.try_recv() {
                events.push(ev);
            }
        }
        for ev in events {
            match ev {
                    BackEvent::RecordedTake {
                        track_id,
                        position,
                        samples,
                    } => {
                        #[cfg(feature = "debug_record")]
                        eprintln!("[app] RecordedTake track={track_id} samples={}", samples.len());
                        let frames = samples.len() / 2;
                        let tid = track_id;
                        let cid = self.project.alloc_id();
                        let audio = Arc::new(AudioData {
                            samples,
                            channels: 2,
                            sample_rate: self.project.sample_rate,
                        });
                        if let Some(t) = self.project.track_by_id_mut(tid) {
                            let take_n = t.takes.len() + 1;
                            let clip = Clip::with_audio(
                                cid,
                                &format!("TAKE {take_n}"),
                                position,
                                audio,
                            );
                            t.clips.push(clip);
                            if !t.takes.iter().any(|x| x.take_id == 0) {
                                t.takes.push(TakeLane {
                                    name: "Take 1".into(),
                                    take_id: 0,
                                    color: t.color,
                                });
                            }
                            if !t.takes.iter().any(|x| x.take_id == take_n as u32 - 0) {
                                // ensure lane list includes the new take id
                                let id = (t.takes.len()) as u32;
                                t.takes.push(TakeLane {
                                    name: format!("Take {id}"),
                                    take_id: id,
                                    color: t.color,
                                });
                            }
                        }
                        self.mark_graph_dirty();
                        self.status = format!(
                            "Take recorded: {:.1}s on track",
                            frames as f64 / 48000.0
                        );
                        self.toast(self.status.clone());
                    }
                    BackEvent::Notice(m) => {
                        self.status = m;
                    }
                }
        }
    }

    pub fn update_ram(&mut self) {
        self.ram_mb = read_rss_mb();
    }
}

pub fn dirs_music() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(home).join("Music").join("Aurora");
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    PathBuf::from(".")
}

#[cfg(target_os = "linux")]
fn read_rss_mb() -> f32 {
    if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb: f32 = rest
                    .trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse()
                    .unwrap_or(0.0);
                return kb / 1024.0;
            }
        }
    }
    0.0
}

#[cfg(not(target_os = "linux"))]
fn read_rss_mb() -> f32 {
    0.0
}

pub struct AppOptions {
    pub empty: bool,
    pub stress: usize,
    pub shots_dir: Option<String>,
    pub autotest: bool,
}

impl Default for AppOptions {
    fn default() -> Self {
        Self {
            empty: false,
            stress: 0,
            shots_dir: None,
            autotest: false,
        }
    }
}

impl eframe::App for AuroraApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ---- frame pump ----
        self.sync_params();
        if self.graph_dirty {
            self.push_graph();
        }
        self.pump_back_events();
        self.pump_jobs();
        self.update_ram();

        // ---- panels ----
        self.draw_topbar(ctx);
        self.draw_browser(ctx);
        self.draw_inspector(ctx);
        self.draw_arranger(ctx);
        if self.mixer_open {
            self.draw_mixer(ctx);
        }
        self.draw_fx_windows(ctx);
        self.draw_piano_roll(ctx);
        self.draw_dialogs(ctx);
        self.draw_toasts(ctx);

        // ---- autotest driver ----
        if self.autotest.is_some() {
            let mut at = self.autotest.take().unwrap();
            at.step_pub(self, ctx);
            self.autotest = Some(at);
        }

        // ---- keyboard shortcuts ----
        if ctx.input(|i| i.key_pressed(egui::Key::Space) && !ctx.wants_keyboard_input()) {
            if self.playing {
                self.pause();
            } else {
                self.play();
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command_only()) {
            self.save_project();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::E) && i.modifiers.command_only()) {
            self.export_dlg = Some(ExportDlg {
                format: ExportFormat::Wav24,
                sample_rate: 48000,
                stems: false,
                dir: dirs_music().display().to_string(),
                name: self.project.name.replace(' ', "_"),
                range_full: true,
                from: 0.0,
                to: 10.0,
            });
        }
        if ctx.input(|i| i.key_pressed(egui::Key::L)) {
            self.toggle_loop();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Home)) {
            self.seek(0.0);
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}
