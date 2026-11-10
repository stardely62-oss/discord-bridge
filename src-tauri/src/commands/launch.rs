//! Launching the official Discord client and opening fixed external links.

use super::*;

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

/// Opens one of a fixed set of pages, or the page of a specific release
/// (`app-release:<SemVer>`, `flowseal-release:<tag>`). Release URLs are built
/// from a validated version, never taken from the caller; only https pages on
/// github.com are opened.
#[tauri::command]
pub async fn open_external(app: AppHandle, target: String) -> R<()> {
    let url = external_url(&target).ok_or("неизвестная ссылка")?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

pub(crate) fn external_url(target: &str) -> Option<String> {
    if let Some(v) = target.strip_prefix("app-release:") {
        let v = upd::app_tag_version(&format!("v{v}"))?;
        return Some(upd::app_release_url(&v));
    }
    if let Some(t) = target.strip_prefix("flowseal-release:") {
        return upd::flowseal_release_url(t);
    }
    Some(
        match target {
            "discord-download" => discord::DOWNLOAD_URL,
            "repo" => "https://github.com/stardely62-oss/discord-bridge",
            "releases" => "https://github.com/stardely62-oss/discord-bridge/releases",
            "flowseal" => "https://github.com/Flowseal/zapret-discord-youtube",
            "flowseal-releases" => "https://github.com/Flowseal/zapret-discord-youtube/releases",
            "zapret" => "https://github.com/bol-van/zapret",
            "windivert" => "https://github.com/basil00/WinDivert",
            // The author's other projects (About page); a fixed GitHub Pages
            // address of the project account, never taken from the network.
            "author-projects" => "https://stardely62-oss.github.io/",
            // Issue forms (.github/ISSUE_TEMPLATE); the report is attached by the user.
            "report-issue" => "https://github.com/stardely62-oss/discord-bridge/issues/new/choose",
            _ => return None,
        }
        .to_string(),
    )
}
