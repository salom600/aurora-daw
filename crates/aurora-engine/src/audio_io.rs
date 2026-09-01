//! Audio device layer — real I/O via cpal (ALSA/WASAPI/CoreAudio) with an
//! automatic precision synthetic driver fallback for environments without a
//! sound device (containers/CI). Output AND input (microphone) both run on
//! the real device when available: captures are fed into the engine's
//! lock-free input ring, converted from the device's native sample format,
//! channel count and sample rate to the engine's stereo 48 kHz stream.

use crate::engine::{EngineRT, LoudnessTap, MeterStore, ParamStore, SpectralTap};
use rtrb::Producer;
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
    /// Capture side: RealDevice = live microphone feeding the engine ring.
    pub input_kind: DriverKind,
    pub input_name: String,
    pub input_sample_rate: u32,
    /// Device name listings for the UI (first = default).
    pub outputs: Vec<String>,
    pub inputs: Vec<String>,
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
            let _ = &engine;
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
            input_kind: DriverKind::Synthetic,
            input_name: "Simulated source (demo vocal)".into(),
            input_sample_rate: sr as u32,
            outputs: Vec::new(),
            inputs: Vec::new(),
            _streams: vec![],
            stop_flag: stop,
            synth_thread: Some(handle),
        }
    }

    #[cfg(feature = "real-audio")]
    fn start_cpal(engine: EngineRT) -> Result<Self, (String, EngineRT)> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        // enumerate device listings for the UI (best-effort)
        let outputs: Vec<String> = host
            .output_devices()
            .map(|it| it.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default();
        let inputs: Vec<String> = host
            .input_devices()
            .map(|it| it.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default();
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

        // ------------------------------------------------------------------
        // Capture stream — the REAL microphone. Feed the engine input ring.
        // ------------------------------------------------------------------
        let meters = cell
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|e| e.meters().clone()));
        let mut input_kind = DriverKind::Synthetic;
        let mut input_name = "no input device found".to_string();
        let mut input_sample_rate = 0u32;
        let mut in_streams: Vec<cpal::Stream> = Vec::new();
        if let Some(meters) = meters {
            let in_tx = cell
                .lock()
                .ok()
                .and_then(|mut g| g.as_mut().and_then(|e| e.take_input_producer()));
            if let Some(tx) = in_tx {
                if let Some(in_dev) = host.default_input_device() {
                    let in_name_try = in_dev.name().unwrap_or_else(|_| "input".into());
                    match in_dev.default_input_config() {
                        Ok(in_sup) => {
                            let in_sr = in_sup.sample_rate().0;
                            let in_cfg: cpal::StreamConfig = in_sup.into();
                            let res = build_capture_stream(
                                &in_dev,
                                &in_cfg,
                                tx,
                                in_sr,
                                sr.min(48000).max(8000),
                            );
                            match res {
                                Ok(s) => {
                                    if s.play().is_ok() {
                                        input_kind = DriverKind::RealDevice;
                                        input_name = in_name_try;
                                        input_sample_rate = in_sr;
                                        meters.input_rate.store(in_sr, Ordering::Relaxed);
                                        log::info!(
                                            "capture device live: {input_name} @ {in_sr} Hz"
                                        );
                                        in_streams.push(s);
                                    } else {
                                        log::warn!("capture stream failed to start");
                                    }
                                }
                                Err(e) => {
                                    log::warn!("capture stream build failed: {e}");
                                }
                            }
                        }
                        Err(e) => log::warn!("input config unavailable: {e}"),
                    }
                }
            }
        }

        let mut streams = vec![StreamBox::Cpal(Box::new(stream))];
        for s in in_streams {
            streams.push(StreamBox::Cpal(Box::new(s)));
        }
        Ok(Self {
            kind: DriverKind::RealDevice,
            device_name,
            sample_rate: sr,
            buffer_frames,
            input_kind,
            input_name,
            input_sample_rate,
            outputs,
            inputs,
            _streams: streams,
            stop_flag: Arc::new(AtomicBool::new(false)),
            synth_thread: None,
        })
    }
}

#[cfg(feature = "real-audio")]
fn build_capture_stream(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    tx: Producer<f32>,
    in_sr: u32,
    out_sr: u32,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    use cpal::traits::DeviceTrait;
    let channels = cfg.channels.max(1) as usize;
    let err_fn = |e| log::error!("cpal capture error: {e}");
    macro_rules! cap {
        ($t:ty, $conv:expr) => {{
            let mut tx = tx;
            let mut rs = StereoResampler::new(in_sr, out_sr);
            dev.build_input_stream(
                cfg,
                move |data: &[$t], _: &cpal::InputCallbackInfo| {
                    let conv: fn($t) -> f32 = $conv;
                    let mut frames = data.chunks_exact(channels);
                    while let Some(fr) = frames.next() {
                        let (l, r) = if channels >= 2 {
                            (conv(fr[0]), conv(fr[1]))
                        } else {
                            let m = conv(fr[0]).clamp(-1.0, 1.0);
                            (m, m)
                        };
                        rs.push(l, r, &mut tx);
                    }
                },
                err_fn,
                None,
            )
        }};
    }
    match cfg_sample_format(dev, cfg) {
        cpal::SampleFormat::F32 => cap!(f32, |v: f32| v),
        cpal::SampleFormat::I16 => cap!(i16, |v: i16| v as f32 / 32768.0),
        cpal::SampleFormat::U16 => cap!(u16, |v: u16| (v as f32 - 32768.0) / 32768.0),
        _ => Err(cpal::BuildStreamError::StreamConfigNotSupported),
    }
}

#[cfg(feature = "real-audio")]
fn cfg_sample_format(dev: &cpal::Device, _cfg: &cpal::StreamConfig) -> cpal::SampleFormat {
    use cpal::traits::DeviceTrait;
    dev.default_input_config()
        .map(|c| c.sample_format())
        .unwrap_or(cpal::SampleFormat::F32)
}

/// Minimal linear-interpolation resampler converting an arbitrary device
/// capture rate to the engine rate, stereo domain, chunk-boundary safe.
#[cfg(feature = "real-audio")]
struct StereoResampler {
    /// output frames per input frame
    step: f64,
    frac: f64,
    prev: [f32; 2],
    started: bool,
}

#[cfg(feature = "real-audio")]
impl StereoResampler {
    fn new(in_sr: u32, out_sr: u32) -> Self {
        Self {
            step: (out_sr as f64 / in_sr as f64).max(1e-6),
            frac: 0.0,
            prev: [0.0; 2],
            started: false,
        }
    }
    #[inline]
    fn push(&mut self, l: f32, r: f32, tx: &mut Producer<f32>) {
        if !self.started {
            self.started = true;
            self.prev = [l, r];
            return;
        }
        while self.frac < 1.0 {
            let t = self.frac as f32;
            let _ = tx.push(self.prev[0] + (l - self.prev[0]) * t);
            let _ = tx.push(self.prev[1] + (r - self.prev[1]) * t);
            self.frac += self.step;
        }
        self.frac -= 1.0;
        self.prev = [l, r];
    }
}

impl Drop for AudioIO {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}
