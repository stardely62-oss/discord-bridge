//! Discord Bridge application entry point.

mod catalog_sync;
mod commands;
mod configuration;
mod diagnostics;
mod discord;
mod platform;
mod privileges;
mod state;
mod strategies;
mod zapret;

use crate::configuration::{CloseBehavior, ConfigurationManager};
use crate::platform::logging::LogStore;
use crate::privileges::HelperClient;
use crate::state::AppState;
use crate::strategies::StrategyManager;
use bridge_core::ipc::EngineState;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager, WindowEvent};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            platform::tray::show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let log_dir = app.path().app_log_dir()?;
            let resource_dir = app.path().resource_dir()?;

            let logs = LogStore::new(log_dir.join("discord-bridge.log"));
            let _ = tracing_subscriber::fmt()
                .with_writer(logs.clone())
                .with_ansi(false)
                .with_target(false)
                .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
                .try_init();

            let config = ConfigurationManager::load(config_dir);
            logs.set_enabled(config.settings.keep_logs);
            if config.recovered {
                tracing::warn!("settings were corrupted and restored from backup");
            }
            let start_minimized =
                std::env::args().any(|a| a == "--minimized") || config.settings.start_minimized;
            let history_path = data_dir.join("diagnostics-history.json");
            let state = AppState {
                history: Mutex::new(AppState::load_history(&history_path)),
                history_path,
                strategies: Mutex::new(StrategyManager::new(data_dir.join("strategies"))),
                config: Mutex::new(config),
                helper: HelperClient::default(),
                logs,
                http: diagnostics::http_client(),
                bundle: Mutex::new(Default::default()),
                bundle_resources: Mutex::new(Default::default()),
                auto: Default::default(),
                op_lock: tokio::sync::Mutex::new(()),
                resource_dir,
                quitting: Default::default(),
                catalog: catalog_sync::CatalogRuntime::production(data_dir.join("catalog")),
            };
            app.manage(state);
            platform::tray::create(&handle)?;
            tracing::info!("Discord Bridge {} started", env!("CARGO_PKG_VERSION"));

            if !start_minimized {
                platform::tray::show_main(&handle);
            }

            // Startup sequence: catalog → optional bypass → optional Discord.
            let h = handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = h.state::<AppState>();
                zapret::load_bundle(&state).await;
                let _ = h.emit("strategies-changed", ());
                // At Windows logon the app can start before the helper
                // service is ready: wait for it (up to ~60 s) instead of failing.
                if state.helper.hello().await.is_err() {
                    for _ in 0..30 {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        if state.helper.hello().await.is_ok() {
                            tracing::info!("helper became available after startup");
                            zapret::load_bundle(&state).await;
                            let _ = h.emit("strategies-changed", ());
                            let _ = h.emit("status-changed", ());
                            break;
                        }
                    }
                }
                zapret::recover_abandoned_trial(&state).await;
                let s = state.settings();
                if s.auto_launch_discord {
                    // One shared scenario: bypass according to preference, then Discord.
                    match commands::launch_with_bridge(&h, &state, "preference").await {
                        Ok(r) if !r.launched => {
                            platform::tray::show_main(&h);
                            if let Some(b) = r.bypass {
                                let _ = h.emit("activation-result", b);
                            }
                        }
                        Ok(_) => {}
                        Err(e) => tracing::warn!("auto-launch Discord failed: {e}"),
                    }
                } else if s.auto_enable_bypass {
                    let id = s
                        .active_strategy_id
                        .clone()
                        .unwrap_or_else(|| "flowseal-general".into());
                    let running = zapret::engine_status(&state)
                        .await
                        .ok()
                        .filter(|e| e.state == EngineState::Running);
                    if running
                        .as_ref()
                        .and_then(|e| e.strategy_id.clone())
                        .as_deref()
                        != Some(id.as_str())
                    {
                        let r = zapret::activate(&h, &state, &id, false, false).await;
                        if !r.ok {
                            platform::tray::show_main(&h);
                            let _ = h.emit("activation-result", r);
                        }
                    }
                }
                commands::refresh_tray(&h, &state).await;
            });

            catalog_sync::spawn_periodic(handle.clone());

            // Lightweight watcher: detects an engine that died on its own.
            let h = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut last: Option<(EngineState, Option<String>)> = None;
                let mut restarts = 0u32;
                loop {
                    let visible = h
                        .get_webview_window("main")
                        .and_then(|w| w.is_visible().ok())
                        .unwrap_or(false);
                    tokio::time::sleep(Duration::from_secs(if visible { 5 } else { 20 })).await;
                    let state = h.state::<AppState>();
                    if state.quitting.load(Ordering::SeqCst) {
                        break;
                    }
                    let cur = zapret::engine_status(&state)
                        .await
                        .ok()
                        .map(|s| (s.state, s.strategy_id));
                    // Self-healing: restart an engine that crashed while the
                    // bypass is supposed to be on (bounded to 3 tries per streak).
                    match &cur {
                        Some((EngineState::Running, _)) => restarts = 0,
                        Some((EngineState::Failed, _))
                            if restarts < 3 && state.settings().bypass_enabled =>
                        {
                            restarts += 1;
                            let id = state
                                .settings()
                                .active_strategy_id
                                .unwrap_or_else(|| "flowseal-general".into());
                            tracing::warn!(
                                "engine failed, automatic restart {restarts}/3 with {id}"
                            );
                            if !state.auto.running.load(Ordering::SeqCst) {
                                let _ = zapret::activate(&h, &state, &id, false, false).await;
                            }
                        }
                        _ => {}
                    }
                    if cur != last {
                        if let Some((EngineState::Failed, _)) = &cur {
                            tracing::warn!("engine is in failed state");
                        }
                        let _ = h.emit("status-changed", ());
                        commands::refresh_tray(&h, &state).await;
                        last = cur;
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                let state = app.state::<AppState>();
                if state.quitting.load(Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();
                match state.settings().close_behavior {
                    CloseBehavior::Tray => {
                        let _ = window.hide();
                    }
                    CloseBehavior::Exit => {
                        tauri::async_runtime::spawn(async move { commands::quit(&app).await });
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_overview,
            commands::launch_discord,
            commands::discord_status,
            commands::set_discord_path,
            commands::open_external,
            commands::list_strategies,
            commands::get_strategy,
            commands::preview_strategy,
            commands::parse_strategy_json,
            commands::save_strategy,
            commands::create_strategy,
            commands::duplicate_strategy,
            commands::rename_strategy,
            commands::delete_strategy,
            commands::restore_original,
            commands::compare_strategies,
            commands::create_discord_only,
            commands::restore_tcp_timestamps,
            commands::flowseal_compat,
            commands::import_preview,
            commands::import_commit,
            commands::export_strategy,
            commands::read_list_file,
            catalog_sync::catalog_get,
            catalog_sync::catalog_refresh,
            catalog_sync::pack_install,
            catalog_sync::pack_remove,
            catalog_sync::pack_rollback,
            catalog_sync::pack_diff,
            commands::write_list_file,
            commands::get_option_specs,
            commands::get_resources,
            commands::bypass_enable,
            commands::bypass_disable,
            commands::select_strategy,
            commands::auto_start,
            commands::auto_cancel,
            commands::engine_log,
            commands::run_diagnostics,
            commands::confirm_voice,
            commands::diagnostics_history,
            commands::set_tcp_timestamps,
            commands::get_logs,
            commands::clear_logs,
            commands::export_report,
            commands::get_settings,
            commands::save_settings,
            commands::flowseal_check_update,
            commands::flowseal_install,
            commands::flowseal_set_active,
            commands::app_check_update,
            commands::get_bundle,
            commands::quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Discord Bridge")
        .run(|_app, _event| {});
}
