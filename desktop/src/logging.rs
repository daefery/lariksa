//! A small rotating log file next to the meetings (``data/lariksa.log``).

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAX_BYTES: u64 = 2_000_000;
const BACKUPS: u32 = 2;

struct FileLog {
    path: PathBuf,
    file: Mutex<Option<File>>,
}

impl FileLog {
    fn rotate(&self, file: &mut Option<File>) {
        if std::fs::metadata(&self.path).map_or(0, |m| m.len()) < MAX_BYTES {
            return;
        }
        *file = None;
        for i in (1..BACKUPS).rev() {
            let _ = std::fs::rename(self.path.with_extension(format!("log.{i}")), self.path.with_extension(format!("log.{}", i + 1)));
        }
        let _ = std::fs::rename(&self.path, self.path.with_extension("log.1"));
    }
}

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let mut file = self.file.lock().unwrap_or_else(|e| e.into_inner());
        self.rotate(&mut file);
        if file.is_none() {
            *file = OpenOptions::new().create(true).append(true).open(&self.path).ok();
        }
        if let Some(f) = file.as_mut() {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "{now} {} {}: {}", r.level(), r.target(), r.args());
        }
    }

    fn flush(&self) {}
}

pub fn init(path: &Path) {
    let logger = Box::leak(Box::new(FileLog { path: path.to_path_buf(), file: Mutex::new(None) }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
