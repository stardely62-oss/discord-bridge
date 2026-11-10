//! Update checks over the network (decisions live in `bridge_core::updates`).
//!
//! * One check of each kind at a time: a caller that arrives while a check
//!   runs waits for it and gets its result instead of starting another one.
//! * The startup check runs at most once per session and only when
//!   `checkUpdatesOnStart` is on; it never retries by itself.
//! * Every failure (no network, time-out, HTTP error, GitHub rate limit, a
//!   manifest that cannot be fetched) is reported as `Error`, never as
//!   "up to date".
//! * The checks download only the small signed manifests, which are
//!   verified to decide whether Flowseal or a Bridge release is installable.
//! * [`download_installer`] fetches a Bridge installer only on the user's
//!   request and only for a verified manifest; it keeps the file only if its
//!   size and SHA-256 are exactly the signed ones.

use bridge_core::apprelease::{self, AppReleaseManifest};
use bridge_core::updates::{
    self as core, ManifestFetch, Release, UpdateInfo, MAX_MANIFEST_BYTES, MAX_RELEASES_BYTES,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// The whole installer download; a stalled connection ends sooner (reqwest
/// read time-outs are per request, so this is the upper bound).
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone)]
pub struct Endpoints {
    /// GitHub REST API root.
    pub api: String,
    /// Where signed Flowseal and app-release manifests are published.
    pub manifest_base: String,
    /// Host of the release assets (the installer).
    pub download_base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            api: "https://api.github.com".into(),
            manifest_base: core::MANIFEST_BASE.into(),
            download_base: "https://github.com".into(),
        }
    }
}

#[derive(Default)]
struct Slot {
    run: tokio::sync::Mutex<()>,
    generation: AtomicU64,
    running: AtomicBool,
    last: Mutex<Option<UpdateInfo>>,
}

impl Slot {
    fn last(&self) -> Option<UpdateInfo> {
        self.last.lock().expect("lock").clone()
    }

    /// Runs `f` unless a check finished while we waited for the running one.
    async fn dedup<F: Future<Output = UpdateInfo>>(&self, f: impl FnOnce() -> F) -> UpdateInfo {
        let seen = self.generation.load(Ordering::SeqCst);
        let _guard = self.run.lock().await;
        if self.generation.load(Ordering::SeqCst) != seen {
            if let Some(l) = self.last() {
                return l;
            }
        }
        self.running.store(true, Ordering::SeqCst);
        let r = f().await;
        *self.last.lock().expect("lock") = Some(r.clone());
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        r
    }
}

/// What the UI shows: the last result of each check and whether one runs.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesView {
    pub app: Option<UpdateInfo>,
    pub flowseal: Option<UpdateInfo>,
    pub checking_app: bool,
    pub checking_flowseal: bool,
}

pub struct UpdateCenter {
    endpoints: Endpoints,
    keys: Vec<String>,
    /// Keys for app-release manifests; empty = installing from the app is off.
    app_keys: Vec<String>,
    /// Verified manifest of the release the last app check found, if any.
    app_release: Mutex<Option<AppReleaseManifest>>,
    app_version: String,
    app: Slot,
    flowseal: Slot,
    startup_done: AtomicBool,
}

impl UpdateCenter {
    pub fn production() -> Self {
        Self::new(
            Endpoints::default(),
            bridge_core::manifest::TRUSTED_KEYS,
            env!("CARGO_PKG_VERSION"),
        )
        .with_app_keys(apprelease::APP_RELEASE_TRUSTED_KEYS)
    }

    pub fn with_app_keys(mut self, keys: &[&str]) -> Self {
        self.app_keys = keys.iter().map(|k| k.to_string()).collect();
        self
    }

    /// The verified manifest of the newer release found by the last app
    /// check. `None` whenever installing from the app is not possible.
    pub fn app_release(&self) -> Option<AppReleaseManifest> {
        self.app_release.lock().expect("lock").clone()
    }

