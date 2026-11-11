//! Remote catalog: network side, in-memory state and Tauri commands.
//!
//! - The signed manifest comes from one fixed URL; the data file is fetched
//!   only from the release of the same serial (`catalog-<serial>`), never
//!   from a URL found in a document.
//! - Every response is size-limited while it is read and time-limited.
//! - Verification and the cache live in `bridge_core::packs` (store). A
//!   failed fetch, a bad signature or a damaged cache never changes settings
//!   or the running engine: the app keeps the last verified catalog, then the
//!   built-in snapshot.
//! - Installing, updating, rolling back or removing a pack only changes
//!   `installedPacks` through the validated, atomic settings update. The
//!   engine is never restarted here: `pendingApply` tells the user.
//! - Install / update is bound to the exact snapshot the user confirmed
//!   ([`PackConfirm`]: id, version, catalog serial, SHA-256 of the pack and
//!   of the installed copy). The check and the write happen under the
//!   settings lock against one catalog state, and the stored data is the
//!   very pack value whose hash was checked; a stale confirmation changes
//!   nothing and asks the user to review again.

use crate::configuration::ConfigurationManager;
use crate::state::AppState;
use bridge_core::lists::InstalledPack;
use bridge_core::packs::store::{CatalogOrigin, CatalogStore, Loaded};
use bridge_core::packs::{
    self, Pack, PackConfirm, PackDiff, PackState, CATALOG_TRUSTED_KEYS, MAX_DATA_BYTES,
    MAX_MANIFEST_BYTES,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

type R<T> = Result<T, String>;

pub const MANIFEST_URL: &str =
    "https://github.com/stardely62-oss/discord-bridge-catalog/releases/latest/download/catalog.signed.json";
pub const DATA_URL_BASE: &str =
    "https://github.com/stardely62-oss/discord-bridge-catalog/releases/download";
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 3600);
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const BUILTIN: &[u8] = include_bytes!("../../resources/catalog/builtin-catalog.json");

#[derive(Debug, Clone)]
pub struct Endpoints {
    pub manifest: String,
    pub data_base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            manifest: MANIFEST_URL.into(),
            data_base: DATA_URL_BASE.into(),
        }
    }
}

impl Endpoints {
    pub fn data_url(&self, serial: u64) -> String {
        format!("{}/catalog-{serial}/catalog-data.json", self.data_base)
    }
}

pub struct CatalogRuntime {
    store: CatalogStore,
    endpoints: Endpoints,
    keys: Vec<String>,
    app_version: String,
    loaded: Mutex<Loaded>,
    syncing: tokio::sync::Mutex<()>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncOutcome {
    Updated(u64),
    Unchanged,
    Failed { error: String, offline: bool },
}

struct FetchError {
    msg: String,
    offline: bool,
}

enum Fetched {
    NotModified,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

async fn get_limited(
    http: &reqwest::Client,
    url: &str,
    etag: Option<&str>,
    limit: usize,
) -> Result<Fetched, FetchError> {
    let mut req = http.get(url).timeout(FETCH_TIMEOUT);
    if let Some(e) = etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, e);
    }
    let mut resp = req.send().await.map_err(|e| FetchError {
        offline: e.is_connect() || e.is_timeout() || e.is_request(),
        msg: if e.is_timeout() {
            "сервер каталога не ответил вовремя".into()
        } else {
            format!("сервер каталога недоступен: {e}")
        },
    })?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Fetched::NotModified);
    }
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(FetchError {
            msg: "каталог ещё не опубликован".into(),
            offline: false,
        });
    }
    if !status.is_success() {
        return Err(FetchError {
            msg: format!("сервер каталога ответил HTTP {}", status.as_u16()),
            offline: false,
        });
    }
    if resp.content_length().is_some_and(|n| n > limit as u64) {
        return Err(FetchError {
            msg: "ответ сервера каталога больше допустимого".into(),
            offline: false,
        });
    }
    let etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let mut bytes = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(c)) => {
                if bytes.len() + c.len() > limit {
                    return Err(FetchError {
                        msg: "ответ сервера каталога больше допустимого".into(),
                        offline: false,
                    });
                }
                bytes.extend_from_slice(&c);
            }
            Ok(None) => break,
            Err(e) => {
                return Err(FetchError {
                    msg: format!("загрузка каталога прервана: {e}"),
                    offline: e.is_timeout(),
                })
            }
        }
    }
    Ok(Fetched::Body { bytes, etag })
}

