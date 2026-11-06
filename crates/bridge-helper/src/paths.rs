//! Locations used by the helper. All writable state lives under
//! `%ProgramData%\DiscordBridge`, which the helper re-protects on start.

use std::path::{Path, PathBuf};

/// Name of the UI executable allowed to talk to the helper.
pub const CLIENT_EXE: &str = "discord-bridge.exe";

#[derive(Debug, Clone)]
pub struct Paths {
    pub install_dir: PathBuf,
    pub data_dir: PathBuf,
    /// Tests only: accept a different client executable.
    pub client_exe_override: Option<PathBuf>,
}

impl Paths {
    pub fn detect() -> Self {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
        let install_dir = exe
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let program_data = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        Paths {
            install_dir,
            data_dir: program_data.join("DiscordBridge"),
            client_exe_override: None,
        }
    }
    pub fn client_exe(&self) -> PathBuf {
        if let Some(p) = &self.client_exe_override {
            return p.clone();
        }
        self.install_dir.join(CLIENT_EXE)
    }
    /// Read-only Flowseal bundles shipped by the installer.
    pub fn builtin_bundles(&self) -> PathBuf {
        self.install_dir.join("flowseal")
    }
    pub fn updates_dir(&self) -> PathBuf {
        self.data_dir.join("bundles")
    }
    /// Every directory a winws of Discord Bridge can run from: the shipped
    /// bundles, verified updates (older versions ran from these) and the
    /// immutable engine copies. Used to tell our winws from foreign ones.
    pub fn engine_roots(&self) -> Vec<PathBuf> {
        vec![
            self.builtin_bundles(),
            self.updates_dir(),
            self.engines_dir(),
        ]
    }
    /// Immutable engine copies the helper runs winws from (see `stage`).
    pub fn engines_dir(&self) -> PathBuf {
        self.data_dir.join("engines")
    }
    /// Root of per-launch generation directories (`gen-N/lists`).
    pub fn runtime_root(&self) -> PathBuf {
        self.data_dir.join("runtime")
    }
    pub fn state_file(&self) -> PathBuf {
        self.data_dir.join("helper-state.json")
    }
    pub fn log_file(&self) -> PathBuf {
        self.data_dir.join("helper.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stopcheck::our_winws;

    #[test]
    fn stop_check_recognises_winws_from_every_engine_root() {
        let p = Paths {
            install_dir: PathBuf::from(r"C:\Program Files\Discord Bridge"),
            data_dir: PathBuf::from(r"C:\ProgramData\DiscordBridge"),
            client_exe_override: None,
        };
        let procs = vec![
            // Immutable copy (current versions run from here).
            (
                1,
                Some(PathBuf::from(
                    r"C:\ProgramData\DiscordBridge\engines\1.10.4-0123456789abcdef\bin\winws.exe",
                )),
            ),
            // Shipped bundle (versions up to 0.1.0-alpha.7).
            (
                2,
                Some(PathBuf::from(
                    r"C:\Program Files\Discord Bridge\flowseal\1.10.4\bin\winws.exe",
                )),
            ),
            // Verified update bundle.
            (
                3,
                Some(PathBuf::from(
                    r"C:\ProgramData\DiscordBridge\bundles\1.10.5\bin\winws.exe",
                )),
            ),
            // Foreign instances: never ours.
            (10, Some(PathBuf::from(r"C:\zapret\bin\winws.exe"))),
            (
                11,
                Some(PathBuf::from(
                    r"C:\ProgramData\DiscordBridge-other\engines\bin\winws.exe",
                )),
            ),
            (
                12,
                Some(PathBuf::from(
                    r"C:\ProgramData\DiscordBridge\enginesX\bin\winws.exe",
                )),
            ),
        ];
        assert_eq!(our_winws(&procs, &p.engine_roots()), vec![1, 2, 3]);
    }
}
