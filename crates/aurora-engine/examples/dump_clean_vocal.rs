//! Writes before/after WAVs of the AI vocal cleanup for spectrogram evidence.
fn main() {
    let contaminated = aurora_engine::demo::demo_vocal_samples(14.0);
    let (clean, report) = aurora_engine::ai::clean_vocal(&contaminated, 48000, &Default::default());
    let dir = std::path::Path::new("/home/z/my-project/download");
    std::fs::create_dir_all(dir).ok();
    aurora_engine::io::encode_wav(&dir.join("vocal_BEFORE_ai_cleanup.wav"), &contaminated, 48000, aurora_engine::io::ExportFormat::Wav24).unwrap();
    aurora_engine::io::encode_wav(&dir.join("vocal_AFTER_ai_cleanup.wav"), &clean, 48000, aurora_engine::io::ExportFormat::Wav24).unwrap();
    println!("report: noise -{:.1}dB clicks={} breaths={} hum={:?}", report.noise_reduction_est_db, report.clicks_fixed, report.breaths_removed, report.hum_freqs);
}
