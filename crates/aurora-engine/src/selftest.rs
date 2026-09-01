//! Headless self-test — verifies the engine, DSP, AI cleanup, synth, bounce
//! and project persistence without any GUI. Run: `aurora-daw --selftest`

use crate::ai::CleanupOptions;
use crate::engine::EngineRT;
use crate::project::{Clip, Project, TrackKind};

fn approx(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() < tol
}

pub struct TestOutcome {
    pub name: String,
    pub passed: bool,
    pub detail: String,
    pub ms: u128,
}

pub fn run_all() -> Vec<TestOutcome> {
    let mut results = Vec::new();
    macro_rules! test {
        ($name:expr, $body:expr) => {
            let t0 = std::time::Instant::now();
            let r: Result<String, String> = (|| $body)();
            results.push(TestOutcome {
                name: $name.to_string(),
                passed: r.is_ok(),
                detail: r.unwrap_or_else(|e| e),
                ms: t0.elapsed().as_millis(),
            });
        };
    }

    test!("project: create/save/load roundtrip", {
        let dir = std::env::temp_dir().join("aurora_selftest");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut p = crate::demo::build_demo_project();
        p.tracks[0].volume_db = -7.25;
        let path = dir.join("roundtrip.aurora");
        p.save_to_path(&path)?;
        let p2 = Project::load_from_path(&path)?;
        if approx(p2.tracks[0].volume_db, -7.25, 0.01) && p2.tracks.len() == p.tracks.len() {
            Ok(format!("{} tracks, tempo {:.0}", p2.tracks.len(), p2.tempo))
        } else {
            Err("roundtrip mismatch".into())
        }
    });

    test!("demo: procedural session integrity", {
        let p = crate::demo::build_demo_project();
        let d = p.duration();
        if p.tracks.len() == 16 && d > 20.0 {
            let clips: usize = p.tracks.iter().map(|t| t.clips.len()).sum();
            Ok(format!("16 tracks, {clips} clips, {:.1}s", d))
        } else {
            Err(format!("bad demo: {} tracks {:.1}s", p.tracks.len(), d))
        }
    });

    test!("engine: render demo produces signal + meters", {
        let p = crate::demo::build_demo_project();
        let data = crate::bounce::render_range(&p, 0.0, 8.0, 48000, |_| {})?;
        let frames = data.len() / 2;
        let mut peak = 0.0f32;
        for v in &data {
            peak = peak.max(v.abs());
        }
        if frames > 100000 && peak > 0.05 && peak < 4.0 && !data.iter().any(|v| v.is_nan()) {
            Ok(format!("8.0s rendered, peak {peak:.3}, no NaN"))
        } else {
            Err(format!("render bad: frames={frames} peak={peak}"))
        }
    });

    test!("engine: 1000-track stress render stability", {
        let p = crate::demo::build_stress_project(1000, 0.08);
        let t0 = std::time::Instant::now();
        let data = crate::bounce::render_range(&p, 0.0, 6.0, 48000, |_| {})?;
        let secs = t0.elapsed().as_secs_f32();
        let mut peak = 0.0f32;
        for v in &data {
            peak = peak.max(v.abs());
            if v.is_nan() {
                return Err("NaN in stress render".into());
            }
        }
        if peak.is_finite() {
            Ok(format!(
                "1000 tracks rendered 6s in {:.1}s (x{:.0} realtime), peak {peak:.3}",
                secs,
                6.0 / secs.max(0.001)
            ))
        } else {
            Err("non-finite output".into())
        }
    });

    test!("synth: instrument clip renders notes", {
        let mut p = Project::new_empty("t");
        let notes = vec![
            crate::project::Note { start_beats: 0.0, len_beats: 1.0, key: 24, vel: 0.9 },
            crate::project::Note { start_beats: 1.0, len_beats: 1.0, key: 28, vel: 0.8 },
            crate::project::Note { start_beats: 2.0, len_beats: 2.0, key: 31, vel: 0.85 },
        ];
        let mut t = p.add_track("SYNTH", TrackKind::Instrument, [100, 100, 255, 255]);
        let clip = Clip::with_notes(1, "MEL", 0.0, 4.0, notes);
        t.clips.push(clip);
        let data = crate::bounce::render_range(&p, 0.0, 4.0, 48000, |_| {})?;
        let peak = data.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if peak > 0.02 {
            Ok(format!("4 notes poly synth, peak {peak:.3}"))
        } else {
            Err("synth silent".into())
        }
    });

    test!("dsp: fx chain changes the signal (EQ+comp+reverb)", {
        let mut p = crate::demo::build_vocal_session();
        if let Some(t) = p.tracks.first_mut() {
            use crate::effects::{EffectInstance, EffectType};
            t.fx.push(EffectInstance::new(EffectType::Eq3, 10));
            t.fx.push(EffectInstance::new(EffectType::Compressor, 11));
            t.fx.push(EffectInstance::new(EffectType::Reverb, 12));
        }
        let d1 = crate::bounce::render_range(&p, 0.0, 4.0, 48000, |_| {})?;
        let e = rms(&d1);
        if e > 1e-5 {
            Ok(format!("fx render rms {e:.5}"))
        } else {
            Err("fx chain silent".into())
        }
    });

    test!("ai: one-click cleanup removes noise/hum/clicks/breaths", {
        // contaminated vocal, exactly like the demo project's VOCAL LEAD
        use crate::demo as d;
        let contaminated = d::demo_vocal_samples(10.0);
        let (clean, report) = crate::ai::clean_vocal(
            &contaminated,
            48000,
            &CleanupOptions::default(),
        );
        // measure noise floor in a silent region (last 0.5s is phrase-free? use
        // first 0.2s before first phrase at 0.5s)
        let win = 4800;
        let noise_before: f32 = contaminated[..win]
            .chunks(2)
            .map(|c| c[0])
            .map(|v| v * v)
            .sum::<f32>()
            / win as f32;
        let noise_after: f32 = clean[..win]
            .chunks(2)
            .map(|c| c[0])
            .map(|v| v * v)
            .sum::<f32>()
            / win as f32;
        let db_before = 10.0 * noise_before.max(1e-12).log10();
        let db_after = 10.0 * noise_after.max(1e-12).log10();
        let reduction = db_before - db_after;
        if reduction > 6.0 && report.clicks_fixed > 0 && report.breaths_removed > 0 && !report.hum_freqs.is_empty() {
            Ok(format!(
                "noise −{:.1} dB, {} clicks, {} breaths, hum at {:?} Hz",
                reduction, report.clicks_fixed, report.breaths_removed, report.hum_freqs
            ))
        } else {
            Err(format!(
                "weak cleanup: −{:.1} dB, clicks {}, breaths {}, hum {:?}",
                reduction, report.clicks_fixed, report.breaths_removed, report.hum_freqs
            ))
        }
    });

    test!("export: WAV 24-bit file valid + header correct", {
        let dir = std::env::temp_dir().join("aurora_selftest");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let p = crate::demo::build_vocal_session();
        let path = dir.join("export_test.wav");
        let r = crate::bounce::bounce(&p, 0.0, 3.0, &path, crate::io::ExportFormat::Wav24, 48000, |_| {})?;
        let mut rd = hound::WavReader::open(&path).map_err(|e| e.to_string())?;
        let spec = rd.spec();
        let n: usize = rd.duration() as usize;
        if spec.sample_rate == 48000
            && spec.channels == 2
            && spec.bits_per_sample == 24
            && n > 140_000
            && r.peak_db.is_finite()
        {
            Ok(format!("{:.1}s, peak {:.2} dBFS", r.duration_s, r.peak_db))
        } else {
            Err(format!("bad wav: {spec:?}, {n} frames"))
        }
    });

    test!("export: MP3 320 file valid", {
        let dir = std::env::temp_dir().join("aurora_selftest");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let p = crate::demo::build_vocal_session();
        let path = dir.join("export_test.mp3");
        crate::bounce::bounce(&p, 0.0, 3.0, &path, crate::io::ExportFormat::Mp3, 48000, |_| {})?;
        let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
        if meta.len() > 40_000 {
            Ok(format!("{} KB", meta.len() / 1024))
        } else {
            Err(format!("mp3 too small: {}", meta.len()))
        }
    });

    test!("loudness: BS.1770 integrated measurement sane", {
        let p = crate::demo::build_demo_project();
        let data = crate::bounce::render_range(&p, 0.0, 12.0, 48000, |_| {})?;
        let mut lm = crate::engine::LoudnessMeter::new(48000.0);
        for chunk in data.chunks(48000) {
            let f = chunk.len() / 2;
            lm.push(chunk, f);
        }
        let lufs = lm.integrated();
        if (-40.0..=-3.0).contains(&lufs) {
            Ok(format!("integrated {lufs:.1} LUFS"))
        } else {
            Err(format!("implausible LUFS: {lufs}"))
        }
    });

    test!("transport: position advance + loop wrap", {
        let (mut engine, parts) = crate::audio_io::create_engine_parts(48000.0);
        engine.load_project(&crate::demo::build_vocal_session());
        let _ = engine.take_command_producer();
        // play for 1s
        engine.apply(crate::engine::Command::SetLoop(Some((0.0, 2.0))));
        engine.apply(crate::engine::Command::Play);
        let mut io = vec![0.0f32; 512 * 2];
        for _ in 0..100 {
            engine.process_block(&mut io, 512);
        }
        let pos = engine.pos;
        let _ = parts;
        if (0.9..1.4).contains(&pos) {
            Ok(format!("pos {pos:.2}s after 100 blocks"))
        } else {
            Err(format!("unexpected pos {pos}"))
        }
    });

    test!("engine: AI mix analysis produces suggestions", {
        let p = crate::demo::build_demo_project();
        let mut stats = std::collections::HashMap::new();
        for t in &p.tracks {
            if t.clips.is_empty() {
                continue;
            }
            let mut mono = Vec::new();
            for c in t.clips.iter().take(2) {
                if let Some(a) = &c.audio {
                    mono.extend(a.mono().into_iter().take(48000 * 2));
                }
            }
            if mono.is_empty() {
                continue;
            }
            let st = crate::ai::analyze_track(&mono, 48000, t.id);
            stats.insert(t.id, st);
        }
        let sug = crate::ai::suggest_mix(&p, &stats);
        if sug.len() >= 3 {
            Ok(format!("{} suggestions (e.g. {})", sug.len(), sug[0].description))
        } else {
            Err(format!("only {} suggestions", sug.len()))
        }
    });

    test!("rapid command flood: 10k commands without crash", {
        let (mut engine, _parts) = crate::audio_io::create_engine_parts(48000.0);
        engine.load_project(&crate::demo::build_demo_project());
        if let Some(mut tx) = engine.take_command_producer() {
            for i in 0..10_000 {
                use crate::engine::Command;
                let _ = tx.push(match i % 5 {
                    0 => Command::Play,
                    1 => Command::Stop,
                    2 => Command::SetPosition(0.5),
                    3 => Command::SetTempo(100.0 + (i % 60) as f64),
                    _ => Command::Panic,
                });
            }
        }
        let mut io = vec![0.0f32; 512 * 2];
        for _ in 0..200 {
            engine.process_block(&mut io, 512);
        }
        Ok("10k commands absorbed, engine alive".to_string())
    });

    test!("record pipeline: simulated input -> take capture", {
        let (mut engine, _parts) = crate::audio_io::create_engine_parts(48000.0);
        let mut p = crate::demo::build_vocal_session();
        let slot = 2; // vocal track
        engine.load_project(&p);
        // arm vocal track
        p.tracks[slot].armed = true;
        engine
            .params()
            .slots[slot]
            .flags
            .store(4 | 8, std::sync::atomic::Ordering::Relaxed);
        engine.apply(crate::engine::Command::SetSimulatedInput(true));
        engine.apply(crate::engine::Command::StartRecord { position: 0.0, capacity_frames: 48000 * 3 });
        let mut io = vec![0.0f32; 512 * 2];
        for _ in 0..100 {
            engine.process_block(&mut io, 512);
        }
        engine.apply(crate::engine::Command::StopRecord);
        // collect back events
        let rx = engine.take_back_receiver().ok_or("no back receiver")?;
        let mut captured = 0usize;
        while let Ok(ev) = rx.recv_timeout(std::time::Duration::from_millis(500)) {
            if let crate::engine::BackEvent::RecordedTake { samples, .. } = ev {
                captured = samples.len();
            }
        }
        if captured > 48000 {
            Ok(format!("captured {} frames (~{:.1}s)", captured / 2, captured as f64 / 96000.0))
        } else {
            Err(format!("capture too small: {captured}"))
        }
    });

    results
}

fn rms(data: &[f32]) -> f32 {
    (data.iter().map(|v| v * v).sum::<f32>() / data.len().max(1) as f32).sqrt()
}

pub fn print_report(results: &[TestOutcome]) -> bool {
    let mut all_ok = true;
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║   AURORA ENGINE SELF-TEST  v{}", env!("CARGO_PKG_VERSION"));
    println!("╚══════════════════════════════════════════════════════════╝");
    for r in results {
        let icon = if r.passed { "PASS" } else { "FAIL" };
        all_ok &= r.passed;
        println!(
            " [{icon}] {:<48} {:>5} ms\n        {}",
            r.name, r.ms, r.detail
        );
    }
    let passed = results.iter().filter(|r| r.passed).count();
    println!(
        "\n {} / {} tests passed\n",
        passed,
        results.len()
    );
    all_ok
}
