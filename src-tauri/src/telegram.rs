//! Telegram WS Proxy in the app: an ordinary user process (see
//! `bridge_telegram`), independent of Zapret and of the helper service. No
//! request here goes to the helper; stopping or starting one component never
//! touches the other.

use crate::state::AppState;
use bridge_core::ipc::EngineState;
use bridge_telegram::component::{self, Component};
use bridge_telegram::link;
use bridge_telegram::secret::SecretStore;
use bridge_telegram::{
    ComponentProblem, ProxyManager, ProxyState, ProxyStatus, TelegramProxySettings,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

type R<T> = Result<T, String>;

/// SHA-256 of the engine of this build (see build.rs).
pub const COMPILED_SHA256: Option<&str> = option_env!("DB_TGPROXY_SHA256");

pub struct TelegramRuntime {
    pub manager: ProxyManager,
    store: SecretStore,
    dir: PathBuf,
    /// Settings the running process was started with (pending restart).
    /// `Some` from a start until the user stops the proxy: also what the
    /// supervisor restarts with after an unexpected exit.
    running: Mutex<Option<TelegramProxySettings>>,
    supervisor: Mutex<bridge_telegram::supervise::Supervisor>,
}

impl TelegramRuntime {
    pub fn new(app: AppHandle, resource_dir: &Path, config_dir: &Path) -> TelegramRuntime {
        let manager = ProxyManager::new(move || {
            let _ = app.emit("telegram-changed", ());
        });
        let rt = TelegramRuntime {
            manager,
            store: SecretStore::new(config_dir),
            dir: resource_dir.join("tg-ws-proxy"),
            running: Mutex::new(None),
            supervisor: Mutex::new(Default::default()),
        };
        if let Err(p) = rt.component() {
            rt.manager.set_not_installed(match p {
                ComponentProblem::NotInstalled => None,
                ComponentProblem::Damaged(m) => Some(m),
            });
        }
        rt
    }

    fn component(&self) -> Result<Component, ComponentProblem> {
        component::locate(&self.dir, COMPILED_SHA256)
    }

    pub fn start(&self, s: &TelegramProxySettings) -> R<()> {
        let c = self.component().map_err(|p| match p {
            ComponentProblem::NotInstalled => {
                "Telegram Proxy не входит в эту установку Bridge".to_string()
            }
            ComponentProblem::Damaged(m) => format!("компонент Telegram Proxy повреждён: {m}"),
        })?;
        let secret = self.store.load_or_create()?;
        self.manager
            .start(&c, COMPILED_SHA256.unwrap_or_default(), s, &secret)?;
        *self.running.lock().expect("lock") = Some(s.clone());
        Ok(())
    }

    pub fn stop(&self) {
        self.manager.stop();
        *self.running.lock().expect("lock") = None;
    }

    /// One step of the supervisor (called about once a second): starts the
    /// proxy again after an unexpected exit, by the rules of
    /// `bridge_telegram::supervise`. Returns true if it was restarted.
    pub fn supervise(&self) -> bool {
        let wanted = self.running.lock().expect("lock").clone();
        let state = self.manager.status().state;
        let restart = self.supervisor.lock().expect("lock").observe(
            state,
            wanted.is_some(),
            std::time::Instant::now(),
        );
        let Some(settings) = wanted.filter(|_| restart) else {
            return false;
        };
        match self.start(&settings) {
            Ok(()) => {
                tracing::info!("Telegram Proxy restarted after an unexpected exit");
                true
            }
            Err(e) => {
                tracing::warn!("Telegram Proxy was not restarted: {e}");
                false
            }
        }
    }

    pub fn view(&self, settings: &TelegramProxySettings) -> TelegramView {
        let status = self.manager.status();
        let component = match self.component() {
            Ok(c) => ComponentView {
                installed: true,
                version: Some(c.manifest.version),
                upstream_commit: Some(c.manifest.upstream_commit),
                problem: None,
            },
            Err(ComponentProblem::NotInstalled) => ComponentView::default(),
            Err(ComponentProblem::Damaged(m)) => ComponentView {
                problem: Some(m),
                ..Default::default()
            },
        };
        let running = self.running.lock().expect("lock").clone();
        TelegramView {
            connection_port: bridge_telegram::manager::connection_port(
                settings.port,
                self.manager.running_port(),
            ),
            pending_restart: self.manager.is_running() && running.as_ref() != Some(settings),
            auto_restart_pending: self.supervisor.lock().expect("lock").pending(),
            status,
            component,
            settings: settings.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentView {
    pub installed: bool,
    pub version: Option<String>,
    pub upstream_commit: Option<String>,
    /// Present but failed the integrity check.
    pub problem: Option<String>,
}

/// What the UI gets: never the secret.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramView {
    pub status: ProxyStatus,
    pub component: ComponentView,
    pub settings: TelegramProxySettings,
    /// Running with older settings: a restart applies the saved ones.
    pub pending_restart: bool,
    /// The proxy exited unexpectedly and Bridge is about to start it again.
    pub auto_restart_pending: bool,
    /// The port Telegram must use now (the running process's port).
    pub connection_port: u16,
}

/// The port Telegram must use now: the running process's port, or the saved
/// one when the proxy is not running (see `connection_port`).
fn current_port(state: &AppState) -> u16 {
    bridge_telegram::manager::connection_port(
        state.settings().telegram_proxy.port,
        state.telegram.manager.running_port(),
    )
}

fn view(state: &AppState) -> TelegramView {
    state.telegram.view(&state.settings().telegram_proxy)
}

async fn blocking<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&AppState) -> T + Send + 'static,
) -> T {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .expect("telegram task")
}

#[tauri::command]
pub async fn telegram_get(state: State<'_, AppState>) -> R<TelegramView> {
    Ok(view(&state))
}

#[tauri::command]
pub async fn telegram_start(app: AppHandle) -> R<TelegramView> {
    blocking(&app, |st| {
        st.telegram.start(&st.settings().telegram_proxy)?;
        Ok(view(st))
    })
    .await
}

#[tauri::command]
pub async fn telegram_stop(app: AppHandle) -> R<TelegramView> {
    Ok(blocking(&app, |st| {
        st.telegram.stop();
        view(st)
    })
    .await)
}

#[tauri::command]
pub async fn telegram_restart(app: AppHandle) -> R<TelegramView> {
    blocking(&app, |st| {
        st.telegram.stop();
        st.telegram.start(&st.settings().telegram_proxy)?;
        Ok(view(st))
    })
    .await
}

/// The connection link (contains the secret) — only on an explicit action.
#[tauri::command]
pub async fn telegram_link(state: State<'_, AppState>) -> R<String> {
    let secret = state.telegram.store.load_or_create()?;
    Ok(link::tg_link(current_port(&state), &secret))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub opened: bool,
    /// `noTelegram`: no application handles tg:// links.
    pub reason: Option<String>,
}

/// Opens the link in Telegram Desktop. Only the link built here (localhost,
/// the configured port, a valid secret) is ever opened.
#[tauri::command]
pub async fn telegram_open(app: AppHandle, state: State<'_, AppState>) -> R<OpenResult> {
    if !tg_handler_registered() {
        return Ok(OpenResult {
            opened: false,
            reason: Some("noTelegram".into()),
        });
    }
    let secret = state.telegram.store.load_or_create()?;
    let l = link::tg_link(current_port(&state), &secret);
    link::validate_tg_link(&l)?;
    app.opener()
        .open_url(l, None::<&str>)
        .map_err(|_| "не удалось передать ссылку Telegram".to_string())?;
    Ok(OpenResult {
        opened: true,
        reason: None,
    })
}

/// New secret (the old link stops working). Restarts a running proxy.
#[tauri::command]
pub async fn telegram_regenerate_secret(app: AppHandle) -> R<TelegramView> {
    blocking(&app, |st| {
        let was_running = st.telegram.manager.is_running();
        if was_running {
            st.telegram.stop();
        }
        st.telegram.store.regenerate()?;
        if was_running {
            st.telegram.start(&st.settings().telegram_proxy)?;
        }
        Ok(view(st))
    })
    .await
}

#[cfg(windows)]
fn tg_handler_registered() -> bool {
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CLASSES_ROOT, KEY_READ,
    };
    let path: Vec<u16> = "tg\\shell\\open\\command"
        .encode_utf16()
        .chain([0])
        .collect();
    let mut key: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(HKEY_CLASSES_ROOT, path.as_ptr(), 0, KEY_READ, &mut key) == 0 {
            RegCloseKey(key);
            return true;
        }
    }
    false
}

