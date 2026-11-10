//! Turning the bypass on and off, Auto, engine log.

use super::*;

#[tauri::command]
pub async fn bypass_enable(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<String>,
    ignore_conflicts: bool,
) -> R<ActivationResult> {
    zapret::ensure_not_auto(&state)?;
    let id = id
        .or(state.settings().active_strategy_id)
        .unwrap_or_else(|| "flowseal-general".into());
    let r = zapret::activate(&app, &state, &id, ignore_conflicts, true).await;
    refresh_tray(&app, &state).await;
    Ok(r)
}

#[tauri::command]
pub async fn bypass_disable(app: AppHandle, state: State<'_, AppState>) -> R<()> {
    zapret::ensure_not_auto(&state)?;
    let r = zapret::deactivate(&app, &state).await;
    refresh_tray(&app, &state).await;
    r
}

#[tauri::command]
pub async fn select_strategy(app: AppHandle, state: State<'_, AppState>, id: String) -> R<()> {
    zapret::ensure_not_auto(&state)?;
    if state.strategies.lock().expect("lock").get(&id).is_none() {
        return Err("стратегия не найдена".into());
    }
    state.update_settings(|s| s.active_strategy_id = Some(id))?;
    refresh_tray(&app, &state).await;
    Ok(())
}

#[tauri::command]
pub async fn auto_start(app: AppHandle, state: State<'_, AppState>) -> R<AutoProgress> {
    let r = zapret::auto_select(app.clone(), &state).await;
    refresh_tray(&app, &state).await;
    Ok(r)
}

#[tauri::command]
pub async fn auto_cancel(state: State<'_, AppState>) -> R<()> {
    state.auto.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn engine_log(state: State<'_, AppState>) -> R<Vec<String>> {
    match state
        .helper
        .request(Request::EngineLog)
        .await
        .map_err(|e| e.to_string())?
    {
        Response::EngineLog { lines } => {
            Ok(lines.into_iter().map(|l| state.logs.redact(&l)).collect())
        }
        _ => Err("неожиданный ответ службы".into()),
    }
}
