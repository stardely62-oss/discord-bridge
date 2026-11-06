//! Verified stop for the installer (`--stop-engine`) and the uninstaller.
//!
//! "Stopped" means: the service reached STOPPED *and* no winws whose image
//! lies inside one of Discord Bridge's bundle directories is still running.
//! Foreign winws / WinDivert users are neither stopped nor waited for.
//! Platform independent so it is tested everywhere; `service.rs` plugs in
//! the Windows calls.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopOutcome {
    /// Service stopped (or was not running) and none of our winws remain.
    Stopped,
    /// The service is not installed and none of our winws run.
    NotInstalled,
    /// The stop request failed or the service did not reach STOPPED in time.
    ServiceNotStopped(String),
    /// The service stopped but our winws processes are still alive.
    EngineStillRunning(Vec<u32>),
}

impl StopOutcome {
    /// Exit code of `--stop-engine` (read by the NSIS hooks).
    pub fn exit_code(&self) -> i32 {
        match self {
            StopOutcome::Stopped | StopOutcome::NotInstalled => 0,
            StopOutcome::ServiceNotStopped(_) => 3,
            StopOutcome::EngineStillRunning(_) => 4,
        }
    }
    pub fn message(&self) -> String {
        match self {
            StopOutcome::Stopped => "служба и движок остановлены".into(),
            StopOutcome::NotInstalled => "служба не установлена, движок не запущен".into(),
            StopOutcome::ServiceNotStopped(e) => format!("служба не остановилась: {e}"),
            StopOutcome::EngineStillRunning(p) => {
                format!("winws Discord Bridge ещё работает (PID {p:?})")
            }
        }
    }
}

fn norm(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// `image` lies inside one of `roots` (case-insensitive, Windows paths).
pub fn inside_any(image: &Path, roots: &[PathBuf]) -> bool {
    let img = norm(image);
    roots.iter().any(|r| {
        let r = norm(r);
        !r.is_empty()
            && img.len() > r.len()
            && img.starts_with(&r)
            && img.as_bytes()[r.len()] == b'\\'
    })
}

/// Our winws processes among `(pid, image)` pairs.
pub fn our_winws(procs: &[(u32, Option<PathBuf>)], roots: &[PathBuf]) -> Vec<u32> {
    procs
        .iter()
        .filter(|(_, img)| {
            img.as_deref().is_some_and(|i| {
                // Windows paths: take the last segment by hand so the logic is
                // the same on every platform the tests run on.
                norm(i).rsplit('\\').next() == Some("winws.exe") && inside_any(i, roots)
            })
        })
        .map(|(pid, _)| *pid)
        .collect()
}

/// Polls `done` until it returns true or `timeout` passes.
pub fn wait_for(timeout: Duration, step: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if done() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(step);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from(r"C:\Program Files\Discord Bridge\flowseal"),
            PathBuf::from(r"C:\ProgramData\DiscordBridge\bundles"),
        ]
    }

    #[test]
    fn recognises_only_our_winws() {
        let procs = vec![
            (
                10,
                Some(PathBuf::from(
                    r"C:\PROGRAM FILES\Discord Bridge\flowseal\1.10.4\bin\winws.exe",
                )),
            ),
            (
                11,
                Some(PathBuf::from(
                    r"C:\ProgramData\DiscordBridge\bundles\1.10.5\bin\WINWS.EXE",
                )),
            ),
            (
                12,
                Some(PathBuf::from(r"C:\zapret-discord-youtube\bin\winws.exe")),
            ), // foreign Flowseal
            (
                13,
                Some(PathBuf::from(
                    r"C:\Program Files\Discord Bridge\flowseal-evil\winws.exe",
                )),
            ), // prefix trick
            (
                14,
                Some(PathBuf::from(
                    r"C:\Program Files\Discord Bridge\flowseal\1.10.4\bin\other.exe",
                )),
            ),
            (15, None), // image not readable
        ];
        assert_eq!(our_winws(&procs, &roots()), vec![10, 11]);
    }

    #[test]
    fn exit_codes_are_distinct() {
        assert_eq!(StopOutcome::Stopped.exit_code(), 0);
        assert_eq!(StopOutcome::NotInstalled.exit_code(), 0);
        assert_eq!(StopOutcome::ServiceNotStopped("x".into()).exit_code(), 3);
        assert_eq!(StopOutcome::EngineStillRunning(vec![1]).exit_code(), 4);
    }

    #[test]
    fn wait_for_reports_timeouts() {
        assert!(!wait_for(
            Duration::from_millis(30),
            Duration::from_millis(5),
            || false
        ));
        let mut n = 0;
        assert!(wait_for(
            Duration::from_secs(1),
            Duration::from_millis(1),
            || {
                n += 1;
                n > 3
            }
        ));
    }
}