#[cfg(not(windows))]
fn tg_handler_registered() -> bool {
    true
}

// ---------------------------------------------------------------- diagnostics

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagItem {
    pub id: String,
    /// ok | warn | bad | unknown ("not checked")
    pub status: String,
    pub title: String,
    pub detail: String,
}

fn item(id: &str, status: &str, title: &str, detail: impl Into<String>) -> DiagItem {
    DiagItem {
        id: id.into(),
        status: status.into(),
        title: title.into(),
        detail: detail.into(),
    }
}

fn ipv4(s: &str) -> Option<u32> {
    let p: Vec<u8> = s
        .split('.')
        .map(|x| x.parse().ok())
        .collect::<Option<_>>()?;
    (p.len() == 4).then(|| u32::from_be_bytes([p[0], p[1], p[2], p[3]]))
}

/// Does an IPv4 list (one network per line) contain `addr`?
fn list_contains(text: &str, addr: u32) -> bool {
    text.lines().any(|l| {
        let l = l.trim();
        let (net, bits) = l.split_once('/').unwrap_or((l, "32"));
        match (ipv4(net), bits.parse::<u32>()) {
            (Some(n), Ok(b)) if b <= 32 => {
                let mask = if b == 0 { 0 } else { u32::MAX << (32 - b) };
                n & mask == addr & mask
            }
            _ => false,
        }
    })
}

