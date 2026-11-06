//! Update checks over the network (decisions live in `bridge_core::updates`).
//!
//! * One check of each kind at a time: a caller that arrives while a check
//!   runs waits for it and gets its result instead of starting another one.
//! * The startup check runs at most once per session and only when
//!   `checkUpdatesOnStart` is on; it never retries by itself.
//! * Every failure (no network, time-out, HTTP error, GitHub rate limit, a
//!   manifest that cannot be fetched) is reported as `Error`, never as
//!   "up to date".
//! * Nothing is downloaded or installed here except the small signed
//!   manifest, which is verified to decide whether Flowseal is installable.

use bridge_core::updates::{
    self as core, ManifestFetch, Release, UpdateInfo, MAX_MANIFEST_BYTES, MAX_RELEASES_BYTES,
};
use serde::Serialize;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

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
    /// Where signed Flowseal manifests are published.
    pub manifest_base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            api: "https://api.github.com".into(),
            manifest_base: core::MANIFEST_BASE.into(),
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
    }

    pub fn new(endpoints: Endpoints, keys: &[&str], app_version: &str) -> Self {
        UpdateCenter {
            endpoints,
            keys: keys.iter().map(|k| k.to_string()).collect(),
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
                match get(http, &url, MAX_RELEASES_BYTES).await {
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
                }
            })
            .await
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

#[cfg(test)]
mod tests;
