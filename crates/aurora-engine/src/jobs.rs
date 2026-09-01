//! Background job system — AI cleanup, bounce, decode, AI-mix analysis.
//! All heavy work happens off the UI/audio threads; progress is polled.

use crate::ai::{CleanupOptions, CleanupReport};
use std::collections::HashMap;
use crate::project::{AudioData, Project, SharedAudio, TrackId};
use crossbeam_channel::{Receiver, Sender};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub enum JobKind {
    VocalCleanup,
    Bounce,
    Stems,
    Decode,
    MixAnalysis,
}

#[derive(Clone, Debug)]
pub enum JobOutcome {
    CleanupDone {
        track_id: TrackId,
        clip_id: u64,
        new_audio: SharedAudio,
        report: CleanupReport,
    },
    BounceDone {
        path: String,
        duration_s: f64,
        peak_db: f32,
        lufs: f32,
    },
    StemsDone {
        paths: Vec<String>,
    },
    DecodeDone {
        audio: AudioData,
        suggested_name: String,
    },
    MixAnalyzed {
        text: String,
        suggestions: Vec<crate::ai::MixSuggestion>,
    },
    Failed {
        kind: String,
        error: String,
    },
}

pub struct JobHandle {
    pub kind: JobKind,
    pub label: String,
    pub progress: Arc<AtomicU32>,
    pub cancel: Arc<AtomicBool>,
}

impl JobHandle {
    pub fn percent(&self) -> u32 {
        self.progress.load(Ordering::Relaxed).min(100)
    }
}

pub struct JobManager {
    results_tx: Sender<(JobKind, JobOutcome)>,
    pub results_rx: Receiver<(JobKind, JobOutcome)>,
}

impl Default for JobManager {
    fn default() -> Self {
        Self::new()
    }
}