/// Telegram's WebSocket endpoints the engine connects to (kws*.web.telegram.org)
/// and the default DC address it uses.
const TELEGRAM_WS_HOST: &str = "kws2.web.telegram.org";
const TELEGRAM_DC_IP: &str = "149.154.167.220";

/// Real checks. What cannot be established reliably is "unknown" (not
/// checked), never "ok". Nothing sensitive (secret, peers) is included.
#[tauri::command]
pub async fn telegram_diagnostics(app: AppHandle, state: State<'_, AppState>) -> R<Vec<DiagItem>> {
    let st = state.telegram.manager.status();
    let s = state.settings();
    let mut out = Vec::new();
    let running = state.telegram.manager.is_running();
    out.push(if running {
        item(
            "process",
            "ok",
            "Процесс Telegram Proxy",
            format!("запущен Bridge (PID {})", st.pid.unwrap_or_default()),
        )
    } else if st.state == ProxyState::NotInstalled {
        item(
            "process",
            "bad",
            "Процесс Telegram Proxy",
            "компонент не установлен или повреждён",
        )
    } else {
        item("process", "warn", "Процесс Telegram Proxy", "не запущен")
    });
    // The port the process really listens on (a saved but not yet applied
    // port is reported separately).
    let port = current_port(&state);
    let open = {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(800)).is_ok()
    };
    out.push(match (running, open, st.listening) {
        (true, true, true) => item(
            "port",
            "ok",
            "Локальный порт",
            format!("127.0.0.1:{port} принимает подключения"),
        ),
        (true, _, _) => item(
            "port",
            "bad",
            "Локальный порт",
            format!("127.0.0.1:{port} не принимает подключения"),
        ),
        (false, true, _) => item(
            "port",
            "warn",
            "Локальный порт",
            format!("127.0.0.1:{port} занят другой программой"),
        ),
        (false, false, _) => item(
            "port",
            "unknown",
            "Локальный порт",
            format!("127.0.0.1:{port} свободен (прокси не запущен)"),
        ),
    });
    out.push(match state.telegram.manager.listener_is_ours() {
        Some(true) => item(
            "owner",
            "ok",
            "Владелец порта",
            "порт слушает процесс, запущенный Bridge",
        ),
        Some(false) => item(
            "owner",
            "bad",
            "Владелец порта",
            "порт слушает чужой процесс",
        ),
        None => item("owner", "unknown", "Владелец порта", "не проверено"),
    });
    out.push(if st.relays_active > 0 {
        item(
            "client",
            "ok",
            "Подключение Telegram",
            format!(
                "сейчас передаются авторизованные соединения клиента: {}",
                st.relays_active
            ),
        )
    } else if running && st.clients_active > 0 && st.stats.connections_bad > 0 {
        item(
            "client",
            "warn",
            "Подключение Telegram",
            "к прокси подключаются с неверным секретом или протоколом — обновите ссылку в Telegram",
        )
    } else if running && st.relayed > 0 {
        item(
            "client",
            "unknown",
            "Подключение Telegram",
            format!(
                "сейчас не подключён; с момента запуска передано соединений: {}",
                st.relayed
            ),
        )
    } else if running {
        item(
            "client",
            "unknown",
            "Подключение Telegram",
            "не подключён (не проверено)",
        )
    } else {
        item("client", "unknown", "Подключение Telegram", "не проверено")
    });
    if let Some(rp) = state.telegram.manager.running_port() {
        if rp != s.telegram_proxy.port {
            out.push(item("pending", "warn", "Изменённый порт", format!("сохранён порт {}, но прокси работает на {rp} — ссылка указывает на {rp}; перезапустите прокси, чтобы применить новый порт", s.telegram_proxy.port)));
        }
    }
    out.push(if st.data_from_telegram {
        item(
            "data",
            "ok",
            "Обмен данными с Telegram",
            "данные от серверов Telegram доходят до клиента",
        )
    } else {
        item(
            "data",
            "unknown",
            "Обмен данными с Telegram",
            "не проверено (нет подтверждённой передачи)",
        )
    });
    let x = st.stats;
    let used: Vec<String> = [
        ("WebSocket напрямую", x.connections_ws),
        ("TCP напрямую (резерв)", x.connections_tcp_fallback),
        ("Cloudflare (CfProxy)", x.connections_cfproxy),
        ("HTTP/2 через Cloudflare", x.connections_h2),
        ("domain fronting", x.connections_fronting),
    ]
    .iter()
    .filter(|(_, n)| *n > 0)
    .map(|(name, n)| format!("{name}: {n}"))
    .collect();
    out.push(if used.is_empty() {
        item(
            "transport",
            "unknown",
            "Использованный маршрут",
            "не проверено (соединений ещё не было)",
        )
    } else {
        item(
            "transport",
            if x.connections_cfproxy + x.connections_h2 > 0 {
                "warn"
            } else {
                "ok"
            },
            "Использованный маршрут",
            used.join(", "),
        )
    });
    out.push(if s.telegram_proxy.fallback_cfproxy {
        item("fallback", "warn", "Резервный маршрут", "включён: при недоступности прямого WebSocket соединения идут через узлы Cloudflare; они видят метаданные (адреса, объём), но не содержимое MTProto")
    } else {
        item("fallback", "ok", "Резервный маршрут", "выключен: только прямые соединения с Telegram")
    });
    out.push(zapret_overlap(&app, &state, &s).await);
    Ok(out)
}

