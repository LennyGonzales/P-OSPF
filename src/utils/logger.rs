use env_logger::{Builder, Target};
use log::LevelFilter;
use std::io::Write;

mod log {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::sync::Mutex;
    use lazy_static::lazy_static;

    lazy_static! {
        static ref LOG_FILE: Mutex<std::fs::File> = Mutex::new(OpenOptions::new()
            .create(true)
            .append(true)
            .open("routing_protocol.log")
            .unwrap());
    }

    pub fn log_info(message: &str) {
        let mut log_file = LOG_FILE.lock().unwrap();
        writeln!(log_file, "[INFO] {}", message).unwrap();
    }

    pub fn log_error(message: &str) {
        let mut log_file = LOG_FILE.lock().unwrap();
        writeln!(log_file, "[ERROR] {}", message).unwrap();
    }

    pub fn log_debug(message: &str) {
        let mut log_file = LOG_FILE.lock().unwrap();
        writeln!(log_file, "[DEBUG] {}", message).unwrap();
    }
}

pub fn init_logger() {
    let mut builder = Builder::from_default_env();

    builder
        .target(Target::Stdout)
        .filter_level(LevelFilter::Info)
        .format(|buf, record| {
            writeln!(
                buf,
                "{} [{}] - {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.args()
            )
        })
        .init();
}