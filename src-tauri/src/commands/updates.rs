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
        let _ = app.emit("updates-changed", v);
        refresh_tray(&app, &state).await;
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
    if let Err(e) = zapret::install_flowseal(&state, &version).await {
        return Ok(FlowsealAction {
            ok: false,
            outcome: "installFailed".into(),
            message: format!("Flowseal {version} не установлен: {e}. Текущая версия не изменилась"),
            version,
        });
    }
    zapret::load_bundle(&state).await;
    let r = zapret::switch_bundle(&app, &state, &version).await;
    // The installed version changes what "newer" means.
    if r.ok {
        state
            .updates
            .check_flowseal(&state.http, flowseal_current(&state))
            .await;
        updates_changed(&app, &state);
    }
    Ok(r)
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

#[tauri::command]
pub async fn get_bundle(state: State<'_, AppState>) -> R<BundleState> {
    Ok(state.bundle.lock().expect("lock").clone())
}
