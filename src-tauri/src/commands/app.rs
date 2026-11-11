//! Overview of the whole application state, tray refresh and full exit.

use super::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperInfo {
    pub available: bool,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackPortStatus {
    pub id: String,
    pub ports: Option<bridge_core::packs::PackPorts>,
    pub coverage: bridge_core::services::PortCoverage,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub app_version: String,
    pub settings: Settings,
    pub discord: DiscordStatus,
    pub helper: HelperInfo,
    pub engine: Option<EngineStatus>,
    pub bundle: BundleState,
    pub active_strategy: Option<StrategyListItem>,
    pub last_diagnostics: Option<DiagnosticsReport>,
    pub voice: VoiceState,
    /// Actual Windows autostart registration (not just the setting).
    pub autostart_enabled: bool,
    pub auto_running: bool,
    pub settings_recovered: bool,
    pub strategy_load_errors: Vec<String>,
    pub catalog_errors: Vec<String>,
    /// The engine runs, but its configuration (strategy, lists, game filter,
    /// ipset mode) differs from the saved one: a restart applies the changes.
    pub pending_apply: bool,
    /// Honest state of the selected IPs (see `bridge_core::ipset::IpSetStatus`).
    pub ip_sets: bridge_core::ipset::IpSetStatus,
    /// Whether the active strategy handles the own ports of catalog packs
    /// (configuration only, never "works").
    pub pack_ports: Vec<PackPortStatus>,
}

/// True if the running engine's snapshot hash differs from the hash of the
/// same strategy with the current settings (both use `config_hash`).
pub(crate) fn pending_apply(state: &AppState, engine: Option<&EngineStatus>) -> bool {
    let Some(e) = engine.filter(|e| e.state == EngineState::Running) else {
        return false;
    };
    let (Some(id), Some(running)) = (e.strategy_id.as_deref(), e.config_hash.as_deref()) else {
        return false;
    };
    let Some(s) = state.strategies.lock().expect("lock").get(id).cloned() else {
        return false;
    };
    let Some(version) = state.bundle.lock().expect("lock").version.clone() else {
        return false;
    };
    bridge_core::strategy::config_hash(&s, &zapret::runtime(state, false), &version) != running
}

/// Hash of the configuration a voice confirmation refers to: the running
/// engine's snapshot hash if it runs this strategy, otherwise the hash the
/// strategy would have with the current settings and Flowseal version.
pub(super) fn effective_hash(
    state: &AppState,
    engine: Option<&EngineStatus>,
    id: &str,
) -> Option<String> {
    if let Some(e) =
        engine.filter(|e| e.state == EngineState::Running && e.strategy_id.as_deref() == Some(id))
    {
        if let Some(h) = &e.config_hash {
            return Some(h.clone());
        }
    }
    let s = state.strategies.lock().expect("lock").get(id).cloned()?;
    let version = state.bundle.lock().expect("lock").version.clone()?;
    Some(bridge_core::strategy::config_hash(
        &s,
        &zapret::runtime(state, false),
        &version,
    ))
}

pub(super) fn voice_state(
    state: &AppState,
    engine: Option<&EngineStatus>,
    id: Option<&str>,
) -> VoiceState {
    let Some(id) = id else {
        return VoiceState::NotChecked;
    };
    let has_rules = state
        .strategies
        .lock()
        .expect("lock")
        .get(id)
        .map(|s| diagnostics::voice_rules(s).status != CheckStatus::Warning)
        .unwrap_or(false);
    if !has_rules {
        return VoiceState::NoUdpRules;
    }
    let Some(hash) = effective_hash(state, engine, id) else {
        return VoiceState::NotChecked;
    };
    match state.settings().voice_confirmations.get(&hash) {
        Some(c) if c.works => VoiceState::UserConfirmed,
        Some(_) => VoiceState::UserReportedBroken,
        None => VoiceState::NotChecked,
    }
}

#[tauri::command]
pub async fn get_overview(app: AppHandle, state: State<'_, AppState>) -> R<Overview> {
    let settings = state.settings();
    let (helper, engine) = match state.helper.hello().await {
        Ok(v) => {
            let e = zapret::engine_status(&state).await.ok();
            (
                HelperInfo {
                    available: true,
                    version: Some(v),
                    error: None,
                },
                e,
            )
        }
        Err(e) => (
            HelperInfo {
                available: false,
                version: None,
                error: Some(e.to_string()),
            },
            None,
        ),
    };
    let active_id = engine
        .as_ref()
        .filter(|e| e.state == EngineState::Running)
        .and_then(|e| e.strategy_id.clone())
        .or(settings.active_strategy_id.clone());
    let (active_strategy, load_errors, catalog_errors) = {
        let sm = state.strategies.lock().expect("lock");
        let item = active_id
            .as_ref()
            .and_then(|id| sm.list().into_iter().find(|s| &s.id == id));
        (
            item,
            sm.load_errors.clone(),
            sm.catalog
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.file, e.error))
                .collect(),
        )
    };
    let pending = pending_apply(&state, engine.as_ref());
    let active_full = active_id
        .as_ref()
        .and_then(|id| state.strategies.lock().expect("lock").get(id).cloned());
    let pack_ports = state
        .catalog
        .port_packs()
        .into_iter()
        .filter_map(|p| {
            bridge_core::services::pack_port_coverage(
                &p,
                active_full.as_ref(),
                &settings.game_filter,
            )
            .map(|coverage| PackPortStatus {
                id: p.id.clone(),
                ports: p.ports.clone(),
                coverage,
            })
        })
        .collect();
    let ip_sets = {
        let strategy = active_full;
        let runs = engine.as_ref().is_some_and(|e| {
            e.state == EngineState::Running && e.strategy_id.is_some() && e.strategy_id == active_id
        });
        bridge_core::ipset::ip_set_status(
            settings.ipset_mode,
            &settings.user_lists,
            &settings.installed_packs,
            strategy.as_ref(),
            runs,
            pending,
        )
    };
    Ok(Overview {
        app_version: env!("CARGO_PKG_VERSION").into(),
        ip_sets,
        pack_ports,
        discord: discord::status(settings.discord_path.as_deref()),
        voice: voice_state(&state, engine.as_ref(), active_id.as_deref()),
        pending_apply: pending,
        autostart_enabled: app.autolaunch().is_enabled().unwrap_or(false),
        settings_recovered: state.config.lock().expect("lock").recovered,
        settings,
        helper,
        engine,
        bundle: state.bundle.lock().expect("lock").clone(),
        active_strategy,
        last_diagnostics: state.history.lock().expect("lock").last().cloned(),
        auto_running: state.auto.running.load(Ordering::SeqCst),
        strategy_load_errors: load_errors,
        catalog_errors,
    })
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) -> R<()> {
    quit(&app).await;
    Ok(())
}

pub async fn refresh_tray(app: &AppHandle, state: &AppState) {
    let st = zapret::engine_status(state).await.ok();
    let running = st.as_ref().is_some_and(|s| s.state == EngineState::Running);
    let active = st
        .and_then(|s| s.strategy_id)
        .or(state.settings().active_strategy_id);
    crate::platform::tray::refresh(app, running, active);
}

/// Full exit: optionally stop our engine (setting), then quit.
pub async fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.quitting.store(true, Ordering::SeqCst);
    // Auto sees `quitting` as a cancellation and restores the baseline; wait for it.
    for _ in 0..60 {
        if !state.auto.running.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let _guard = state.op_lock.lock().await;
    if state.settings().stop_bypass_on_exit {
        match state.helper.request(Request::Stop).await {
            Ok(_) => tracing::info!("engine stopped on exit"),
            Err(e) => tracing::warn!("could not stop engine on exit: {e}"),
        }
    } else {
        tracing::info!("exit with engine left running by user setting");
    }
    // Telegram Proxy is our own child process: a full exit always ends it
    // (stopBypassOnExit concerns Zapret only). Closing to the tray does not
    // come here.
    let a = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || {
        a.state::<AppState>().telegram.stop();
    })
    .await;
    app.exit(0);
}
