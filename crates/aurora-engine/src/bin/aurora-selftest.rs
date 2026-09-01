fn main() {
    env_logger::try_init().ok();
    let results = aurora_engine::selftest::run_all();
    let ok = aurora_engine::selftest::print_report(&results);
    std::process::exit(if ok { 0 } else { 1 });
}
