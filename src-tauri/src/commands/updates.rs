//! Update checks and Flowseal bundle install / activation.

use super::*;

pub(super) fn flowseal_current(state: &AppState) -> Option<String> {
    state.bundle.lock().expect("lock").version.clone()
}

pub(super) fn updates_changed(app: &AppHandle, state: &AppState) {
    let _ = app.emit("updates-changed", state.updates.view());
}

/// The last results of both checks (the startup check may have run while
/// the window was hidden).
#[tauri::command]
pub async fn updates_get(state: State<'_, AppState>) -> R<UpdatesView> {
    Ok(state.updates.view())
}

#[tauri::command]
pub async fn flowseal_check_update(app: AppHandle, state: State<'_, AppState>) -> R<UpdateInfo> {
    let r = state
        .updates
        .check_flowseal(&state.http, flowseal_current(&state))
        .await;
    updates_changed(&app, &state);
    Ok(r)
}

#[tauri::command]
pub async fn app_check_update(app: AppHandle, state: State<'_, AppState>) -> R<UpdateInfo> {
    let r = state.updates.check_app(&state.http).await;
    updates_changed(&app, &state);
    Ok(r)
}

/// Startup check: once per session, only with `checkUpdatesOnStart`, in the
/// background (the window may stay hidden), after the active Flowseal
/// version is known. It never retries on its own.
pub fn spawn_startup_update_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        let state = app.state::<AppState>();
        if !state
            .updates
            .claim_startup_check(state.settings().check_updates_on_start)
        {
            return;
        }
        // The helper may still be starting (Windows logon): wait up to ~60 s
        // for the active Flowseal version, then check with what is known.
        for _ in 0..30 {
            if flowseal_current(&state).is_some() || state.quitting.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        let v = state
            .updates
            .check_all(&state.http, flowseal_current(&state))
            .await;
        let status = |u: &Option<UpdateInfo>| u.as_ref().map(|u| u.status);
        tracing::info!(
            "startup update check: app {:?}, flowseal {:?}",
            status(&v.app),
            status(&v.flowseal)
        );
        let flowseal = v.flowseal.clone();
        let _ = app.emit("updates-changed", v);
        refresh_tray(&app, &state).await;
        auto_install_flowseal(&app, &state, flowseal.as_ref()).await;
    });
}

