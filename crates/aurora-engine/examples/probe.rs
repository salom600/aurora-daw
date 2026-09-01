fn main() {
    let (mut engine, _parts) = aurora_engine::audio_io::create_engine_parts(48000.0);
    let p = aurora_engine::demo::build_vocal_session();
    engine.load_project(&p);
    engine.params().slots[2].flags.store(4|8, std::sync::atomic::Ordering::Relaxed);
    engine.apply(aurora_engine::engine::Command::SetSimulatedInput(true));
    engine.apply(aurora_engine::engine::Command::StartRecord{ position: 0.0, capacity_frames: 48000*3 });
    let mut io = vec![0.0f32; 512*2];
    for i in 0..100 {
        engine.process_block(&mut io, 512);
        if i % 25 == 0 {
            println!("block {i}: input_peak_bits={:?} sim={}", f32::from_bits(engine.meters().input_peak.load(std::sync::atomic::Ordering::Relaxed)), true);
        }
    }
    engine.apply(aurora_engine::engine::Command::StopRecord);
    let rx = engine.take_back_receiver().unwrap();
    while let Ok(ev) = rx.recv_timeout(std::time::Duration::from_millis(500)) {
        match ev {
            aurora_engine::engine::BackEvent::RecordedTake{samples, ..} => println!("TAKE: {} samples", samples.len()),
            aurora_engine::engine::BackEvent::Notice(m) => println!("NOTICE: {m}"),
        }
    }
}
