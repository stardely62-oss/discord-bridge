//! Local cache of the verified catalog (platform independent, synchronous).
//!
//! Files in the cache directory:
//! - `current.signed.json` + `current.data.json` — the last verified catalog;
//! - `previous.signed.json` + `previous.data.json` — the one before it;
//! - `state.json` — highest accepted serial (anti-rollback), ETag, times,
//!   last error.
//!
//! Everything read from disk is verified again (signature, hashes, packs):
//! a damaged current copy is replaced by the verified previous one, and if
//! neither verifies the built-in snapshot is used. Network access is the
//! caller's job: it hands the fetched bytes to [`CatalogStore::accept_manifest`]
//! and [`CatalogStore::commit`]. Nothing here touches settings or the engine.

use super::{
    check_not_rollback, verify_catalog, verify_manifest, CatalogData, CatalogError, CatalogManifest,
};
use crate::atomic::write_atomic;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct StoreState {
    /// Highest serial ever accepted on this machine.
    pub highest_serial: u64,
    pub etag: Option<String>,
    pub last_check: Option<u64>,
    pub last_success: Option<u64>,
    pub last_error: Option<String>,
    /// The last check failed to reach the server.
    pub offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CatalogOrigin {
    /// A verified published catalog from the cache.
    Remote,
    /// The snapshot compiled into the app (no published catalog yet).
    Builtin,
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub origin: CatalogOrigin,
    pub manifest: Option<CatalogManifest>,
    pub data: CatalogData,
    /// The current copy was damaged and the verified previous one was restored.
    pub repaired: bool,
    pub state: StoreState,
}

pub struct CatalogStore {
    dir: PathBuf,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl CatalogStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        CatalogStore { dir: dir.into() }
    }

    fn p(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn state(&self) -> StoreState {
        std::fs::read(self.p("state.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, st: &StoreState) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        write_atomic(
            &self.p("state.json"),
            &serde_json::to_vec_pretty(st).expect("serializable"),
        )
        .map_err(|e| e.to_string())
    }

    fn read_pair(&self, prefix: &str) -> Option<(Vec<u8>, Vec<u8>)> {
        let s = std::fs::read(self.p(&format!("{prefix}.signed.json"))).ok()?;
        let d = std::fs::read(self.p(&format!("{prefix}.data.json"))).ok()?;
        Some((s, d))
    }

    fn write_pair(&self, prefix: &str, signed: &[u8], data: &[u8]) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        // Data first, then the manifest that refers to it.
        write_atomic(&self.p(&format!("{prefix}.data.json")), data).map_err(|e| e.to_string())?;
        write_atomic(&self.p(&format!("{prefix}.signed.json")), signed).map_err(|e| e.to_string())
    }

    /// The best verified catalog available: current, else previous (and the
    /// damaged current copy is replaced by it), else the built-in snapshot.
    pub fn load(
        &self,
        keys: &[&str],
        app_version: &str,
        builtin: &[u8],
    ) -> Result<Loaded, CatalogError> {
        let state = self.state();
        let verify = |pair: Option<(Vec<u8>, Vec<u8>)>| {
            pair.and_then(|(s, d)| {
                verify_catalog(&s, &d, keys, app_version)
                    .ok()
                    .map(|v| (v, s, d))
            })
        };
        if let Some((v, _, _)) = verify(self.read_pair("current")) {
            return Ok(Loaded {
                origin: CatalogOrigin::Remote,
                manifest: Some(v.manifest),
                data: v.data,
                repaired: false,
                state,
            });
        }
        let had_current =
            self.p("current.signed.json").exists() || self.p("current.data.json").exists();
        if let Some((v, s, d)) = verify(self.read_pair("previous")) {
            if had_current {
                let _ = self.write_pair("current", &s, &d);
            }
            return Ok(Loaded {
                origin: CatalogOrigin::Remote,
                manifest: Some(v.manifest),
                data: v.data,
                repaired: had_current,
                state,
            });
        }
        let data: CatalogData = serde_json::from_slice(builtin)
            .map_err(|e| CatalogError::Malformed(format!("встроенный каталог: {e}")))?;
        data.validate(app_version)?;
        Ok(Loaded {
            origin: CatalogOrigin::Builtin,
            manifest: None,
            data,
            repaired: false,
            state,
        })
    }

    /// Verifies a freshly fetched signed manifest. `Ok(None)`: nothing new
    /// (same serial as the current catalog). Older than the newest accepted
    /// one: `Err(Rollback)`.
    pub fn accept_manifest(
        &self,
        signed: &[u8],
        keys: &[&str],
        app_version: &str,
    ) -> Result<Option<CatalogManifest>, CatalogError> {
        let m = verify_manifest(signed, keys, app_version)?;
        let st = self.state();
        check_not_rollback(m.serial, st.highest_serial)?;
        let current = self
            .read_pair("current")
            .and_then(|(s, d)| verify_catalog(&s, &d, keys, app_version).ok())
            .map(|v| v.manifest.serial);
        if current == Some(m.serial) {
            return Ok(None);
        }
        Ok(Some(m))
    }

    /// Verifies manifest + data and makes them current (the old current
    /// becomes previous). Nothing is written unless everything verifies.
    pub fn commit(
        &self,
        signed: &[u8],
        data: &[u8],
        keys: &[&str],
        app_version: &str,
        etag: Option<String>,
    ) -> Result<CatalogManifest, CatalogError> {
        let v = verify_catalog(signed, data, keys, app_version)?;
        let mut st = self.state();
        check_not_rollback(v.manifest.serial, st.highest_serial)?;
        let io = |e: String| CatalogError::Malformed(format!("запись кеша: {e}"));
        if let Some((s, d)) = self.read_pair("current") {
            if verify_catalog(&s, &d, keys, app_version).is_ok() {
                self.write_pair("previous", &s, &d).map_err(io)?;
            }
        }
        self.write_pair("current", signed, data).map_err(io)?;
        st.highest_serial = v.manifest.serial;
        st.etag = etag;
        st.last_check = Some(now());
        st.last_success = st.last_check;
        st.last_error = None;
        st.offline = false;
        self.save_state(&st).map_err(io)?;
        Ok(v.manifest)
    }

    /// The server had nothing new (304 or the same serial).
    pub fn record_unchanged(&self, etag: Option<String>) {
        let mut st = self.state();
        if etag.is_some() {
            st.etag = etag;
        }
        st.last_check = Some(now());
        st.last_success = st.last_check;
        st.last_error = None;
        st.offline = false;
        let _ = self.save_state(&st);
    }

    pub fn record_failure(&self, error: &str, offline: bool) {
        let mut st = self.state();
        st.last_check = Some(now());
        st.last_error = Some(error.chars().take(300).collect());
        st.offline = offline;
        let _ = self.save_state(&st);
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests_support::{key, publish, sample};
    use super::*;

    const APP: &str = "0.1.0-alpha.9";
    fn builtin() -> Vec<u8> {
        serde_json::to_vec(&sample(1)).unwrap()
    }

    #[test]
    fn without_cache_the_builtin_snapshot_is_used() {
        let t = tempfile::tempdir().unwrap();
        let st = CatalogStore::new(t.path().join("c"));
        let (_, pk) = key();
        let l = st.load(&[&pk], APP, &builtin()).unwrap();
        assert_eq!(l.origin, CatalogOrigin::Builtin);
        assert!(l.manifest.is_none());
    }

    #[test]
    fn commit_then_load_and_previous_is_kept() {
        let t = tempfile::tempdir().unwrap();
        let st = CatalogStore::new(t.path());
        let (k, pk) = key();
        let (s1, d1) = publish(&k, &sample(10));
        assert!(st.accept_manifest(&s1, &[&pk], APP).unwrap().is_some());
        st.commit(&s1, &d1, &[&pk], APP, Some("e1".into())).unwrap();
        // Same serial again: nothing new.
        assert!(st.accept_manifest(&s1, &[&pk], APP).unwrap().is_none());
        let (s2, d2) = publish(&k, &sample(20));
        st.commit(&s2, &d2, &[&pk], APP, None).unwrap();
        let l = st.load(&[&pk], APP, &builtin()).unwrap();
        assert_eq!(
            (l.origin, l.manifest.unwrap().serial),
            (CatalogOrigin::Remote, 20)
        );
        assert_eq!(l.state.highest_serial, 20);
        assert!(t.path().join("previous.signed.json").exists());
    }

    #[test]
    fn an_older_signed_catalog_never_rolls_back() {
        let t = tempfile::tempdir().unwrap();
        let st = CatalogStore::new(t.path());
        let (k, pk) = key();
        let (s2, d2) = publish(&k, &sample(20));
        st.commit(&s2, &d2, &[&pk], APP, None).unwrap();
        let (s1, d1) = publish(&k, &sample(10));
        assert_eq!(
            st.accept_manifest(&s1, &[&pk], APP),
            Err(CatalogError::Rollback { got: 10, have: 20 })
        );
        assert!(st.commit(&s1, &d1, &[&pk], APP, None).is_err());
        assert_eq!(
            st.load(&[&pk], APP, &builtin())
                .unwrap()
                .manifest
                .unwrap()
                .serial,
            20
        );
    }

    #[test]
    fn damaged_cache_is_replaced_by_the_verified_previous_copy() {
        let t = tempfile::tempdir().unwrap();
        let st = CatalogStore::new(t.path());
        let (k, pk) = key();
        let (s1, d1) = publish(&k, &sample(10));
        let (s2, d2) = publish(&k, &sample(20));
        st.commit(&s1, &d1, &[&pk], APP, None).unwrap();
        st.commit(&s2, &d2, &[&pk], APP, None).unwrap();
        std::fs::write(t.path().join("current.data.json"), b"{garbage").unwrap();
        let l = st.load(&[&pk], APP, &builtin()).unwrap();
        assert!(l.repaired);
        assert_eq!(l.manifest.unwrap().serial, 10);
        // The current copy was restored from the verified previous one.
        let again = st.load(&[&pk], APP, &builtin()).unwrap();
        assert!(!again.repaired);
        // Both damaged: the built-in snapshot.
        std::fs::write(t.path().join("current.data.json"), b"x").unwrap();
        std::fs::write(t.path().join("previous.data.json"), b"x").unwrap();
        assert_eq!(
            st.load(&[&pk], APP, &builtin()).unwrap().origin,
            CatalogOrigin::Builtin
        );
    }

    #[test]
    fn a_failed_fetch_changes_nothing_but_the_status() {
        let t = tempfile::tempdir().unwrap();
        let st = CatalogStore::new(t.path());
        let (k, pk) = key();
        let (s, d) = publish(&k, &sample(10));
        st.commit(&s, &d, &[&pk], APP, None).unwrap();
        st.record_failure("сервер недоступен", true);
        let l = st.load(&[&pk], APP, &builtin()).unwrap();
        assert_eq!(l.manifest.unwrap().serial, 10);
        assert!(l.state.offline);
        assert_eq!(l.state.last_error.as_deref(), Some("сервер недоступен"));
        // A bad signature is rejected and leaves the cache as it was.
        let mut bad = s.clone();
        let i = bad.len() - 5;
        bad[i] ^= 1;
        assert!(st.commit(&bad, &d, &[&pk], APP, None).is_err());
        assert_eq!(
            st.load(&[&pk], APP, &builtin())
                .unwrap()
                .manifest
                .unwrap()
                .serial,
            10
        );
    }
}
