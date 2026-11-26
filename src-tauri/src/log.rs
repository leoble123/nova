//! A small log file (`nova.log` in Nova's data folder) so problems on someone's PC leave
//! evidence: startup facts, crashes (Rust panics), interface errors, and WebView2 process
//! failures. Kept under ~1 MB (one previous file is kept as `nova.log.1`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
const MAX: u64 = 1024 * 1024;

pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("nova.log");
    if std::fs::metadata(&path).map(|m| m.len() > MAX).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("nova.log.1"));
    }
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        line("PANIC", &format!("{info}"));
        prev(info);
    }));
}

pub fn line(level: &str, text: &str) {
    let Some(path) = FILE.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // UTC time of day is enough to line events up; avoid pulling in a date library.
    let t = format!("{:02}:{:02}:{:02}", secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let text: String = text.chars().take(4000).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{t} {level:<5} {}", text.replace('\n', "\n                "));
    }
    if cfg!(debug_assertions) {
        eprintln!("[nova] {level} {text}");
    }
}

#[macro_export]
macro_rules! log_info {
    ($($t:tt)*) => { $crate::log::line("INFO", &format!($($t)*)) };
}
#[macro_export]
macro_rules! log_warn {
    ($($t:tt)*) => { $crate::log::line("WARN", &format!($($t)*)) };
}
