//! Minimal size-capped log for the helper (no external logging stack, no
//! sensitive data: the helper never sees credentials or traffic content).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG: u64 = 1024 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());

pub fn init(path: PathBuf) {
    let _ = PATH.set(path);
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn log(level: &str, msg: &str) {
    let line = format!("{} [{level}] {msg}\n", now_unix());
    if cfg!(debug_assertions) {
        eprint!("{line}");
    }
    let Some(path) = PATH.get() else { return };
    let _g = LOCK.lock();
    if fs::metadata(path)
        .map(|m| m.len() > MAX_LOG)
        .unwrap_or(false)
    {
        let _ = fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[macro_export]
macro_rules! info { ($($t:tt)*) => { $crate::hlog::log("INFO", &format!($($t)*)) } }
#[macro_export]
macro_rules! warn { ($($t:tt)*) => { $crate::hlog::log("WARN", &format!($($t)*)) } }
