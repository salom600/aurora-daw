//! AURORA Producer Suite — entry point.

use app::{AppOptions, AuroraApp};

mod app;
mod autotest;
mod panels;
mod theme;
mod widgets;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

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

    let native = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1200.0, 720.0])
            .with_title("AURORA Producer Suite — Virtual Studio"),
        ..Default::default()
    };

    let r = eframe::run_native(
        "AURORA Producer Suite",
        native,
        Box::new(move |cc| Ok(Box::new(AuroraApp::new(cc, opts)))),
    );
    if let Err(e) = r {
        eprintln!("AURORA failed to start: {e}");
        std::process::exit(1);
    }
}