    pub fn installer_url(&self, m: &AppReleaseManifest) -> Result<String, String> {
        let v = apprelease::validate(m)?;
        Ok(apprelease::installer_url(&self.endpoints.download_base, &v))
    }

    pub fn new(endpoints: Endpoints, keys: &[&str], app_version: &str) -> Self {
        UpdateCenter {
            endpoints,
            keys: keys.iter().map(|k| k.to_string()).collect(),
            app_keys: Vec::new(),
            app_release: Mutex::new(None),
            app_version: app_version.into(),
            app: Slot::default(),
            flowseal: Slot::default(),
            startup_done: AtomicBool::new(false),
        }
    }

    pub fn view(&self) -> UpdatesView {
        UpdatesView {
            app: self.app.last(),
            flowseal: self.flowseal.last(),
            checking_app: self.app.running.load(Ordering::SeqCst),
            checking_flowseal: self.flowseal.running.load(Ordering::SeqCst),
        }
    }

    pub async fn check_app(&self, http: &reqwest::Client) -> UpdateInfo {
        self.app
            .dedup(move || async move {
                let url = format!(
                    "{}/repos/{}/releases?per_page=100",
                    self.endpoints.api,
                    core::APP_REPO
                );
                let t = now();
                let mut info = match get(http, &url, MAX_RELEASES_BYTES).await {
                    Ok(Some(body)) => match serde_json::from_slice::<Vec<Release>>(&body) {
                        Ok(list) => core::evaluate_app(&self.app_version, &list, t),
                        Err(e) => UpdateInfo::failed(
                            Some(self.app_version.clone()),
                            format!("непонятный ответ GitHub: {e}"),
                            t,
                        ),
                    },
                    Ok(None) => UpdateInfo::failed(
                        Some(self.app_version.clone()),
                        core::http_error(404, None, None, t),
                        t,
                    ),
                    Err(e) => UpdateInfo::failed(Some(self.app_version.clone()), e, t),
                };
                let release = self.app_manifest(http, &mut info).await;
                *self.app_release.lock().expect("lock") = release;
                info
            })
            .await
    }

    /// The signed manifest of a newer release. Requested only when a key is
    /// pinned and the release list named a newer version.
    async fn app_manifest(
        &self,
        http: &reqwest::Client,
        info: &mut UpdateInfo,
    ) -> Option<AppReleaseManifest> {
        if self.app_keys.is_empty() || info.status != core::UpdateStatus::Available {
            return None;
        }
        let version = semver::Version::parse(info.latest.as_deref()?).ok()?;
        let url = apprelease::app_manifest_url(&self.endpoints.manifest_base, &version);
        let fetch = match get(http, &url, MAX_MANIFEST_BYTES + 1).await {
            Ok(Some(b)) => ManifestFetch::Bytes(b),
            Ok(None) => ManifestFetch::NotFound,
            Err(e) => ManifestFetch::Failed(e),
        };
        let keys: Vec<&str> = self.app_keys.iter().map(String::as_str).collect();
        core::apply_app_manifest(info, fetch, &keys)
    }

    /// `current`: the active Flowseal version (None if unknown).
    pub async fn check_flowseal(
        &self,
        http: &reqwest::Client,
        current: Option<String>,
    ) -> UpdateInfo {
        self.flowseal
            .dedup(move || async move {
                let url = format!(
                    "{}/repos/{}/releases/latest",
                    self.endpoints.api,
                    core::FLOWSEAL_REPO
                );
                let t = now();
                let release = match get(http, &url, MAX_RELEASES_BYTES).await {
                    Ok(Some(body)) => match serde_json::from_slice::<Release>(&body) {
                        Ok(r) => r,
                        Err(e) => {
                            return UpdateInfo::failed(
                                current,
                                format!("непонятный ответ GitHub: {e}"),
                                t,
                            )
                        }
                    },
                    Ok(None) => {
                        return UpdateInfo::failed(current, core::http_error(404, None, None, t), t)
                    }
                    Err(e) => return UpdateInfo::failed(current, e, t),
                };
                let mut info = core::evaluate_flowseal_release(current.as_deref(), &release, t);
                if info.status == core::UpdateStatus::Available {
                    let version = info.latest.clone().unwrap_or_default();
                    let fetch = match get(
                        http,
                        &core::manifest_url(&self.endpoints.manifest_base, &version),
                        MAX_MANIFEST_BYTES + 1,
                    )
                    .await
                    {
                        Ok(Some(b)) => ManifestFetch::Bytes(b),
                        Ok(None) => ManifestFetch::NotFound,
                        Err(e) => ManifestFetch::Failed(e),
                    };
                    let keys: Vec<&str> = self.keys.iter().map(String::as_str).collect();
                    core::apply_manifest(&mut info, fetch, &self.app_version, &keys);
                }
                info
            })
            .await
    }