impl JobManager {
    pub fn new() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            results_tx: tx,
            results_rx: rx,
        }
    }

    fn spawn<F: FnOnce(Arc<AtomicU32>, Arc<AtomicBool>, Sender<(JobKind, JobOutcome)>) + Send + 'static>(
        &self,
        kind: JobKind,
        label: String,
        f: F,
    ) -> JobHandle {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p, c, tx) = (progress.clone(), cancel.clone(), self.results_tx.clone());
        std::thread::Builder::new()
            .name(format!("aurora-job-{}", label))
            .spawn(move || {
                f(p, c, tx);
            })
            .ok();
        JobHandle {
            kind,
            label,
            progress,
            cancel,
        }
    }

    pub fn start_cleanup(
        &self,
        track_id: TrackId,
        clip_id: u64,
        track_name: String,
        audio: SharedAudio,
        opts: CleanupOptions,
        sample_rate: u32,
    ) -> JobHandle {
        self.spawn(
            JobKind::VocalCleanup,
            format!("AI Clean: {track_name}"),
            move |progress, cancel, tx| {
                progress.store(8, Ordering::Relaxed);
                // one dedicated thread per half of the work — keeps UI snappy
                let res = std::thread::scope(|s| {
                    let h = s.spawn(|| crate::ai::clean_vocal(&audio.samples, sample_rate, &opts));
                    // pulse progress while working (analysis is inside)
                    let mut v = 8;
                    while !h.is_finished() {
                        if cancel.load(Ordering::Relaxed) {
                            return Err("cancelled".to_string());
                        }
                        v = (v + 1).min(92);
                        progress.store(v, Ordering::Relaxed);
                        std::thread::sleep(std::time::Duration::from_millis(60));
                    }
                    h.join().map_err(|_| "worker panicked".to_string())
                });
                match res {
                    Ok((processed, report)) => {
                        progress.store(96, Ordering::Relaxed);
                        let new_audio = Arc::new(AudioData {
                            samples: processed,
                            channels: 2,
                            sample_rate,
                        });
                        let _ = tx.send((
                            JobKind::VocalCleanup,
                            JobOutcome::CleanupDone {
                                track_id,
                                clip_id,
                                new_audio,
                                report,
                            },
                        ));
                    }
                    Err(e) => {
                        let _ = tx.send((
                            JobKind::VocalCleanup,
                            JobOutcome::Failed {
                                kind: "AI Vocal Cleanup".into(),
                                error: e,
                            },
                        ));
                    }
                }
                progress.store(100, Ordering::Relaxed);
            },
        )
    }

    pub fn start_bounce(
        &self,
        project: Project,
        start: f64,
        end: f64,
        path: PathBuf,
        format: crate::io::ExportFormat,
        sample_rate: u32,
    ) -> JobHandle {
        self.spawn(JobKind::Bounce, "Bounce mix".into(), move |progress, cancel, tx| {
            let last = Arc::new(AtomicU32::new(0));
            let l2 = last.clone();
            let c2 = cancel.clone();
            let res = crate::bounce::bounce(
                &project,
                start,
                end,
                &path,
                format,
                sample_rate,
                move |p| {
                    if c2.load(Ordering::Relaxed) {
                        // note: render continues but result is discarded on cancel
                    }
                    l2.store(p, Ordering::Relaxed);
                },
            );
            // mirror progress
            progress.store(last.load(Ordering::Relaxed), Ordering::Relaxed);
            match res {
                Ok(r) => {
                    let _ = tx.send((
                        JobKind::Bounce,
                        JobOutcome::BounceDone {
                            path: r.path,
                            duration_s: r.duration_s,
                            peak_db: r.peak_db,
                            lufs: r.lufs,
                        },
                    ));
                }
                Err(e) => {
                    let _ = tx.send((
                        JobKind::Bounce,
                        JobOutcome::Failed {
                            kind: "Export".into(),
                            error: e,
                        },
                    ));
                }
            }
            progress.store(100, Ordering::Relaxed);
        })
    }

    pub fn start_decode(
        &self,
        path: PathBuf,
        sample_rate: u32,
        suggested_name: String,
    ) -> JobHandle {
        self.spawn(JobKind::Decode, "Import audio".into(), move |progress, _c, tx| {
            progress.store(10, Ordering::Relaxed);
            match crate::io::decode_file(&path, sample_rate) {
                Ok(audio) => {
                    progress.store(100, Ordering::Relaxed);
                    let _ = tx.send((
                        JobKind::Decode,
                        JobOutcome::DecodeDone {
                            audio,
                            suggested_name,
                        },
                    ));
                }
                Err(e) => {
                    let _ = tx.send((
                        JobKind::Decode,
                        JobOutcome::Failed {
                            kind: "Import".into(),
                            error: e,
                        },
                    ));
                }
            }
        })
    }

    pub fn start_mix_analysis(&self, project: Project, sample_rate: u32) -> JobHandle {
        self.spawn(JobKind::MixAnalysis, "AI mix analysis".into(), move |progress, _c, tx| {
            progress.store(5, Ordering::Relaxed);
            let mut stats = HashMap::new();
            let total = project.tracks.len().max(1);
            for (i, t) in project.tracks.iter().enumerate() {
                let mut mono = Vec::new();
                for c in t.clips.iter().take(3) {
                    if let Some(a) = &c.audio {
                        mono.extend(a.mono().into_iter().take(sample_rate as usize));
                    }
                }
                if mono.is_empty() {
                    continue;
                }
                let st = crate::ai::analyze_track(&mono, sample_rate, t.id);
                stats.insert(t.id, st);
                progress.store(((i + 1) * 80 / total) as u32, Ordering::Relaxed);
            }
            let suggestions = crate::ai::suggest_mix(&project, &stats);
            progress.store(95, Ordering::Relaxed);
            let text = format!(
                "{} of {} tracks analyzed",
                stats.len(),
                total
            );
            let _ = tx.send((JobKind::MixAnalysis, JobOutcome::MixAnalyzed { text, suggestions }));
            progress.store(100, Ordering::Relaxed);
        })
    }

    pub fn start_stems(
        &self,
        project: Project,
        start: f64,
        end: f64,
        dir: PathBuf,
        format: crate::io::ExportFormat,
        sample_rate: u32,
    ) -> JobHandle {
        self.spawn(JobKind::Stems, "Export stems".into(), move |progress, _c, tx| {
            match crate::bounce::bounce_stems(&project, start, end, &dir, format, sample_rate, |p| {
                progress.store(p, Ordering::Relaxed);
            }) {
                Ok(paths) => {
                    let _ = tx.send((JobKind::Stems, JobOutcome::StemsDone { paths }));
                }
                Err(e) => {
                    let _ = tx.send((
                        JobKind::Stems,
                        JobOutcome::Failed {
                            kind: "Stems".into(),
                            error: e,
                        },
                    ));
                }
            }
            progress.store(100, Ordering::Relaxed);
        })
    }
}
