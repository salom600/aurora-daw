//! Offline render (bounce/export) — re-uses the exact realtime graph so the
//! exported file is precisely what you hear.

use crate::engine::EngineRT;
use crate::io::ExportFormat;
use crate::project::Project;
use std::path::Path;

pub struct BounceResult {
    pub path: String,
    pub frames: usize,
    pub duration_s: f64,
    pub peak_db: f32,
    pub lufs: f32,
}

/// Render `project` from `start` to `end` (seconds) at `sample_rate`,
/// calling `progress(0..100)` periodically. Returns interleaved stereo.
pub fn render_range(
    project: &Project,
    start: f64,
    end: f64,
    sample_rate: u32,
    mut progress: impl FnMut(u32),
) -> Result<Vec<f32>, String> {
    let mut engine = EngineRT::offline(sample_rate as f32);
    engine.load_project(project);
    engine.pos = start;

    let duration = (end - start).max(0.01);
    let total_frames = (duration * sample_rate as f64) as usize;
    let mut out = Vec::with_capacity(total_frames * 2);
    let block = crate::engine::BLOCK;
    let mut io = vec![0.0f32; block * 2];
    let mut done = 0usize;
    let total_blocks = (total_frames / block).max(1);
    let mut bi = 0usize;
    while done < total_frames {
        let frames = block.min(total_frames - done);
        io[..frames * 2].fill(0.0);
        engine.process_block(&mut io[..frames * 2], frames);
        out.extend_from_slice(&io[..frames * 2]);
        done += frames;
        bi += 1;
        if bi % 32 == 0 {
            progress(((done as f64 / total_frames.max(1) as f64) * 100.0) as u32);
        }
    }
    progress(100);
    Ok(out)
}

pub fn bounce(
    project: &Project,
    start: f64,
    end: f64,
    path: &Path,
    format: ExportFormat,
    sample_rate: u32,
    mut progress: impl FnMut(u32),
) -> Result<BounceResult, String> {
    let data = render_range(project, start, end, sample_rate, &mut progress)?;
    match format {
        ExportFormat::Mp3 => crate::io::encode_mp3(path, &data, sample_rate)?,
        _ => crate::io::encode_wav(path, &data, sample_rate, format)?,
    }
    // measure
    let frames = data.len() / 2;
    let mut peak = 0.0f32;
    let mut sum = 0.0f64;
    for chunk in data.chunks_exact(2) {
        let (l, r) = (chunk[0], chunk[1]);
        peak = peak.max(l.abs()).max(r.abs());
        sum += (l * l + r * r) as f64;
    }
    let rms = (sum / frames.max(1) as f64).sqrt() as f32;
    let lufs = -0.691 + 20.0 * rms.max(1e-8).log10();
    Ok(BounceResult {
        path: path.display().to_string(),
        frames,
        duration_s: frames as f64 / sample_rate as f64,
        peak_db: 20.0 * peak.max(1e-8).log10(),
        lufs,
    })
}

/// Export each track (post-fx, pre-master) to its own WAV stem.
pub fn bounce_stems(
    project: &Project,
    start: f64,
    end: f64,
    dir: &Path,
    format: ExportFormat,
    sample_rate: u32,
    mut progress: impl FnMut(u32),
) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    let total = project.tracks.len().max(1);
    for (i, track) in project.tracks.iter().enumerate() {
        let mut solo_proj = solo_project(project, track.id);
        // master silent: stems are pre-master
        solo_proj.master_fx.clear();
        solo_proj.master_volume_db = 0.0;
        let data = render_range(&solo_proj, start, end, sample_rate, |_| {})?;
        let safe = track.name.replace(['/', '\\', ':'], "-");
        let p = dir.join(format!("{:02}_{}.{}", i + 1, safe, format.extension()));
        match format {
            ExportFormat::Mp3 => crate::io::encode_mp3(&p, &data, sample_rate)?,
            _ => crate::io::encode_wav(&p, &data, sample_rate, format)?,
        }
        written.push(p.display().to_string());
        progress(((i + 1) * 100 / total) as u32);
    }
    Ok(written)
}

/// Solo out one track (mute everything else) for stem rendering.
fn solo_project(project: &Project, keep: u64) -> Project {
    let mut p = project.clone();
    for t in p.tracks.iter_mut() {
        t.mute = t.id != keep;
        t.solo = false;
    }
    p
}
