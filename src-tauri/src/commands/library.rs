//! Strategy library: list, edit, import, export, compare.

use super::*;

#[tauri::command]
pub async fn list_strategies(state: State<'_, AppState>) -> R<Vec<StrategyListItem>> {
    Ok(state.strategies.lock().expect("lock").list())
}

#[tauri::command]
pub async fn get_strategy(state: State<'_, AppState>, id: String) -> R<StrategyDetails> {
    let gf = state.settings().game_filter;
    let sm = state.strategies.lock().expect("lock");
    let s = sm.get(&id).ok_or("стратегия не найдена")?;
    Ok(sm.details(s, &gf))
}

#[tauri::command]
pub async fn preview_strategy(
    state: State<'_, AppState>,
    strategy: Strategy,
) -> R<StrategyDetails> {
    let gf = state.settings().game_filter;
    Ok(state
        .strategies
        .lock()
        .expect("lock")
        .details(&strategy, &gf))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorParse {
    pub ok: bool,
    pub details: Option<StrategyDetails>,
    pub error: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[tauri::command]
pub async fn parse_strategy_json(state: State<'_, AppState>, text: String) -> R<EditorParse> {
    match StrategyManager::parse_editor(&text) {
        Ok(s) => {
            let gf = state.settings().game_filter;
            let d = state.strategies.lock().expect("lock").details(&s, &gf);
            Ok(EditorParse {
                ok: true,
                details: Some(d),
                error: None,
                line: None,
                column: None,
            })
        }
        Err((msg, line, column)) => Ok(EditorParse {
            ok: false,
            details: None,
            error: Some(msg),
            line,
            column,
        }),
    }
}

pub(super) fn after_strategy_change(app: &AppHandle) {
    let _ = app.emit("strategies-changed", ());
}

#[tauri::command]
pub async fn save_strategy(
    app: AppHandle,
    state: State<'_, AppState>,
    strategy: Strategy,
) -> R<StrategyDetails> {
    let gf = state.settings().game_filter;
    let d = {
        let mut sm = state.strategies.lock().expect("lock");
        let s = sm.save(strategy)?;
        sm.details(&s, &gf)
    };
    after_strategy_change(&app);
    Ok(d)
}

#[tauri::command]
pub async fn create_strategy(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> R<Strategy> {
    let s = state.strategies.lock().expect("lock").create(&name)?;
    after_strategy_change(&app);
    Ok(s)
}

#[tauri::command]
pub async fn duplicate_strategy(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
) -> R<Strategy> {
    let s = state
        .strategies
        .lock()
        .expect("lock")
        .duplicate(&id, name)?;
    after_strategy_change(&app);
    Ok(s)
}

#[tauri::command]
pub async fn rename_strategy(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> R<Strategy> {
    let s = state.strategies.lock().expect("lock").rename(&id, &name)?;
    after_strategy_change(&app);
    Ok(s)
}

#[tauri::command]
pub async fn delete_strategy(app: AppHandle, state: State<'_, AppState>, id: String) -> R<()> {
    let running = zapret::engine_status(&state)
        .await
        .ok()
        .filter(|s| s.state == EngineState::Running)
        .and_then(|s| s.strategy_id);
    if running.as_deref() == Some(id.as_str()) {
        return Err("нельзя удалить активную стратегию — сначала переключитесь на другую или выключите обход".into());
    }
    state.strategies.lock().expect("lock").delete(&id)?;
    if state.settings().active_strategy_id.as_deref() == Some(id.as_str()) {
        state.update_settings(|s| s.active_strategy_id = Some("flowseal-general".into()))?;
    }
    after_strategy_change(&app);
    Ok(())
}

#[tauri::command]
pub async fn restore_original(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> R<Strategy> {
    let s = state
        .strategies
        .lock()
        .expect("lock")
        .restore_original(&id)?;
    after_strategy_change(&app);
    Ok(s)
}

#[tauri::command]
pub async fn compare_strategies(
    state: State<'_, AppState>,
    left: String,
    right: String,
) -> R<StrategyDiff> {
    let gf = state.settings().game_filter;
    state
        .strategies
        .lock()
        .expect("lock")
        .compare(&left, &right, &gf)
}

#[tauri::command]
pub async fn import_preview(state: State<'_, AppState>, path: String) -> R<ImportPreview> {
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if meta.len() > crate::strategies::MAX_IMPORT_SIZE as u64 {
        return Err("файл слишком большой".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let content = match String::from_utf8(bytes) {
        Ok(s) => s,
        // Old BAT files are often in a legacy code page; Latin parts survive.
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    state
        .strategies
        .lock()
        .expect("lock")
        .preview_import(&name, &content)
}

#[tauri::command]
pub async fn import_commit(
    app: AppHandle,
    state: State<'_, AppState>,
    token: String,
    name: Option<String>,
    accept_partial: bool,
) -> R<Strategy> {
    let saved =
        state
            .strategies
            .lock()
            .expect("lock")
            .commit_import(&token, name, accept_partial)?;
    tracing::info!("imported strategy {}", saved.id);
    after_strategy_change(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn create_discord_only(app: AppHandle, state: State<'_, AppState>) -> R<Strategy> {
    let s = state
        .strategies
        .lock()
        .expect("lock")
        .create_discord_only()?;
    after_strategy_change(&app);
    Ok(s)
}

#[tauri::command]
pub async fn export_strategy(
    state: State<'_, AppState>,
    id: String,
    format: String,
    path: String,
) -> R<()> {
    let text = state
        .strategies
        .lock()
        .expect("lock")
        .export(&id, &format)?;
    bridge_core::atomic::write_atomic(std::path::Path::new(&path), text.as_bytes())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_option_specs() -> Vec<OptionSpec> {
    option_specs()
}

#[derive(Serialize)]
pub struct Resources {
    pub fakes: Vec<String>,
    pub lists: Vec<String>,
}

#[tauri::command]
pub async fn get_resources(state: State<'_, AppState>) -> R<Resources> {
    let sm = state.strategies.lock().expect("lock");
    Ok(Resources {
        fakes: sm.resources.fakes.iter().cloned().collect(),
        lists: sm.resources.lists.iter().cloned().collect(),
    })
}
