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