impl CatalogRuntime {
    pub fn new(
        dir: std::path::PathBuf,
        endpoints: Endpoints,
        keys: &[&str],
        app_version: &str,
    ) -> Self {
        let store = CatalogStore::new(dir);
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        let loaded = store
            .load(&refs, app_version, BUILTIN)
            .expect("the built-in catalog is validated by the test suite");
        if loaded.repaired {
            tracing::warn!("catalog cache was damaged; the verified previous copy was restored");
        }
        CatalogRuntime {
            store,
            endpoints,
            keys,
            app_version: app_version.into(),
            loaded: Mutex::new(loaded),
            syncing: tokio::sync::Mutex::new(()),
        }
    }

    pub fn production(dir: std::path::PathBuf) -> Self {
        Self::new(dir, Endpoints::default(), CATALOG_TRUSTED_KEYS, APP_VERSION)
    }

    fn keys(&self) -> Vec<&str> {
        self.keys.iter().map(String::as_str).collect()
    }

    fn reload(&self) {
        if let Ok(l) = self.store.load(&self.keys(), &self.app_version, BUILTIN) {
            *self.loaded.lock().expect("catalog lock") = l;
        }
    }

    pub fn loaded(&self) -> Loaded {
        self.loaded.lock().expect("catalog lock").clone()
    }

    /// One check against the server. Never touches settings or the engine.
    pub async fn sync(&self, http: &reqwest::Client) -> SyncOutcome {
        let _one = self.syncing.lock().await;
        let keys = self.keys();
        let have_remote = self.loaded().origin == CatalogOrigin::Remote;
        let etag = if have_remote {
            self.store.state().etag
        } else {
            None
        };
        let fail = |e: String, offline: bool| {
            self.store.record_failure(&e, offline);
            self.reload();
            SyncOutcome::Failed { error: e, offline }
        };
        let (signed, etag) = match get_limited(
            http,
            &self.endpoints.manifest,
            etag.as_deref(),
            MAX_MANIFEST_BYTES,
        )
        .await
        {
            Ok(Fetched::NotModified) => {
                self.store.record_unchanged(None);
                self.reload();
                return SyncOutcome::Unchanged;
            }
            Ok(Fetched::Body { bytes, etag }) => (bytes, etag),
            Err(e) => return fail(e.msg, e.offline),
        };
        let manifest = match self
            .store
            .accept_manifest(&signed, &keys, &self.app_version)
        {
            Ok(None) => {
                self.store.record_unchanged(etag);
                self.reload();
                return SyncOutcome::Unchanged;
            }
            Ok(Some(m)) => m,
            Err(e) => return fail(e.to_string(), false),
        };
        let data = match get_limited(
            http,
            &self.endpoints.data_url(manifest.serial),
            None,
            MAX_DATA_BYTES,
        )
        .await
        {
            Ok(Fetched::Body { bytes, .. }) => bytes,
            Ok(Fetched::NotModified) => {
                return fail("сервер каталога не отдал данные".into(), false)
            }
            Err(e) => return fail(e.msg, e.offline),
        };
        match self
            .store
            .commit(&signed, &data, &keys, &self.app_version, etag)
        {
            Ok(m) => {
                self.reload();
                tracing::info!("catalog {} verified and cached", m.serial);
                SyncOutcome::Updated(m.serial)
            }
            Err(e) => fail(e.to_string(), false),
        }
    }

