//! Audio file I/O — decode (WAV/MP3/FLAC/OGG) + encode (WAV, MP3).

use crate::project::{AudioData, ENGINE_SAMPLE_RATE};
use std::path::Path;

/// Decode any supported file to interleaved stereo f32 at `target_rate`.
pub fn decode_file(path: &Path, target_rate: u32) -> Result<AudioData, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if ext == "wav" {
        if let Ok(a) = decode_wav(path, target_rate) {
            return Ok(a);
        }
    }
    decode_symphonia(path, target_rate)
}

fn decode_wav(path: &Path, target_rate: u32) -> Result<AudioData, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    let channels = spec.channels as u32;
    let sr = spec.sample_rate;
    let mut samples = Vec::with_capacity(reader.duration() as usize * 2);
    let max_val: f32 = match spec.bits_per_sample {
        8 => 127.0,
        16 => 32767.0,
        24 => 8388607.0,
        32 => 2147483647.0,
        _ => 32767.0,
    };
    let frames_total = reader.duration() as usize;
    let mut frame: Vec<f32> = vec![0.0; channels as usize];
    let mut fi = 0usize;
    let iter: Box<dyn Iterator<Item = Result<f32, hound::Error>>> = match spec.sample_format {
        hound::SampleFormat::Float => Box::new(reader.samples::<f32>().map(|s| s)),
        hound::SampleFormat::Int => Box::new(reader.samples::<i32>().map(|s| {
            s.map(|v| v as f32 / max_val)
        })),
    };
    for s in iter {
        let v = s.map_err(|e| e.to_string())?;
        frame[fi] = v;
        fi += 1;
        if fi == channels as usize {
            let l = *frame.get(0).unwrap_or(&0.0);
            let r = *frame.get(1).unwrap_or(&l);
            samples.push(l);
            samples.push(r);
            fi = 0;
        }
    }
    let mut data = AudioData {
        samples,
        channels: 2,
        sample_rate: sr,
    };
    let _ = frames_total;
    resample_if_needed(&mut data, target_rate);
    Ok(data)
}

fn decode_symphonia(path: &Path, target_rate: u32) -> Result<AudioData, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let src = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(e);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| e.to_string())?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or("no audio track")?
        .clone();
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;

    let mut out: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(e.to_string()),
        };
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let mut sbuf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                sbuf.copy_interleaved_ref(decoded);
                let ch = spec.channels.count();
                let s = sbuf.samples();
                match ch {
                    1 => out.extend(s.iter().flat_map(|v| [*v, *v])),
                    2 => out.extend_from_slice(s),
                    n => out.extend(s.chunks_exact(n).flat_map(|c| {
                        let l = c[0];
                        let r = c.get(1).copied().unwrap_or(l);
                        [l, r]
                    })),
                }
            }
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    let mut data = AudioData {
        samples: out,
        channels: 2,
        sample_rate,
    };
    resample_if_needed(&mut data, target_rate);
    Ok(data)
}

pub fn resample_if_needed(data: &mut AudioData, target_rate: u32) {
    if data.sample_rate == target_rate || data.samples.is_empty() {
        return;
    }
    let ratio = target_rate as f64 / data.sample_rate as f64;
    let n_out = ((data.samples.len() as f64) * ratio) as usize;
    let mut out = Vec::with_capacity(n_out);
    let ch = data.channels.max(1) as usize;
    for i in 0..n_out / 2 {
        let src = i as f64 / ratio;
        let i0 = (src as usize).min(data.frames().saturating_sub(1));
        let i1 = (i0 + 1).min(data.frames().saturating_sub(1));
        let f = (src - i0 as f64) as f32;
        let base0 = i0 * ch;
        let base1 = i1 * ch;
        for c in 0..2 {
            let a = data.samples.get(base0 + c).copied().unwrap_or(0.0);
            let b = data.samples.get(base1 + c).copied().unwrap_or(0.0);
            out.push(a + (b - a) * f);
        }
    }
    data.samples = out;
    data.sample_rate = target_rate;
}

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExportFormat {
    Wav16,
    Wav24,
    Wav32F,
    Mp3,
}

impl ExportFormat {
    pub fn name(&self) -> &'static str {
        match self {
            ExportFormat::Wav16 => "WAV 16-bit",
            ExportFormat::Wav24 => "WAV 24-bit",
            ExportFormat::Wav32F => "WAV 32-bit float",
            ExportFormat::Mp3 => "MP3 320 kbps",
        }
    }
    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Mp3 => "mp3",
            _ => "wav",
        }
    }
}

pub fn encode_wav(
    path: &Path,
    interleaved: &[f32],
    sample_rate: u32,
    fmt: ExportFormat,
) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: match fmt {
            ExportFormat::Wav16 => 16,
            ExportFormat::Wav24 => 24,
            ExportFormat::Wav32F => 32,
            _ => 24,
        },
        sample_format: match fmt {
            ExportFormat::Wav32F => hound::SampleFormat::Float,
            _ => hound::SampleFormat::Int,
        },
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    match fmt {
        ExportFormat::Wav32F => {
            for s in interleaved {
                w.write_sample(*s).map_err(|e| e.to_string())?;
            }
        }
        ExportFormat::Wav24 => {
            for s in interleaved {
                let v = (s.clamp(-1.0, 1.0) * 8388607.0) as i32;
                w.write_sample(v).map_err(|e| e.to_string())?;
            }
        }
        _ => {
            for s in interleaved {
                let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
                w.write_sample(v).map_err(|e| e.to_string())?;
            }
        }
    }
    w.finalize().map_err(|e| e.to_string())
}

pub fn encode_mp3(path: &Path, interleaved: &[f32], sample_rate: u32) -> Result<(), String> {
    use mp3lame_encoder::{Builder, InterleavedPcm};
    let mut builder = Builder::new().ok_or("lame init failed")?;
    builder.set_sample_rate(sample_rate);
    builder.set_brate(mp3lame_encoder::Bitrate::Kbps320);
    builder.set_num_channels(2).map_err(|e| e.to_string())?;
    let mut enc = builder.build().map_err(|e| e.to_string())?;
    let frames = interleaved.len() / 2;
    let mut pcm: Vec<i16> = Vec::with_capacity(frames * 2);
    for f in 0..frames {
        pcm.push((interleaved[f * 2].clamp(-1.0, 1.0) * 32767.0) as i16);
        pcm.push((interleaved[f * 2 + 1].clamp(-1.0, 1.0) * 32767.0) as i16);
    }
    let mut mp3_out = Vec::with_capacity(frames * 2 / 10 + 4096);
    let mut buf: Vec<core::mem::MaybeUninit<u8>> =
        vec![core::mem::MaybeUninit::new(0); pcm.len() * 5 / 4 + 7200];
    let written = enc
        .encode(InterleavedPcm(&pcm), &mut buf)
        .map_err(|e| e.to_string())?;
    mp3_out.extend_from_slice(unsafe {
        std::slice::from_raw_parts(buf.as_ptr() as *const u8, written)
    });
    let written = enc.flush::<mp3lame_encoder::FlushNoGap>(&mut buf).map_err(|e| e.to_string())?;
    mp3_out.extend_from_slice(unsafe {
        std::slice::from_raw_parts(buf.as_ptr() as *const u8, written)
    });
    std::fs::write(path, mp3_out).map_err(|e| e.to_string())
}

pub fn default_export_name(project: &str) -> String {
    format!(
        "{}_mix_{}",
        project.replace(' ', "_"),
        chrono_stamp()
    )
}

fn chrono_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{t:x}")
}

pub const ENGINE_RATE: u32 = ENGINE_SAMPLE_RATE;
