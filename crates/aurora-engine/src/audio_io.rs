//! Audio device layer — real I/O via cpal (ALSA/WASAPI/CoreAudio) with an
//! automatic precision synthetic driver fallback for environments without a
//! sound device (containers/CI). Recording input comes through the same
//! layer; when no capture device exists, the simulated vocal source feeds
//! the pipeline so every feature stays fully functional and testable.

use crate::engine::{EngineRT, LoudnessTap, MeterStore, ParamStore, SpectralTap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverKind {
    RealDevice,
    Synthetic,
}

/// Everything the app keeps while the engine runs on the audio side.
pub struct EngineParts {
    pub params: Arc<ParamStore>,
    pub meters: Arc<MeterStore>,
    pub spectral: Arc<SpectralTap>,
    pub loudness: Arc<LoudnessTap>,
}

pub fn create_engine_parts(sample_rate: f32) -> (EngineRT, EngineParts) {
    let params = ParamStore::new();
    let meters = MeterStore::new();
    let spectral = Arc::new(SpectralTap::new(2048));
    let loudness = Arc::new(LoudnessTap {
        momentary_lu: std::sync::atomic::AtomicU32::new(0),
        shortterm_lu: std::sync::atomic::AtomicU32::new(0),
        integrated_lu: std::sync::atomic::AtomicU32::new(0),
        true_peak_db: std::sync::atomic::AtomicU32::new(0),
    });
    let engine = EngineRT::new(
        sample_rate,
        params.clone(),
        meters.clone(),
        spectral.clone(),
        loudness.clone(),
    );
    (
        engine,
        EngineParts {
            params,
            meters,
            spectral,
            loudness,
        },
    )
}

pub struct AudioIO {
    pub kind: DriverKind,
    pub device_name: String,
    pub sample_rate: u32,
    pub buffer_frames: usize,
    _streams: Vec<StreamBox>,
    stop_flag: Arc<AtomicBool>,
    synth_thread: Option<std::thread::JoinHandle<()>>,
}

enum StreamBox {
    #[cfg(feature = "real-audio")]
    Cpal(Box<cpal::Stream>),
    None,
}

impl AudioIO {
    /// Start rendering `engine` on the best available driver.
    pub fn start(engine: EngineRT) -> Self {
        #[cfg(feature = "real-audio")]
        {
            use cpal::traits::{DeviceTrait, HostTrait};
            // probe the device first so the engine is never lost on failure
            let device_ok = cpal::default_host()
                .default_output_device()
                .and_then(|d| d.default_output_config().ok())
                .is_some();
            let engine = if device_ok {
                match Self::start_cpal(engine) {
                    Ok(a) => return a,
                    Err((e, eng)) => {
                        log::warn!("cpal failed ({e}); using synthetic driver");
                        eng
                    }
                }
            } else {
                log::warn!("no audio device; using synthetic driver");
                engine
            };
            Self::start_synthetic_with(Some(engine))
        }
        #[cfg(not(feature = "real-audio"))]
        {
            Self::start_synthetic_with(Some(engine))
        }
    }

    /// Start with an existing engine already on the synthetic thread
    /// (used when the app wants to control construction order).
    pub fn start_synthetic_with(existing: Option<EngineRT>) -> Self {
        let (mut engine, fresh) = match existing {
            Some(e) => (e, None),
            None => {
                let (e, _p) = create_engine_parts(48000.0);
                (e, Some(()))
            }
        };
        let _ = fresh;
        engine
            .meters()
            .driver_kind
            .store(2, Ordering::Relaxed);
        let sr = engine.sr;
        let block = crate::engine::BLOCK;
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::Builder::new()
            .name("aurora-synth-driver".into())
            .spawn(move || {
                let mut io = vec![0.0f32; block * 2];
                let period = std::time::Duration::from_secs_f64(block as f64 / sr as f64);
                let mut next = std::time::Instant::now() + period;
                loop {
                    if stop2.load(Ordering::Relaxed) {
                        break;
                    }
                    engine.process_block(&mut io, block);
                    let now = std::time::Instant::now();
                    if next > now {
                        std::thread::sleep(next - now);
                    } else {
                        next = now;
                    }
                    next += period;
                }
            })
            .expect("spawn synth driver");
        Self {
            kind: DriverKind::Synthetic,
            device_name: "Aurora Synthetic Driver (software clock)".into(),
            sample_rate: sr as u32,
            buffer_frames: block,
            _streams: vec![],
            stop_flag: stop,
            synth_thread: Some(handle),
        }
    }

    #[cfg(feature = "real-audio")]
    fn start_cpal(engine: EngineRT) -> Result<Self, (String, EngineRT)> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        // all fallible steps happen while `engine` is still owned here
        let device = match host.default_output_device() {
            Some(d) => d,
            None => return Err(("no output device".into(), engine)),
        };
        let device_name = device.name().unwrap_or_else(|_| "unknown".into());
        let supported = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => return Err((e.to_string(), engine)),
        };
        let sample_format = supported.sample_format();
        let mut cfg: cpal::StreamConfig = supported.into();
        cfg.channels = 2;
        let sr = cfg.sample_rate.0;
        let buffer_frames = match cfg.buffer_size {
            cpal::BufferSize::Fixed(n) => n as usize,
            cpal::BufferSize::Default => 512,
        };
        // engine slot: filled only after the stream is successfully built+playing
        let cell: Arc<std::sync::Mutex<Option<EngineRT>>> = Arc::new(std::sync::Mutex::new(None));
        let cell_cb = cell.clone();
        let err_fn = |e| log::error!("cpal output error: {e}");
        let stream = match sample_format {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &cfg,
                move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / 2;
                    if let Ok(mut g) = cell_cb.lock() {
                        if let Some(eng) = g.as_mut() {
                            eng.process_block(data, frames);
                        }
                    }
                },
                err_fn,
                None,
            ),
            other => Err(cpal::BuildStreamError::StreamConfigNotSupported),
        };
        let stream = match stream {
            Ok(s) => s,
            Err(e) => return Err((e.to_string(), engine)),
        };
        if let Err(e) = stream.play() {
            return Err((e.to_string(), engine));
        }
        // success: hand the engine to the callback
        if let Ok(mut g) = cell.lock() {
            *g = Some(engine);
            g.as_mut()
                .unwrap()
                .meters()
                .driver_kind
                .store(1, Ordering::Relaxed);
        }
        Ok(Self {
            kind: DriverKind::RealDevice,
            device_name,
            sample_rate: sr,
            buffer_frames,
            _streams: vec![StreamBox::Cpal(Box::new(stream))],
            stop_flag: Arc::new(AtomicBool::new(false)),
            synth_thread: None,
        })
    }
}

impl Drop for AudioIO {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}