/// Install a verified Flowseal release, then switch to it transactionally
/// (the previous version stays installed for rollback). The signed manifest
/// and the archive are verified here and again inside the helper.
#[tauri::command]
pub async fn flowseal_install(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> R<FlowsealAction> {
    zapret::ensure_not_auto(&state)?;
    Ok(install_and_switch(&app, &state, version, false).await)
}

/// Install a verified Flowseal version and switch to it as a transaction.
/// With `only_if_compatible` (the automatic path) the switch is skipped when
/// a strategy of the user would break on the new version: the version stays
/// installed and the user decides in Settings.
pub(super) async fn install_and_switch(
    app: &AppHandle,
    state: &AppState,
    version: String,
    only_if_compatible: bool,
) -> FlowsealAction {
    let failed = |outcome: &str, message: String, version: String| FlowsealAction {
        ok: false,
        outcome: outcome.into(),
        message,
        version,
    };
    if let Err(e) = zapret::install_flowseal(state, &version).await {
        return failed(
            "installFailed",
            format!("Flowseal {version} не установлен: {e}. Текущая версия не изменилась"),
            version,
        );
    }
    zapret::load_bundle(state).await;
    if only_if_compatible {
        match zapret::compat_report(state, &version) {
            Ok(broken) if broken.is_empty() => {}
            Ok(broken) => {
                return failed(
                    "refused",
                    format!(
                        "Flowseal {version} скачан и проверен, но не включён автоматически: с ним перестанут работать ваши стратегии ({}). Переключиться можно в «Настройки → Обновления»",
                        broken.len()
                    ),
                    version,
                )
            }
            Err(e) => {
                return failed(
                    "refused",
                    format!("Flowseal {version} скачан и проверен, но не включён автоматически: {e}"),
                    version,
                )
            }
        }
    }
    let r = zapret::switch_bundle(app, state, &version).await;
    // The installed version changes what "newer" means.
    if r.ok {
        state
            .updates
            .check_flowseal(&state.http, flowseal_current(state))
            .await;
        updates_changed(app, state);
    }
    r
}

/// The automatic Flowseal update (setting `autoInstallFlowseal`, off by
/// default), once per session after the startup check. Only a version the
/// project verified — `installable` means a signed manifest passed the
/// pinned key — is installed; the switch is the usual transaction with a
/// rollback, and it is skipped while Auto runs, while quitting, or when a
/// strategy of the user would break. The result is shown to the user.
async fn auto_install_flowseal(app: &AppHandle, state: &AppState, info: Option<&UpdateInfo>) {
    let Some(version) = info
        .filter(|u| u.status == upd::UpdateStatus::Available && u.installable)
        .and_then(|u| u.latest.clone())
    else {
        return;
    };
    if !state.settings().auto_install_flowseal
        || state.quitting.load(Ordering::SeqCst)
        || zapret::ensure_not_auto(state).is_err()
    {
        return;
    }
    tracing::info!("automatic Flowseal update to {version}");
    let r = install_and_switch(app, state, version, true).await;
    tracing::info!("automatic Flowseal update: {} — {}", r.outcome, r.message);
    let _ = app.emit("flowseal-auto-update", r);
    refresh_tray(app, state).await;
}

#[tauri::command]
pub async fn flowseal_set_active(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> R<FlowsealAction> {
    Ok(zapret::switch_bundle(&app, &state, &version).await)
}

/// User strategies that would not work with Flowseal `version`.
#[tauri::command]
pub async fn flowseal_compat(
    state: State<'_, AppState>,
    version: String,
) -> R<Vec<zapret::CompatEntry>> {
    zapret::compat_report(&state, &version)
}

/// Installs the newer Bridge release found by the last check: downloads the
/// installer, checks it against the signed manifest, starts it and exits.
///
/// * Works only for a release whose manifest was verified with a pinned key
///   during the check; the UI passes nothing — neither a version nor a URL.
/// * The installer is fetched from the release of exactly that version; the
///   file is kept only with the signed size and SHA-256.
/// * Before the start the file is reopened so that nobody can change or
///   replace it, hashed again through that handle, and the handle is held
///   until this process exits.
/// * The installer asks for administrator rights itself (UAC). If it does
///   not start — for example the request was declined — Bridge keeps running.
#[tauri::command]
pub async fn app_update_install(app: AppHandle, state: State<'_, AppState>) -> R<()> {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let m = state.updates.app_release().ok_or(
        "для этой версии нет проверенного подписанного манифеста — установите её со страницы релиза",
    )?;
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("обновление уже скачивается".into());
    }
    let r = download_and_start(&app, &state, &m).await;
    RUNNING.store(false, Ordering::SeqCst);
    match &r {
        Ok(()) => {
            tracing::info!("installer of Bridge {} started, exiting", m.version);
            let a = app.clone();
            tauri::async_runtime::spawn(async move { quit(&a).await });
        }
        Err(e) => tracing::warn!("app update not installed: {e}"),
    }
    r
}

pub(super) async fn download_and_start(
    app: &AppHandle,
    state: &AppState,
    m: &bridge_core::apprelease::AppReleaseManifest,
) -> R<()> {
    let url = state.updates.installer_url(m)?;
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("нет каталога данных приложения: {e}"))?
        .join("updates");
    // Earlier downloads are never reused: every installation starts clean.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("не удалось создать каталог для установщика: {e}"))?;
    let dest = dir.join(&m.installer_name);
    bridge_updates::download_installer(&state.http, &url, m, &dest).await?;
    let locked = crate::platform::lock_and_verify(&dest, &m.installer_sha256)?;
    app.opener()
        .open_path(dest.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("установщик не запущен: {e}"))?;
    // Keep the file locked until this process exits.
    std::mem::forget(locked);
    Ok(())
}

#[tauri::command]
pub async fn get_bundle(state: State<'_, AppState>) -> R<BundleState> {
    Ok(state.bundle.lock().expect("lock").clone())
}
