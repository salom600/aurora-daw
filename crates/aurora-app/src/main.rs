//! AURORA Producer Suite — entry point.
//!
//! Windows hardening:
//! - GUI subsystem (no stray console window in release)
//! - logs go to %LOCALAPPDATA%\AuroraDAW\aurora.log
//! - panics are reported in a native message box instead of dying silently
//! - renderer fallback: OpenGL (glow) first, then wgpu (Vulkan/DX12) so the
//!   window opens even on GPUs without OpenGL 3.2

#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

use app::{AppOptions, AuroraApp};

mod app;
mod autotest;
mod panels;
mod theme;
mod widgets;

// ---------------------------------------------------------------------------
// Logging — file on Windows (GUI apps have no console), stderr elsewhere.
// ---------------------------------------------------------------------------

struct FileLogger(std::sync::Mutex<std::fs::File>);

impl log::Log for FileLogger {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        use std::io::Write;
        if self.enabled(record.metadata()) {
            if let Ok(mut f) = self.0.lock() {
                let _ = writeln!(
                    f,
                    "{} [{}] {}",
                    chrono_now(),
                    record.level(),
                    record.args()
                );
            }
        }
    }
    fn flush(&self) {}
}

fn chrono_now() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let s = t.as_secs();
    let (h, m, sec) = ((s % 86400) / 3600, (s % 3600) / 60, s % 60);
    format!("{h:02}:{m:02}:{sec:02}")
}

fn log_file_path() -> Option<std::path::PathBuf> {
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        let dir = std::path::PathBuf::from(base).join("AuroraDAW");
        if std::fs::create_dir_all(&dir).is_ok() {
            return Some(dir.join("aurora.log"));
        }
    }
    if let Some(base) = std::env::var_os("HOME") {
        let dir = std::path::PathBuf::from(base).join(".aurora-daw");
        if std::fs::create_dir_all(&dir).is_ok() {
            return Some(dir.join("aurora.log"));
        }
    }
    None
}

fn init_logging() {
    if let Some(path) = log_file_path() {
        if let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = log::set_boxed_logger(Box::new(FileLogger(std::sync::Mutex::new(f))));
            log::set_max_level(log::LevelFilter::Info);
            log::info!("AURORA starting — log file {}", path.display());
            return;
        }
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
}

// ---------------------------------------------------------------------------
// Panic reporting — never die silently.
// ---------------------------------------------------------------------------

fn init_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("AURORA crashed: {info}");
        log::error!("{msg}");
        #[cfg(windows)]
        show_windows_message(&msg, "AURORA Producer Suite — Error");
        default(info);
    }));
}

#[cfg(windows)]
fn show_windows_message(text: &str, title: &str) {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let wide_title: Vec<u16> = std::ffi::OsStr::new(title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW;
        MessageBoxW(std::ptr::null_mut(), wide.as_ptr(), wide_title.as_ptr(), 0x40); // MB_ICONINFORMATION
    }
}

fn show_fatal(text: &str) {
    eprintln!("{text}");
    log::error!("{text}");
    #[cfg(windows)]
    show_windows_message(
        &format!(
            "{text}\n\nTroubleshooting:\n• Update your graphics driver (the UI tried OpenGL and DirectX/Vulkan)\n• Check %LOCALAPPDATA%\\AuroraDAW\\aurora.log for details\n• Try: aurora-daw-windows.exe --selftest in a terminal"
        ),
        "AURORA Producer Suite — could not start",
    );
}

fn native_opts(renderer: eframe::Renderer) -> eframe::NativeOptions {
    eframe::NativeOptions {
        renderer,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1200.0, 720.0])
            .with_title("AURORA Producer Suite — Virtual Studio"),
        ..Default::default()
    }
}

fn main() {
    init_logging();
    init_panic_hook();

    let args: Vec<String> = std::env::args().collect();
    // headless engine self-test
    if args.iter().any(|a| a == "--selftest") {
        let results = aurora_engine::selftest::run_all();
        let ok = aurora_engine::selftest::print_report(&results);
        std::process::exit(if ok { 0 } else { 1 });
    }

    let mut opts = AppOptions::default();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--empty" => opts.empty = true,
            "--stress" => {
                opts.stress = it.next().and_then(|v| v.parse().ok()).unwrap_or(1000);
            }
            "--shots" => {
                opts.shots_dir = it.next().cloned();
            }
            "--autotest" => {
                opts.autotest = true;
                if opts.shots_dir.is_none() {
                    opts.shots_dir = it.next().cloned();
                }
            }
            _ => {}
        }
    }

    // Renderer resilience: try OpenGL first (fastest), fall back to
    // wgpu (Vulkan / DirectX 12) for GPUs without OpenGL 3.2 support.
    let opts = std::cell::RefCell::new(opts);
    let mut last_err: Option<eframe::Error> = None;
    let mut started = false;
    for renderer in [eframe::Renderer::Glow, eframe::Renderer::Wgpu] {
        let o = opts.borrow().clone();
        match eframe::run_native(
            "AURORA Producer Suite",
            native_opts(renderer),
            Box::new(move |cc| Ok(Box::new(AuroraApp::new(cc, o)))),
        ) {
            Ok(()) => {
                started = true;
                break;
            }
            Err(e) => {
                log::warn!("renderer {renderer:?} failed to start: {e} — trying next backend");
                last_err = Some(e);
            }
        }
    }
    if !started {
        if let Some(e) = last_err {
            show_fatal(&format!("AURORA failed to start: {e}"));
        }
        std::process::exit(1);
    }
}
