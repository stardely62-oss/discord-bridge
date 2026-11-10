//! Settings and the plain-text list files of the manual lists editor.

use super::*;

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

pub(super) const MAX_LIST_FILE: u64 = 1024 * 1024;

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

pub(super) fn parse_list_text(text: &str) -> Vec<String> {
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

    #[test]
    fn external_links_are_fixed_or_built_from_validated_versions() {
        assert_eq!(
            super::external_url("app-release:0.1.0-alpha.11").as_deref(),
            Some("https://github.com/stardely62-oss/discord-bridge/releases/tag/v0.1.0-alpha.11")
        );
        assert_eq!(
            super::external_url("flowseal-release:1.10.5").as_deref(),
            Some("https://github.com/Flowseal/zapret-discord-youtube/releases/tag/1.10.5")
        );
        assert!(super::external_url("releases").is_some());
        assert_eq!(
            super::external_url("author-projects").as_deref(),
            Some("https://stardely62-oss.github.io/")
        );
        assert_eq!(
            super::external_url("report-issue").as_deref(),
            Some("https://github.com/stardely62-oss/discord-bridge/issues/new/choose")
        );
        for bad in [
            "app-release:0.1.0/../../x",
            "app-release:https://evil.example",
            "app-release:",
            "flowseal-release:1.10.5/../x",
            "flowseal-release:file:///c:/x",
            "https://evil.example",
            "file:///C:/Windows/System32/calc.exe",
            "unknown",
        ] {
            assert!(super::external_url(bad).is_none(), "{bad}");
        }
    }
}
