//! Tauri commands: the complete API used by the UI. Every command maps to a
//! real backend operation.

use crate::configuration::{LaunchMode, Settings, VoiceConfirmation};
use crate::diagnostics::{self, CheckItem, CheckStatus, DiagnosticsReport, VoiceState};
use crate::discord::{self, DiscordStatus};
use crate::state::{AppState, BundleState};
use crate::strategies::{ImportPreview, StrategyDetails, StrategyListItem, StrategyManager};
use crate::zapret::{self, ActivationResult, AutoProgress, UpdateInfo};
use bridge_core::diff::StrategyDiff;
use bridge_core::ipc::{EngineState, EngineStatus, Request, Response};
use bridge_core::strategy::options::{option_specs, OptionSpec};
use bridge_core::strategy::Strategy;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

type R<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperInfo {
    pub available: bool,
    pub version: Option<String>,
    pub error: Option<String>,
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
fn effective_hash(state: &AppState, engine: Option<&EngineStatus>, id: &str) -> Option<String> {
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

fn voice_state(state: &AppState, engine: Option<&EngineStatus>, id: Option<&str>) -> VoiceState {
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
    let ip_sets = {
        let strategy = active_id
            .as_ref()
            .and_then(|id| state.strategies.lock().expect("lock").get(id).cloned());
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

// ---------------- Discord ----------------

/// Launch the official client as is (no bypass involvement).
pub fn do_launch_discord(state: &AppState) -> R<DiscordStatus> {
    let s = discord::status(state.settings().discord_path.as_deref());
    let install = s.install.clone().ok_or(
        "Discord не найден. Установите его с discord.com/download или укажите путь в настройках.",
    )?;
    if s.running {
        // This channel is already running: no second instance.
        return Ok(s);
    }
    discord::launch(&install)?;
    tracing::info!("Discord launched ({})", install.channel);
    Ok(discord::status(state.settings().discord_path.as_deref()))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LaunchResult {
    pub launched: bool,
    pub discord: Option<DiscordStatus>,
    /// Present when the bypass had to be started for this launch.
    pub bypass: Option<ActivationResult>,
    pub bypass_was_running: bool,
    pub message: String,
}

/// The single «Launch Discord» scenario used by the home page, onboarding,
/// the tray and logon autostart. `mode`: `withBypass`, `withoutBypass` or
/// `preference` (the saved setting). With the bypass, the engine is started
/// first if needed (no second instance), without the slow HTTPS probe; if it
/// cannot be started, Discord is NOT launched and the error is returned, so
/// the user can explicitly choose to launch without the bypass.
pub async fn launch_with_bridge(app: &AppHandle, state: &AppState, mode: &str) -> R<LaunchResult> {
    let settings = state.settings();
    let with_bypass = match mode {
        "withBypass" => true,
        "withoutBypass" => false,
        "preference" => settings.launch_mode == LaunchMode::WithBypass,
        _ => return Err("неизвестный режим запуска".into()),
    };
    let d = discord::status(settings.discord_path.as_deref());
    if !d.installed {
        return Err(d.custom_path_error.unwrap_or_else(|| "Discord не найден. Установите его с discord.com/download или укажите путь в настройках.".into()));
    }
    let mut result = LaunchResult {
        launched: false,
        discord: None,
        bypass: None,
        bypass_was_running: false,
        message: String::new(),
    };
    if with_bypass {
        zapret::ensure_not_auto(state)?;
        state
            .helper
            .hello()
            .await
            .map_err(|e| format!("обход не запущен: {e}"))?;
        let st = zapret::engine_status(state).await?;
        if st.state == EngineState::Running {
            result.bypass_was_running = true;
        } else {
            let id = settings
                .active_strategy_id
                .clone()
                .unwrap_or_else(|| "flowseal-general".into());
            let r = zapret::activate(app, state, &id, false, false).await;
            let ok = r.ok;
            let msg = r.message.clone().unwrap_or_else(|| r.outcome.clone());
            result.bypass = Some(r);
            if !ok {
                result.message = format!("обход DPI не запустился: {msg}. Discord не запущен — можно запустить его без обхода.");
                return Ok(result);
            }
        }
    }
    let ds = do_launch_discord(state)?;
    result.launched = true;
    result.message = match (with_bypass, result.bypass_was_running, d.running) {
        (_, _, true) => "Discord уже запущен".into(),
        (true, true, _) => "обход уже работал, Discord запускается".into(),
        (true, false, _) => "обход включён, Discord запускается".into(),
        (false, _, _) => "Discord запускается без обхода".into(),
    };
    result.discord = Some(ds);
    let _ = app.emit("status-changed", ());
    Ok(result)
}

#[tauri::command]
pub async fn launch_discord(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: Option<String>,
) -> R<LaunchResult> {
    launch_with_bridge(&app, &state, mode.as_deref().unwrap_or("preference")).await
}

#[tauri::command]
pub async fn discord_status(state: State<'_, AppState>) -> R<DiscordStatus> {
    Ok(discord::status(state.settings().discord_path.as_deref()))
}

#[tauri::command]
pub async fn set_discord_path(
    state: State<'_, AppState>,
    path: Option<String>,
) -> R<DiscordStatus> {
    if let Some(p) = &path {
        discord::from_custom(p)?;
    }
    state.update_settings(|s| s.discord_path = path.filter(|p| !p.is_empty()))?;
    Ok(discord::status(state.settings().discord_path.as_deref()))
}

#[tauri::command]
pub async fn open_external(app: AppHandle, target: String) -> R<()> {
    let url = match target.as_str() {
        "discord-download" => discord::DOWNLOAD_URL,
        "repo" => "https://github.com/stardely62-oss/discord-bridge",
        "releases" => "https://github.com/stardely62-oss/discord-bridge/releases",
        "flowseal" => "https://github.com/Flowseal/zapret-discord-youtube",
        "flowseal-releases" => "https://github.com/Flowseal/zapret-discord-youtube/releases",
        "zapret" => "https://github.com/bol-van/zapret",
        "windivert" => "https://github.com/basil00/WinDivert",
        _ => return Err("неизвестная ссылка".into()),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

// ---------------- Strategies ----------------

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

fn after_strategy_change(app: &AppHandle) {
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

// ---------------- Bypass ----------------

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

// ---------------- Diagnostics ----------------

#[tauri::command]
pub async fn run_diagnostics(state: State<'_, AppState>) -> R<DiagnosticsReport> {
    let settings = state.settings();
    let mut items: Vec<CheckItem> = Vec::new();
    let d = discord::status(settings.discord_path.as_deref());
    items.push(CheckItem {
        id: "discord".into(),
        group: "discord".into(),
        title: "Клиент Discord".into(),
        status: if d.installed {
            CheckStatus::Ok
        } else {
            CheckStatus::Error
        },
        detail: match &d.install {
            Some(i) => format!(
                "{} {}{}",
                i.channel,
                i.version.clone().unwrap_or_default(),
                if d.running { ", запущен" } else { "" }
            ),
            None => d
                .custom_path_error
                .clone()
                .unwrap_or_else(|| "не найден".into()),
        },
        hint: (!d.installed)
            .then(|| "Установите официальный клиент с discord.com/download.".to_string()),
        duration_ms: None,
    });
    let (engine, helper_err) = match state.helper.hello().await {
        Ok(_) => (zapret::engine_status(&state).await.ok(), None),
        Err(e) => (None, Some(e.to_string())),
    };
    items.extend(diagnostics::engine_items(
        engine.as_ref(),
        helper_err.as_deref(),
    ));
    let active_id = engine
        .as_ref()
        .filter(|e| e.state == EngineState::Running)
        .and_then(|e| e.strategy_id.clone())
        .or(settings.active_strategy_id.clone());
    let active = active_id
        .as_ref()
        .and_then(|id| state.strategies.lock().expect("lock").get(id).cloned());
    let needs_ts = active.as_ref().is_some_and(|s| {
        s.rules.iter().flat_map(|r| &r.args).any(|a| {
            a.opt == "--dpi-desync-fooling" && a.text_value().is_some_and(|v| v.contains("ts"))
        })
    });
    if helper_err.is_none() {
        if let Ok(Response::Diagnose { diagnostics: hd }) =
            state.helper.request(Request::Diagnose).await
        {
            items.extend(diagnostics::helper_items(&hd, needs_ts));
        }
    }
    if let Some(s) = &active {
        let report = {
            let sm = state.strategies.lock().expect("lock");
            bridge_core::strategy::validate::validate(s, Some(&sm.resources))
        };
        let errs: Vec<String> = report
            .errors()
            .map(|i| format!("{}: {}", i.path, i.message))
            .collect();
        items.push(CheckItem {
            id: "strategy-resources".into(),
            group: "zapret".into(),
            title: format!("Стратегия «{}»", s.name),
            status: if errs.is_empty() {
                CheckStatus::Ok
            } else {
                CheckStatus::Error
            },
            detail: if errs.is_empty() {
                format!("конфигурация корректна, ресурсов: {}", s.resources().len())
            } else {
                errs.join("; ")
            },
            hint: None,
            duration_ms: None,
        });
        items.push(diagnostics::voice_rules(s));
    }
    // DNS and HTTPS in parallel.
    let mut set = tokio::task::JoinSet::new();
    for (i, (host, url)) in diagnostics::HTTPS_TARGETS.iter().enumerate() {
        let c = state.http.clone();
        let (h, u) = (host.to_string(), url.to_string());
        set.spawn(async move {
            (
                i,
                diagnostics::dns_check(&h).await,
                diagnostics::https_check(&c, &h, &u).await,
            )
        });
    }
    let mut net = Vec::new();
    while let Some(Ok(r)) = set.join_next().await {
        net.push(r);
    }
    net.sort_by_key(|(i, _, _)| *i);
    let api = diagnostics::api_check(&state.http).await;
    let https_items: Vec<CheckItem> = net
        .iter()
        .map(|(_, _, h)| h.clone())
        .chain(std::iter::once(api.clone()))
        .collect();
    // Every planned check must have produced a result; an empty or partial
    // set is a failure, never a success.
    let https_ok = diagnostics::probe_verdict(diagnostics::HTTPS_TARGETS.len() + 1, &https_items);
    for (_, dns, _) in &net {
        items.push(dns.clone());
    }
    for (_, _, h) in net {
        items.push(h);
    }
    items.push(api);
    let voice = voice_state(&state, engine.as_ref(), active_id.as_deref());
    items.push(CheckItem {
        id: "voice".into(),
        group: "udp".into(),
        title: "Голосовые каналы".into(),
        status: match voice {
            VoiceState::UserConfirmed => CheckStatus::Ok,
            VoiceState::UserReportedBroken => CheckStatus::Error,
            VoiceState::NoUdpRules => CheckStatus::Warning,
            VoiceState::NotChecked => CheckStatus::Unknown,
        },
        detail: match voice {
            VoiceState::UserConfirmed => "вы подтвердили, что голос работает с этой стратегией".into(),
            VoiceState::UserReportedBroken => "вы отметили, что голос не работает с этой стратегией".into(),
            VoiceState::NoUdpRules => "в стратегии нет UDP-правил для голоса".into(),
            VoiceState::NotChecked => "голос не проверен: автоматической проверки голосового соединения нет, подключитесь к голосовому каналу и подтвердите результат".into(),
        },
        hint: None,
        duration_ms: None,
    });
    let report = DiagnosticsReport {
        at: crate::strategies::now_unix(),
        strategy_id: active_id,
        engine_running: engine
            .as_ref()
            .is_some_and(|e| e.state == EngineState::Running),
        https_ok: Some(https_ok),
        voice,
        items,
    };
    tracing::info!(
        "diagnostics: https_ok={https_ok}, engine_running={}",
        report.engine_running
    );
    state.push_history(report.clone());
    Ok(report)
}

#[tauri::command]
pub async fn confirm_voice(state: State<'_, AppState>, strategy_id: String, works: bool) -> R<()> {
    let engine = zapret::engine_status(&state).await.ok();
    let running_this = engine.as_ref().is_some_and(|e| {
        e.state == EngineState::Running && e.strategy_id.as_deref() == Some(strategy_id.as_str())
    });
    if !running_this {
        return Err("подтвердить голос можно только для работающей сейчас стратегии".into());
    }
    let hash = effective_hash(&state, engine.as_ref(), &strategy_id)
        .ok_or("не удалось определить конфигурацию")?;
    state.update_settings(|s| {
        s.voice_confirmations.insert(
            hash.clone(),
            VoiceConfirmation {
                at: crate::strategies::now_unix(),
                works,
                strategy_id: strategy_id.clone(),
            },
        );
        // bounded history
        while s.voice_confirmations.len() > 200 {
            let oldest = s
                .voice_confirmations
                .iter()
                .min_by_key(|(_, v)| v.at)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    s.voice_confirmations.remove(&k);
                }
                None => break,
            }
        }
    })?;
    tracing::info!(
        "user voice confirmation for {strategy_id} ({}): {works}",
        &hash[..12]
    );
    Ok(())
}

#[tauri::command]
pub async fn diagnostics_history(state: State<'_, AppState>) -> R<Vec<DiagnosticsReport>> {
    Ok(state.history.lock().expect("lock").clone())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TcpInfo {
    pub current: Option<bool>,
    pub changed_by_us: bool,
    pub message: String,
}

fn tcp_info(r: Response) -> R<TcpInfo> {
    match r {
        Response::TcpTimestamps {
            current,
            changed_by_us,
            message,
        } => Ok(TcpInfo {
            current,
            changed_by_us,
            message,
        }),
        Response::Error { message, .. } => Err(message),
        _ => Err("неожиданный ответ службы".into()),
    }
}

/// Global Windows TCP setting. The UI asks for explicit consent; the helper
/// additionally authorizes the caller (owner policy) and records the original
/// value so it can be restored later.
#[tauri::command]
pub async fn set_tcp_timestamps(
    state: State<'_, AppState>,
    enable: bool,
    user_confirmed: bool,
) -> R<TcpInfo> {
    tcp_info(
        state
            .helper
            .request(Request::SetTcpTimestamps {
                enable,
                user_confirmed,
            })
            .await
            .map_err(|e| e.to_string())?,
    )
}

#[tauri::command]
pub async fn restore_tcp_timestamps(state: State<'_, AppState>) -> R<TcpInfo> {
    tcp_info(
        state
            .helper
            .request(Request::RestoreTcpTimestamps)
            .await
            .map_err(|e| e.to_string())?,
    )
}

#[tauri::command]
pub async fn get_logs(state: State<'_, AppState>, lines: Option<usize>) -> R<Vec<String>> {
    Ok(state.logs.tail(lines.unwrap_or(300).min(5000)))
}

#[tauri::command]
pub async fn clear_logs(state: State<'_, AppState>) -> R<()> {
    state.logs.clear().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_report(state: State<'_, AppState>, path: String) -> R<()> {
    let s = state.settings();
    let engine = zapret::engine_status(&state).await.ok();
    let engine_log = match state.helper.request(Request::EngineLog).await {
        Ok(Response::EngineLog { lines }) => lines,
        _ => vec![],
    };
    let report = serde_json::json!({
        "generatedAt": crate::strategies::now_unix(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "bundle": &*state.bundle.lock().expect("lock"),
        "engine": engine,
        "settings": {
            "activeStrategyId": s.active_strategy_id,
            "bypassEnabled": s.bypass_enabled,
            "gameFilter": s.game_filter,
            "ipsetMode": s.ipset_mode,
            "customDiscordPath": s.discord_path.is_some(),
            "userListSizes": [s.user_lists.general.len(), s.user_lists.exclude.len(), s.user_lists.ipset_exclude.len()],
        },
        "diagnostics": state.history.lock().expect("lock").iter().rev().take(5).cloned().collect::<Vec<_>>(),
        "engineLog": engine_log,
        "appLog": state.logs.tail(400),
    });
    let text = state
        .logs
        .redact(&serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    bridge_core::atomic::write_atomic(std::path::Path::new(&path), text.as_bytes())
        .map_err(|e| e.to_string())
}

// ---------------- Settings ----------------

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> R<Settings> {
    Ok(state.settings())
}

/// Settings and Windows autostart are kept consistent: the autostart entry is
/// changed first and verified; if saving the settings then fails, the
/// autostart change is reverted, so the UI never shows a state Windows lacks.
#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> R<Settings> {
    settings.validate()?;
    let autostart = app.autolaunch();
    let before = autostart.is_enabled().unwrap_or(false);
    let changed = settings.launch_at_startup != before;
    if changed {
        let r = if settings.launch_at_startup {
            autostart.enable()
        } else {
            autostart.disable()
        };
        r.map_err(|e| format!("автозапуск Windows не изменён: {e}"))?;
        if autostart
            .is_enabled()
            .unwrap_or(!settings.launch_at_startup)
            != settings.launch_at_startup
        {
            return Err("Windows не подтвердила изменение автозапуска".into());
        }
    }
    let result = state.config.lock().expect("lock").update(settings.clone());
    if let Err(e) = result {
        if changed {
            let revert = if before {
                autostart.enable()
            } else {
                autostart.disable()
            };
            if let Err(re) = revert {
                tracing::warn!("autostart revert failed: {re}");
            }
        }
        return Err(e);
    }
    state.logs.set_enabled(settings.keep_logs);
    // The tray menu follows the interface language.
    refresh_tray(&app, &state).await;
    Ok(state.settings())
}

// ---------------- Updates ----------------

#[tauri::command]
pub async fn flowseal_check_update(state: State<'_, AppState>) -> R<UpdateInfo> {
    Ok(zapret::check_flowseal_update(&state).await)
}

/// Install a verified Flowseal release, then switch to it transactionally
/// (the previous version stays installed for rollback).
#[tauri::command]
pub async fn flowseal_install(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> R<String> {
    zapret::ensure_not_auto(&state)?;
    zapret::install_flowseal(&state, &version).await?;
    zapret::load_bundle(&state).await;
    zapret::switch_bundle(&app, &state, &version).await
}

#[tauri::command]
pub async fn flowseal_set_active(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> R<String> {
    zapret::switch_bundle(&app, &state, &version).await
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
pub async fn app_check_update(state: State<'_, AppState>) -> R<UpdateInfo> {
    Ok(zapret::check_app_update(&state).await)
}

#[tauri::command]
pub async fn get_bundle(state: State<'_, AppState>) -> R<BundleState> {
    Ok(state.bundle.lock().expect("lock").clone())
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
    app.exit(0);
}

const MAX_LIST_FILE: u64 = 1024 * 1024;

/// Reads a plain-text list (one entry per line) chosen by the user for the
/// manual lists editor. Nothing is applied here: entries are validated when
/// the settings are saved.
#[tauri::command]
pub fn read_list_file(path: String) -> R<Vec<String>> {
    let p = std::path::Path::new(&path);
    let meta = std::fs::metadata(p).map_err(|e| format!("файл недоступен: {e}"))?;
    if !meta.is_file() || meta.len() > MAX_LIST_FILE {
        return Err("нужен текстовый файл не больше 1 МБ".into());
    }
    let text =
        std::fs::read_to_string(p).map_err(|_| "файл не является текстом UTF-8".to_string())?;
    Ok(parse_list_text(&text))
}

/// Writes a list to a user-chosen text file.
#[tauri::command]
pub fn write_list_file(path: String, entries: Vec<String>) -> R<()> {
    let mut text = entries.join("\n");
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| format!("не удалось сохранить файл: {e}"))
}

fn parse_list_text(text: &str) -> Vec<String> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod list_file_tests {
    #[test]
    fn parses_lines_bom_and_blanks() {
        assert_eq!(
            super::parse_list_text("\u{feff}a.com\r\n\n  # note \nb.org\n"),
            vec!["a.com", "# note", "b.org"]
        );
    }
}
