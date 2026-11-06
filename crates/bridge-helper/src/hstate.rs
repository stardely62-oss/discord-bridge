//! Persistent helper state and TCP timestamps maintenance (platform
//! independent).
//!
//! The maintenance functions here only read and write the small state file.
//! They never construct the engine controller, so running
//! `--tcp-changed` / `--restore-tcp` (e.g. from the uninstaller while the
//! service and its winws are still running) cannot touch runtime
//! directories, processes or the Zapret state.

use crate::policy::decide_tcp_restore;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HelperState {
    pub active_bundle: Option<String>,
    /// TCP timestamps value before Discord Bridge changed it the first time.
    pub tcp_original: Option<bool>,
    /// Value Discord Bridge set (to detect later changes by others).
    pub tcp_we_set: Option<bool>,
}

impl HelperState {
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        bridge_core::atomic::write_atomic(path, &data)
            .map_err(|e| format!("не удалось сохранить состояние службы: {e}"))
    }
}

/// True if Discord Bridge changed TCP timestamps and the value is still ours.
pub fn tcp_changed_by_us(state_file: &Path, current: Option<bool>) -> bool {
    let st = HelperState::load(state_file);
    matches!((st.tcp_we_set, current), (Some(w), Some(c)) if w == c)
}

/// Restore the recorded original value if it is still the one we set.
/// `set` performs the actual system change. Only the state file is written.
pub fn tcp_restore(
    state_file: &Path,
    current: Option<bool>,
    set: impl FnOnce(bool) -> Result<(), String>,
) -> Result<String, String> {
    let mut st = HelperState::load(state_file);
    let decision = decide_tcp_restore(st.tcp_original, st.tcp_we_set, current);
    let forget = |st: &mut HelperState| {
        st.tcp_original = None;
        st.tcp_we_set = None;
    };
    match decision {
        Ok(None) => Ok("Discord Bridge эту настройку не менял".into()),
        Ok(Some(v)) => {
            set(v)?;
            forget(&mut st);
            st.save(state_file)?;
            Ok("исходное состояние восстановлено".into())
        }
        Err(e) => {
            // Someone else changed it: forget our record, never fight them.
            forget(&mut st);
            st.save(state_file)?;
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("DiscordBridge");
        let gen = data.join("runtime").join("gen-7").join("lists");
        std::fs::create_dir_all(&gen).unwrap();
        std::fs::write(gen.join("list-general.txt"), "discord.com\n").unwrap();
        let state = data.join("helper-state.json");
        HelperState {
            active_bundle: Some("1.10.4".into()),
            tcp_original: Some(false),
            tcp_we_set: Some(true),
        }
        .save(&state)
        .unwrap();
        (t, state, gen)
    }

    #[test]
    fn tcp_maintenance_never_touches_runtime_directories() {
        let (_t, state, gen) = layout();
        assert!(tcp_changed_by_us(&state, Some(true)));
        assert!(
            gen.join("list-general.txt").exists(),
            "query left runtime intact"
        );

        let mut set_to = None;
        let msg = tcp_restore(&state, Some(true), |v| {
            set_to = Some(v);
            Ok(())
        })
        .unwrap();
        assert_eq!(set_to, Some(false), "original value restored");
        assert!(msg.contains("восстановлено"));
        assert!(
            gen.join("list-general.txt").exists(),
            "restore left runtime intact"
        );
        let st = HelperState::load(&state);
        assert_eq!(
            st.active_bundle.as_deref(),
            Some("1.10.4"),
            "other state preserved"
        );
        assert_eq!((st.tcp_original, st.tcp_we_set), (None, None));
    }

    #[test]
    fn changed_by_someone_else_is_not_reverted() {
        let (_t, state, gen) = layout();
        assert!(!tcp_changed_by_us(&state, Some(false)));
        let r = tcp_restore(&state, Some(false), |_| {
            panic!("must not change the system")
        });
        assert!(r.is_err());
        assert!(gen.exists());
    }

    #[test]
    fn missing_state_file_is_harmless() {
        let t = tempfile::tempdir().unwrap();
        let state = t.path().join("none.json");
        assert!(!tcp_changed_by_us(&state, Some(true)));
        assert!(tcp_restore(&state, Some(true), |_| panic!("no change")).is_ok());
    }
}
