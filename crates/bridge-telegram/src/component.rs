//! The bundled engine: `<resources>/tg-ws-proxy/discord-bridge-tg-ws-proxy.exe`
//! and its manifest `tg-ws-proxy.json` (written by `telegram-proxy/build.ps1`).
//!
//! The executable is used only if its SHA-256 equals the hash compiled into
//! Bridge at build time (from the same build) and the manifest. The
//! check is repeated right before every start.

use crate::{EXE_NAME, MANIFEST_NAME};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub component: String,
    pub version: String,
    pub upstream_commit: String,
    #[serde(default)]
    pub runner: String,
    #[serde(default)]
    pub python: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Component {
    pub exe: PathBuf,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "detail")]
pub enum ComponentProblem {
    /// Not part of this build or not installed.
    NotInstalled,
    /// Present but its hash does not match: never started.
    Damaged(String),
}

pub fn sha256_file(p: &Path) -> Result<String, String> {
    let data = std::fs::read(p).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(&data)))
}

/// Finds and verifies the engine in `dir`. `compiled_sha256`: the hash the
/// app was built with (None in builds without the component).
pub fn locate(dir: &Path, compiled_sha256: Option<&str>) -> Result<Component, ComponentProblem> {
    let Some(expected) = compiled_sha256.filter(|s| s.len() == 64) else {
        return Err(ComponentProblem::NotInstalled);
    };
    let exe = dir.join(EXE_NAME);
    if !exe.is_file() {
        return Err(ComponentProblem::NotInstalled);
    }
    let manifest: Manifest = std::fs::read(dir.join(MANIFEST_NAME))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| {
            ComponentProblem::Damaged("нет или повреждён файл описания компонента".into())
        })?;
    let c = Component { exe, manifest };
    c.verify(expected)?;
    Ok(c)
}

impl Component {
    /// Hash and size of the file on disk against the build and the manifest.
    pub fn verify(&self, expected: &str) -> Result<(), ComponentProblem> {
        let damaged = |m: &str| Err(ComponentProblem::Damaged(m.into()));
        if !self.manifest.sha256.eq_ignore_ascii_case(expected) {
            return damaged("файл описания не соответствует этой сборке Bridge");
        }
        let meta =
            std::fs::metadata(&self.exe).map_err(|e| ComponentProblem::Damaged(e.to_string()))?;
        if meta.len() != self.manifest.size {
            return damaged("размер исполняемого файла не совпадает");
        }
        match sha256_file(&self.exe) {
            Ok(h) if h.eq_ignore_ascii_case(expected) => Ok(()),
            Ok(_) => {
                damaged("SHA-256 исполняемого файла не совпадает — файл изменён или повреждён")
            }
            Err(e) => damaged(&e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(dir: &Path, body: &[u8], sha_in_manifest: Option<&str>) -> String {
        let sha = hex::encode(Sha256::digest(body));
        std::fs::write(dir.join(EXE_NAME), body).unwrap();
        let m = Manifest {
            component: "tg-ws-proxy-headless".into(),
            version: "1.11.4".into(),
            upstream_commit: "2085861957070eeea07990a522b29f42e5230210".into(),
            runner: "1".into(),
            python: "3.11.9".into(),
            sha256: sha_in_manifest.unwrap_or(&sha).into(),
            size: body.len() as u64,
        };
        std::fs::write(dir.join(MANIFEST_NAME), serde_json::to_vec(&m).unwrap()).unwrap();
        sha
    }

    #[test]
    fn a_matching_engine_is_found() {
        let d = tempfile::tempdir().unwrap();
        let sha = install(d.path(), b"engine", None);
        let c = locate(d.path(), Some(&sha)).unwrap();
        assert_eq!(c.manifest.version, "1.11.4");
    }

    #[test]
    fn missing_or_unbuilt_engine_is_not_installed() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            locate(d.path(), Some(&"a".repeat(64))).unwrap_err(),
            ComponentProblem::NotInstalled
        );
        let sha = install(d.path(), b"engine", None);
        assert_eq!(
            locate(d.path(), None).unwrap_err(),
            ComponentProblem::NotInstalled
        );
        assert!(locate(d.path(), Some(&sha)).is_ok());
    }

    #[test]
    fn a_changed_or_foreign_engine_is_damaged_and_never_used() {
        let d = tempfile::tempdir().unwrap();
        let sha = install(d.path(), b"engine", None);
        // Same size, other content.
        std::fs::write(d.path().join(EXE_NAME), b"enginE").unwrap();
        assert!(matches!(
            locate(d.path(), Some(&sha)),
            Err(ComponentProblem::Damaged(_))
        ));
        // Manifest and file replaced consistently, but not by this build.
        let other = install(d.path(), b"another engine", None);
        assert_ne!(other, sha);
        assert!(matches!(
            locate(d.path(), Some(&sha)),
            Err(ComponentProblem::Damaged(_))
        ));
        // Broken manifest.
        std::fs::write(d.path().join(MANIFEST_NAME), b"{").unwrap();
        assert!(matches!(
            locate(d.path(), Some(&other)),
            Err(ComponentProblem::Damaged(_))
        ));
    }
}