/// Can the running WinWS also handle the proxy's own connections to
/// Telegram? Independent control does not mean independent traffic.
async fn zapret_overlap(
    _app: &AppHandle,
    state: &AppState,
    s: &crate::configuration::Settings,
) -> DiagItem {
    let title = "Пересечение с Zapret";
    let engine = crate::zapret::engine_status(state).await.ok();
    if !engine
        .as_ref()
        .is_some_and(|e| e.state == EngineState::Running)
    {
        return item(
            "zapret",
            "ok",
            title,
            "Zapret выключен — его фильтры не затрагивают Telegram Proxy",
        );
    }
    let lists = bridge_core::lists::effective_lists(&s.user_lists, &s.installed_packs);
    let mut why = Vec::new();
    if let Some(d) = lists.general.iter().find(|d| {
        let d = d.trim().trim_start_matches('^');
        !d.starts_with('#') && bridge_core::services::covers(d, false, TELEGRAM_WS_HOST)
    }) {
        why.push(format!("в ваших списках есть «{d}» — WinWS обрабатывает соединения прокси с {TELEGRAM_WS_HOST} на портах своих правил"));
    }
    let (_, lists_files, _) = crate::services::active_lists(state).await;
    let backup = lists_files.get("ipset-all.txt.backup").map(String::as_str);
    if let Ok(files) = bridge_core::lists::runtime_list_files(&lists, s.ipset_mode, backup) {
        if let Some((_, ipset)) = files.iter().find(|(n, _)| *n == "ipset-all.txt") {
            let dc = ipv4(TELEGRAM_DC_IP).unwrap_or(0);
            if ipset.trim().is_empty() {
                why.push(
                    "режим IPSet «все адреса» — WinWS обрабатывает и соединения прокси к Telegram"
                        .into(),
                );
            } else if list_contains(ipset, dc) {
                why.push(format!("IPSet содержит адрес Telegram {TELEGRAM_DC_IP} — правила с IPSet обрабатывают соединения прокси"));
            }
        }
    }
    if why.is_empty() {
        item("zapret", "ok", title, "Zapret включён, но адресов Telegram в его списках и IPSet нет — соединения прокси он не обрабатывает")
    } else {
        item("zapret", "warn", title, format!("{}. Это может как помочь, так и помешать; если Telegram Proxy работает хуже при включённом Zapret, уберите Telegram из списков и IPSet.", why.join("; ")))
    }
}

