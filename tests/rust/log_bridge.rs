//! Native `log` compatibility sink. It formats messages into owned memory.
use std::sync::Mutex;
static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
struct Logger;
static LOGGER: Logger = Logger;
impl log::Log for Logger {
    fn enabled(&self, _: &log::Metadata<'_>) -> bool {
        true
    }
    fn log(&self, record: &log::Record<'_>) {
        RECORDS.lock().unwrap().push(format!("{}", record.args()));
    }
    fn flush(&self) {}
}
pub fn install() {
    assert!(
        !tracing::dispatcher::has_been_set(),
        "log-fallback test must precede subscriber installation"
    );
    log::set_logger(&LOGGER).expect("only one logger per unit binary");
    log::set_max_level(log::LevelFilter::Trace);
}
pub fn records() -> Vec<String> {
    RECORDS.lock().unwrap().clone()
}