    /// True exactly once per session, and only if the setting is on.
    pub fn claim_startup_check(&self, enabled: bool) -> bool {
        enabled && !self.startup_done.swap(true, Ordering::SeqCst)
    }

    /// Both checks concurrently (the startup check).
    pub async fn check_all(
        &self,
        http: &reqwest::Client,
        flowseal_current: Option<String>,
    ) -> UpdatesView {
        tokio::join!(
            self.check_app(http),
            self.check_flowseal(http, flowseal_current)
        );
        self.view()
    }
}

fn network_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "GitHub не ответил вовремя (тайм-аут)".into()
    } else if e.is_connect() {
        "нет соединения с GitHub — проверьте интернет".into()
    } else {
        format!("сетевая ошибка: {e}")
    }
}

/// GET with a time-out and a size limit. `Ok(None)`: HTTP 404.
async fn get(http: &reqwest::Client, url: &str, limit: usize) -> Result<Option<Vec<u8>>, String> {
    let mut r = http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|e| network_error(&e))?;
    let status = r.status().as_u16();
    if status == 404 {
        return Ok(None);
    }
    if !r.status().is_success() {
        let h = |n: &str| {
            r.headers()
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let remaining = h("x-ratelimit-remaining");
        let reset = h("x-ratelimit-reset").and_then(|v| v.parse().ok());
        return Err(core::http_error(status, remaining.as_deref(), reset, now()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = r.chunk().await.map_err(|e| network_error(&e))? {
        body.extend_from_slice(&chunk);
        if body.len() > limit {
            break;
        }
    }
    Ok(Some(body))
}

/// Downloads the installer described by a verified manifest into the new
/// file `dest`. The file is kept only if exactly `installer_size` bytes
/// arrived and their SHA-256 is the signed one; in every other case it is
/// removed and an error is returned. More bytes than signed are never
/// written.
pub async fn download_installer(
    http: &reqwest::Client,
    url: &str,
    m: &AppReleaseManifest,
    dest: &Path,
) -> Result<(), String> {
    apprelease::validate(m)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|e| format!("не удалось создать файл установщика: {e}"))?;
    let r = async {
        let mut resp = http
            .get(url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .map_err(|e| network_error(&e))?;
        if !resp.status().is_success() {
            return Err(format!(
                "установщик не скачан: сервер ответил {}",
                resp.status().as_u16()
            ));
        }
        let mut hash = Sha256::new();
        let mut size: u64 = 0;
        while let Some(chunk) = resp.chunk().await.map_err(|e| network_error(&e))? {
            size += chunk.len() as u64;
            if size > m.installer_size {
                return Err("установщик больше, чем указано в подписанном манифесте".into());
            }
            hash.update(&chunk);
            file.write_all(&chunk)
                .map_err(|e| format!("не удалось записать установщик: {e}"))?;
        }
        if size != m.installer_size {
            return Err("установщик скачан не полностью".into());
        }
        let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != m.installer_sha256 {
            return Err("SHA-256 установщика не совпадает с подписанным манифестом".into());
        }
        file.sync_all()
            .map_err(|e| format!("не удалось записать установщик: {e}"))
    }
    .await;
    drop(file);
    if r.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    r
}

#[cfg(test)]
mod tests;