    /// Exact diff of pack `id` against `installed`, with the confirmation
    /// token of the same catalog state.
    pub fn diff(&self, installed: &[InstalledPack], id: &str) -> Option<PackDiff> {
        let l = self.loaded.lock().expect("catalog lock");
        let p = l
            .data
            .usable_packs(&self.app_version)
            .into_iter()
            .find(|p| p.id == id)?;
        Some(packs::pack_diff(
            l.data.serial,
            installed.iter().find(|i| i.id == id),
            p,
        ))
    }

    /// The new `installedPacks` with exactly the confirmed snapshot, or a
    /// stale / invalid confirmation error. Reads one catalog state under its
    /// lock; the caller holds the settings lock around this call.
    pub fn install_confirmed(
        &self,
        installed: &[InstalledPack],
        c: &PackConfirm,
    ) -> R<Vec<InstalledPack>> {
        let l = self.loaded.lock().expect("catalog lock");
        let Some(p) = l
            .data
            .usable_packs(&self.app_version)
            .into_iter()
            .find(|p| p.id == c.id)
        else {
            return Err(format!(
                "набора «{}» нет в проверенном каталоге — обновите информацию",
                c.id.chars().take(40).collect::<String>()
            ));
        };
        packs::install_confirmed(installed, l.data.serial, p, c).map_err(|e| e.to_string())
    }

    /// The verified catalog pack `id` usable by this app version (tests;
    /// the app installs only through [`Self::install_confirmed`]).
    #[cfg(test)]
    pub fn pack(&self, id: &str) -> Option<Pack> {
        let l = self.loaded();
        l.data
            .usable_packs(&self.app_version)
            .into_iter()
            .find(|p| p.id == id)
            .cloned()
    }

    /// Usable packs whose own ports can be checked against the strategy:
    /// with `ports`, or with `requires.gameFilter` (older catalog data).
    pub fn port_packs(&self) -> Vec<Pack> {
        self.loaded()
            .data
            .usable_packs(&self.app_version)
            .into_iter()
            .filter(|p| p.ports.is_some() || p.requires.game_filter)
            .cloned()
            .collect()
    }

