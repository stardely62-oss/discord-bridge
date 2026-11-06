//! System tray icon and menu.

use crate::state::AppState;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

pub const TRAY_ID: &str = "main";

const ICON_ON: &[u8] = include_bytes!("../../icons/tray-on.png");
const ICON_OFF: &[u8] = include_bytes!("../../icons/tray-off.png");

/// Tray texts in the interface language.
struct Texts {
    choose: &'static str,
    on: &'static str,
    off: &'static str,
    open: &'static str,
    launch: &'static str,
    enable: &'static str,
    disable: &'static str,
    diagnostics: &'static str,
    quit: &'static str,
    tip_on: &'static str,
    tip_off: &'static str,
}

fn texts(lang: &str) -> Texts {
    if lang == "en" {
        Texts {
            choose: "Choose strategy",
            on: "DPI bypass: on",
            off: "DPI bypass: off",
            open: "Open Discord Bridge",
            launch: "Launch Discord",
            enable: "Turn on DPI bypass",
            disable: "Turn off DPI bypass",
            diagnostics: "Open diagnostics",
            quit: "Quit",
            tip_on: "Discord Bridge — DPI bypass on",
            tip_off: "Discord Bridge — DPI bypass off",
        }
    } else {
        Texts {
            choose: "Выбрать стратегию",
            on: "Обход DPI: включён",
            off: "Обход DPI: выключен",
            open: "Открыть Discord Bridge",
            launch: "Запустить Discord",
            enable: "Включить обход DPI",
            disable: "Выключить обход DPI",
            diagnostics: "Открыть диагностику",
            quit: "Выход",
            tip_on: "Discord Bridge — обход DPI включён",
            tip_off: "Discord Bridge — обход DPI выключен",
        }
    }
}

fn state_icon(running: bool) -> Option<tauri::image::Image<'static>> {
    tauri::image::Image::from_bytes(if running { ICON_ON } else { ICON_OFF }).ok()
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_menu(app: &AppHandle, running: bool, active: Option<String>) -> tauri::Result<Menu<Wry>> {
    let state = app.state::<AppState>();
    let t = texts(&state.settings().language);
    let items: Vec<(String, String)> = state
        .strategies
        .lock()
        .expect("lock")
        .all()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect();
    let strat = Submenu::with_id(app, "strategies", t.choose, true)?;
    for (id, name) in items {
        let checked = active.as_deref() == Some(id.as_str());
        let it = CheckMenuItem::with_id(
            app,
            format!("strategy:{id}"),
            name,
            true,
            checked,
            None::<&str>,
        )?;
        strat.append(&it)?;
    }
    let status = MenuItem::with_id(
        app,
        "status",
        if running { t.on } else { t.off },
        false,
        None::<&str>,
    )?;
    Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open", t.open, true, None::<&str>)?,
            &MenuItem::with_id(app, "launch", t.launch, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "enable", t.enable, !running, None::<&str>)?,
            &MenuItem::with_id(app, "disable", t.disable, running, None::<&str>)?,
            &strat,
            &MenuItem::with_id(app, "diagnostics", t.diagnostics, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?,
        ],
    )
}

pub fn refresh(app: &AppHandle, running: bool, active: Option<String>) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_menu(app, running, active) {
            let _ = tray.set_menu(Some(menu));
        }
        let t = texts(&app.state::<AppState>().settings().language);
        let _ = tray.set_tooltip(Some(if running { t.tip_on } else { t.tip_off }));
        if let Some(icon) = state_icon(running) {
            let _ = tray.set_icon(Some(icon));
        }
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, false, None)?;
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Discord Bridge");
    if let Some(icon) = state_icon(false).or_else(|| app.default_window_icon().cloned()) {
        b = b.icon(icon);
    }
    b.on_menu_event(|app, ev| {
        let id = ev.id().as_ref().to_string();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            match id.as_str() {
                "open" => show_main(&app),
                "launch" => {
                    match crate::commands::launch_with_bridge(&app, &state, "preference").await {
                        Ok(r) if !r.launched => {
                            show_main(&app);
                            if let Some(b) = r.bypass {
                                let _ = app.emit("activation-result", b);
                            }
                        }
                        Ok(_) => {}
                        Err(e) => {
                            show_main(&app);
                            tracing::warn!("tray launch failed: {e}");
                        }
                    }
                }
                "enable" => {
                    let target = state
                        .settings()
                        .active_strategy_id
                        .unwrap_or_else(|| "flowseal-general".into());
                    if crate::zapret::ensure_not_auto(&state).is_err() {
                        show_main(&app);
                        return;
                    }
                    let r = crate::zapret::activate(&app, &state, &target, false, true).await;
                    if !r.ok {
                        show_main(&app);
                        let _ = app.emit("activation-result", r);
                    }
                }
                "disable" => {
                    if crate::zapret::ensure_not_auto(&state).is_ok() {
                        if let Err(e) = crate::zapret::deactivate(&app, &state).await {
                            tracing::warn!("tray disable failed: {e}");
                        }
                    } else {
                        show_main(&app);
                    }
                }
                "diagnostics" => {
                    show_main(&app);
                    let _ = app.emit("navigate", "diagnostics");
                }
                "quit" => crate::commands::quit(&app).await,
                other => {
                    if let Some(sid) = other.strip_prefix("strategy:") {
                        if crate::zapret::ensure_not_auto(&state).is_err() {
                            show_main(&app);
                            return;
                        }
                        let r = crate::zapret::activate(&app, &state, sid, false, true).await;
                        if !r.ok {
                            show_main(&app);
                        }
                        let _ = app.emit("activation-result", r);
                    }
                }
            }
        });
    })
    .on_tray_icon_event(|tray, ev| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = ev
        {
            show_main(tray.app_handle());
        }
    })
    .build(app)?;
    Ok(())
}