/// Startup: only the proxy's own autostart setting matters (not the bypass
/// settings).
pub fn spawn_autostart(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let s = state.settings().telegram_proxy;
        if !s.autostart {
            return;
        }
        let app2 = app.clone();
        let r = blocking(&app2, move |st| st.telegram.start(&s)).await;
        match r {
            Ok(()) => tracing::info!("Telegram Proxy started at startup"),
            Err(e) => tracing::warn!("Telegram Proxy autostart failed: {e}"),
        }
    });
}

/// Watches the proxy and restarts it after an unexpected exit (see
/// `TelegramRuntime::supervise`). Ends when the app is quitting.
pub fn spawn_supervisor(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let state = app.state::<AppState>();
        if state.quitting.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let before = state.telegram.supervisor.lock().expect("lock").pending();
        let restarted = state.telegram.supervise();
        let after = state.telegram.supervisor.lock().expect("lock").pending();
        if restarted || before != after {
            let _ = app.emit("telegram-changed", ());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipset_membership_for_the_telegram_dc() {
        let dc = ipv4(TELEGRAM_DC_IP).unwrap();
        assert!(list_contains("1.0.0.0/24\n149.154.160.0/20\n", dc));
        assert!(!list_contains("203.0.113.113/32\n", dc));
        assert!(list_contains("149.154.167.220\n", dc));
        assert!(!list_contains("garbage\n::1\n", dc));
    }

    #[test]
    fn the_compiled_hash_is_absent_or_a_sha256() {
        if let Some(h) = COMPILED_SHA256 {
            assert_eq!(h.len(), 64);
            assert!(h.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }
}