    pub fn view(&self, installed: &[InstalledPack]) -> CatalogView {
        let l = self.loaded();
        let packs: Vec<PackView> = l
            .data
            .usable_packs(&self.app_version)
            .into_iter()
            .map(|p| {
                let state = packs::pack_state(p, installed);
                let inst = installed.iter().find(|i| i.id == p.id).cloned();
                let diff = matches!(state, PackState::UpdateAvailable | PackState::Differs)
                    .then(|| packs::pack_diff(l.data.serial, inst.as_ref(), p));
                PackView {
                    confirm: PackConfirm::of(l.data.serial, p, installed),
                    pack: p.clone(),
                    state,
                    installed: inst,
                    diff,
                }
            })
            .collect();
        let orphans = installed
            .iter()
            .filter(|i| !packs.iter().any(|p| p.pack.id == i.id))
            .cloned()
            .collect();
        let icons = l
            .data
            .icons
            .iter()
            .map(|i| {
                let mime = if i.name.ends_with(".png") {
                    "image/png"
                } else {
                    "image/webp"
                };
                (i.name.clone(), format!("data:{mime};base64,{}", i.data))
            })
            .collect();
        CatalogView {
            origin: l.origin,
            serial: l.manifest.as_ref().map(|m| m.serial),
            published: l.manifest.as_ref().map(|m| m.published.clone()),
            last_check: l.state.last_check,
            last_success: l.state.last_success,
            error: l.state.last_error.clone(),
            offline: l.state.offline,
            repaired: l.repaired,
            data_serial: l.data.serial,
            packs,
            orphans,
            icons,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackView {
    pub pack: Pack,
    pub state: PackState,
    pub installed: Option<InstalledPack>,
    /// Exact address changes against the installed snapshot (update/differs).
    pub diff: Option<PackDiff>,
    /// Token for installing exactly this shown snapshot.
    pub confirm: PackConfirm,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub origin: CatalogOrigin,
    pub serial: Option<u64>,
    pub published: Option<String>,
    pub last_check: Option<u64>,
    pub last_success: Option<u64>,
    pub error: Option<String>,
    pub offline: bool,
    pub repaired: bool,
    /// Serial of the catalog in use (remote or built-in).
    pub data_serial: u64,
    pub packs: Vec<PackView>,
    /// Installed packs no longer in the published catalog (kept locally).
    pub orphans: Vec<InstalledPack>,
    /// Icon name → data URL (PNG / WebP, verified).
    pub icons: BTreeMap<String, String>,
}

fn installed(state: &AppState) -> Vec<InstalledPack> {
    state.settings().installed_packs
}

/// Validated, atomic change of `installedPacks` only. `f` runs under the
/// settings lock; on any error nothing is written.
fn save_packs_in(
    config: &Mutex<ConfigurationManager>,
    f: impl FnOnce(&[InstalledPack]) -> R<Vec<InstalledPack>>,
) -> R<()> {
    let mut cfg = config.lock().expect("config lock");
    let mut s = cfg.settings.clone();
    s.installed_packs = f(&s.installed_packs)?;
    cfg.update(s)
}

fn save_packs(
    state: &AppState,
    f: impl FnOnce(&[InstalledPack]) -> R<Vec<InstalledPack>>,
) -> R<()> {
    save_packs_in(&state.config, f)
}

/// Check + save of a confirmed snapshot in one settings critical section.
pub(crate) fn install_confirmed_in(
    config: &Mutex<ConfigurationManager>,
    catalog: &CatalogRuntime,
    c: &PackConfirm,
) -> R<()> {
    save_packs_in(config, |cur| catalog.install_confirmed(cur, c))
}

#[tauri::command]
pub fn catalog_get(state: State<'_, AppState>) -> CatalogView {
    state.catalog.view(&installed(&state))
}

#[tauri::command]
pub async fn catalog_refresh(app: AppHandle, state: State<'_, AppState>) -> R<CatalogView> {
    let outcome = state.catalog.sync(&state.http).await;
    if matches!(outcome, SyncOutcome::Updated(_)) {
        let _ = app.emit("catalog-changed", ());
    }
    Ok(state.catalog.view(&installed(&state)))
}

/// Installs or updates a pack to exactly the snapshot the user saw and
/// confirmed (`confirm` from `PackView` / `PackDiff`). A stale confirmation
/// changes nothing; the engine is never restarted here.
#[tauri::command]
pub fn pack_install(
    app: AppHandle,
    state: State<'_, AppState>,
    confirm: PackConfirm,
) -> R<CatalogView> {
    install_confirmed_in(&state.config, &state.catalog, &confirm)?;
    let _ = app.emit("status-changed", ());
    Ok(state.catalog.view(&installed(&state)))
}

#[tauri::command]
pub fn pack_remove(app: AppHandle, state: State<'_, AppState>, id: String) -> R<CatalogView> {
    save_packs(&state, |cur| Ok(packs::remove(cur, &id)))?;
    let _ = app.emit("status-changed", ());
    Ok(state.catalog.view(&installed(&state)))
}

#[tauri::command]
pub fn pack_rollback(app: AppHandle, state: State<'_, AppState>, id: String) -> R<CatalogView> {
    save_packs(&state, |cur| {
        packs::rollback(cur, &id).map_err(|e| e.to_string())
    })?;
    let _ = app.emit("status-changed", ());
    Ok(state.catalog.view(&installed(&state)))
}

#[tauri::command]
pub fn pack_diff(state: State<'_, AppState>, id: String) -> R<PackDiff> {
    state
        .catalog
        .diff(&installed(&state), &id)
        .ok_or_else(|| "набора нет в проверенном каталоге".into())
}

/// Background check: shortly after start (if enabled), then periodically.
pub fn spawn_periodic(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        tokio::time::sleep(Duration::from_secs(15)).await;
        let mut first = true;
        loop {
            let state = app.state::<AppState>();
            if state.quitting.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            if !first || state.settings().check_updates_on_start {
                if let SyncOutcome::Updated(_) = state.catalog.sync(&state.http).await {
                    let _ = app.emit("catalog-changed", ());
                }
            }
            first = false;
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests;
