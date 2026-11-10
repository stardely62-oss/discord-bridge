//! Diagnostics, voice confirmations, TCP timestamps, logs and the report.

use super::*;

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

pub(super) fn tcp_info(r: Response) -> R<TcpInfo> {
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
