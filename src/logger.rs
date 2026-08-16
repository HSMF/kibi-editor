use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::Mutex,
};

use log::{Level, Log};

struct Logger {
    file: Mutex<File>,
}

impl Logger {
    fn new(p: &str) -> std::io::Result<Self> {
        let mut o = OpenOptions::new();
        o.create(true).append(true);
        let file = o.open(p)?;
        let file = Mutex::new(file);
        Ok(Logger { file })
    }
}

impl Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let file = record.file().unwrap_or("unknown");
        let line = record.line().unwrap_or(0);
        let level = record.level();
        let message = record.args();
        let module = record.module_path().unwrap_or("");
        let mut f = self.file.lock().unwrap();
        let _ = writeln!(f, "[{level}] {module} {file}:{line} {message}");
    }

    fn flush(&self) {
        let _ = self.file.lock().unwrap().flush();
    }
}

pub fn init() -> anyhow::Result<()> {
    let logger = Logger::new("log/output2.log")?;
    log::set_max_level(log::LevelFilter::Debug);
    log::set_boxed_logger(Box::new(logger))?;
    Ok(())
}
