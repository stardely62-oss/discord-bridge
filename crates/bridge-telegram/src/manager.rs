//! Lifecycle of the engine process. The state always comes from the real
//! child process and what it reports, never from a UI flag.
//!
//! * start: verify the executable, refuse if the port is taken (the other
//!   program is never stopped), spawn without a shell — on Windows suspended,
//!   placed in a kill-on-close Job Object, then resumed — and send the
//!   configuration (with the secret) as one line on stdin;
//! * stop: close stdin (graceful), wait, then terminate the job / child;
//! * an unexpected exit becomes `error` with the reason; there is no automatic
//!   restart (no restart loops).

use crate::component::Component;
use crate::protocol::{parse_line, EngineEvent, EngineStats};
use crate::secret::Secret;
use crate::settings::TelegramProxySettings;
use crate::LISTEN_HOST;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How long the engine may take to start listening.
pub const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a graceful stop may take before the process is terminated.
pub const STOP_TIMEOUT: Duration = Duration::from_secs(6);
const LOG_LINES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ProxyState {
    /// The engine is not part of this installation (or is damaged).
    NotInstalled,
    #[default]
    Stopped,
    /// Process started, not listening yet.
    Starting,
    /// Accepting connections on the local port.
    Listening,
    /// A client authenticated with our secret has a connection open now and
    /// at least one connection was relayed towards Telegram.
    Connected,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub state: ProxyState,
    pub version: Option<String>,
    pub host: String,
    pub port: u16,
    pub pid: Option<u32>,
    /// Unix seconds.
    pub started_at: Option<u64>,
    /// The engine reports its listening socket alive.
    pub listening: bool,
    /// Client connections open now (also ones that failed the handshake).
    pub clients_active: u64,
    /// Client connections relayed towards Telegram since the start.
    pub relayed: u64,
    /// Bytes came back from Telegram to a client (data really flowed).
    pub data_from_telegram: bool,
    pub stats: EngineStats,
    /// The relay fallback (Cloudflare) is allowed in this run.
    pub fallback_cfproxy: bool,
    pub error_code: Option<String>,
    pub last_error: Option<String>,
    pub exit_code: Option<i32>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

struct Run {
    child: Child,
    stdin: Option<ChildStdin>,
    stopping: bool,
    started: Instant,
    #[cfg(windows)]
    job: crate::win::Job,
}

impl Run {
    fn kill(&mut self) {
        #[cfg(windows)]
        self.job.terminate();
        let _ = self.child.kill();
    }
}

#[derive(Default)]
struct Inner {
    status: ProxyStatus,
    run: Option<Run>,
    generation: u64,
    log: VecDeque<String>,
    /// For redacting any engine output.
    secret: Option<Secret>,
}

type Notify = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct ProxyManager {
    inner: Arc<Mutex<Inner>>,
    notify: Notify,
}

fn derive_state(s: &ProxyStatus) -> ProxyState {
    if s.listening && s.clients_active > 0 && s.relayed > 0 {
        ProxyState::Connected
    } else if s.listening {
        ProxyState::Listening
    } else {
        ProxyState::Starting
    }
}

fn exit_reason(code: Option<i32>) -> (&'static str, String) {
    match code {
        Some(2) => ("config", "движок отклонил настройки".into()),
        Some(3) => ("addressInUse", "порт уже занят другой программой".into()),
        Some(4) => ("listen", "не удалось открыть локальный порт".into()),
        Some(c) => (
            "exited",
            format!("процесс Telegram Proxy завершился неожиданно (код {c})"),
        ),
        None => (
            "exited",
            "процесс Telegram Proxy завершился неожиданно".into(),
        ),
    }
}

impl ProxyManager {
    /// `notify` is called (outside the lock) after every meaningful change.
    pub fn new(notify: impl Fn() + Send + Sync + 'static) -> ProxyManager {
        ProxyManager {
            inner: Arc::new(Mutex::new(Inner::default())),
            notify: Arc::new(notify),
        }
    }

    pub fn status(&self) -> ProxyStatus {
        self.inner.lock().expect("lock").status.clone()
    }

    /// Marks the component as missing / damaged (shown instead of `stopped`).
    pub fn set_not_installed(&self, reason: Option<String>) {
        let mut g = self.inner.lock().expect("lock");
        if g.run.is_none() {
            g.status = ProxyStatus {
                state: ProxyState::NotInstalled,
                last_error: reason,
                ..Default::default()
            };
        }
        drop(g);
        (self.notify)();
    }

    pub fn is_running(&self) -> bool {
        self.inner.lock().expect("lock").run.is_some()
    }

    /// Last lines of the engine log (secret redacted).
    pub fn log_tail(&self) -> Vec<String> {
        self.inner
            .lock()
            .expect("lock")
            .log
            .iter()
            .cloned()
            .collect()
    }

    /// Process ids that belong to this run (the bootloader and the Python
    /// process on Windows).
    pub fn owned_pids(&self) -> Vec<u32> {
        let g = self.inner.lock().expect("lock");
        let Some(r) = &g.run else { return vec![] };
        #[cfg(windows)]
        {
            let mut p = r.job.pids();
            if !p.contains(&r.child.id()) {
                p.push(r.child.id());
            }
            p
        }
        #[cfg(not(windows))]
        vec![r.child.id()]
    }

    pub fn start(
        &self,
        component: &Component,
        expected_sha256: &str,
        settings: &TelegramProxySettings,
        secret: &Secret,
    ) -> Result<(), String> {
        let mut g = self.inner.lock().expect("lock");
        if g.run.is_some() {
            return Err("Telegram Proxy уже запущен".into());
        }
        let fail = |g: &mut Inner, code: &str, msg: String| {
            g.status = ProxyStatus {
                state: ProxyState::Error,
                host: LISTEN_HOST.into(),
                port: settings.port,
                version: Some(component.manifest.version.clone()),
                error_code: Some(code.into()),
                last_error: Some(msg.clone()),
                ..Default::default()
            };
            Err(msg)
        };
        if let Err(e) = settings.validate() {
            let r = fail(&mut g, "config", e);
            drop(g);
            (self.notify)();
            return r;
        }
        if let Err(p) = component.verify(expected_sha256) {
            let msg = match p {
                crate::ComponentProblem::Damaged(m) => m,
                crate::ComponentProblem::NotInstalled => "компонент не установлен".into(),
            };
            let r = fail(&mut g, "integrity", msg);
            drop(g);
            (self.notify)();
            return r;
        }
        // Never take over or stop another program's listener.
        match std::net::TcpListener::bind((LISTEN_HOST, settings.port)) {
            Ok(l) => drop(l),
            Err(_) => {
                let r = fail(
                    &mut g,
                    "addressInUse",
                    format!("порт {} уже занят другой программой — выберите другой порт; Discord Bridge её не останавливает", settings.port),
                );
                drop(g);
                (self.notify)();
                return r;
            }
        }
        let mut cmd = Command::new(&component.exe);
        cmd.arg("--stdin-config")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for v in [
            "PYTHONPATH",
            "PYTHONHOME",
            "PYTHONSTARTUP",
            "PYTHONINSPECT",
            "PYTHONUSERBASE",
            "PYTHONDEBUG",
            "PYTHONVERBOSE",
        ] {
            cmd.env_remove(v);
        }
        if let Some(dir) = component.exe.parent() {
            cmd.current_dir(dir);
        }
        #[cfg(windows)]
        let job = {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(crate::win::CREATE_FLAGS);
            match crate::win::Job::new() {
                Ok(j) => j,
                Err(e) => {
                    let r = fail(&mut g, "spawn", format!("Job Object: {e}"));
                    drop(g);
                    (self.notify)();
                    return r;
                }
            }
        };
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let r = fail(
                    &mut g,
                    "spawn",
                    format!("не удалось запустить Telegram Proxy: {e}"),
                );
                drop(g);
                (self.notify)();
                return r;
            }
        };
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            let placed = job
                .assign(child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE)
                .and_then(|_| crate::win::resume_process(child.id()));
            if let Err(e) = placed {
                let _ = child.kill();
                let _ = child.wait();
                let r = fail(
                    &mut g,
                    "spawn",
                    format!("процесс не помещён под контроль Discord Bridge: {e}"),
                );
                drop(g);
                (self.notify)();
                return r;
            }
        }
        let mut stdin = child.stdin.take();
        let sent = stdin.as_mut().map(|s| {
            s.write_all(format!("{}\n", settings.engine_config(secret)).as_bytes())
                .and_then(|_| s.flush())
        });
        if !matches!(sent, Some(Ok(()))) {
            #[cfg(windows)]
            job.terminate();
            let _ = child.kill();
            let _ = child.wait();
            let r = fail(
                &mut g,
                "spawn",
                "не удалось передать настройки процессу Telegram Proxy".into(),
            );
            drop(g);
            (self.notify)();
            return r;
        }
        g.generation += 1;
        let generation = g.generation;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        g.secret = Some(secret.clone());
        g.log.clear();
        g.status = ProxyStatus {
            state: ProxyState::Starting,
            version: Some(component.manifest.version.clone()),
            host: LISTEN_HOST.into(),
            port: settings.port,
            pid: Some(child.id()),
            started_at: Some(now()),
            fallback_cfproxy: settings.fallback_cfproxy,
            ..Default::default()
        };
        g.run = Some(Run {
            child,
            stdin,
            stopping: false,
            started: Instant::now(),
            #[cfg(windows)]
            job,
        });
        drop(g);
        if let Some(out) = stdout {
            let me = self.clone();
            std::thread::Builder::new()
                .name("tgproxy-stdout".into())
                .spawn(move || me.read_stdout(generation, out))
                .ok();
        }
        if let Some(err) = stderr {
            let me = self.clone();
            std::thread::Builder::new()
                .name("tgproxy-stderr".into())
                .spawn(move || me.read_stderr(generation, err))
                .ok();
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("tgproxy-watch".into())
            .spawn(move || me.watch(generation))
            .ok();
        (self.notify)();
        Ok(())
    }

    fn read_stdout(&self, generation: u64, out: std::process::ChildStdout) {
        for line in BufReader::new(out).lines() {
            let Ok(line) = line else { break };
            let Some(ev) = parse_line(&line) else {
                continue;
            };
            let changed = {
                let mut g = self.inner.lock().expect("lock");
                if g.generation != generation || g.run.is_none() {
                    break;
                }
                let before = g.status.clone();
                let s = &mut g.status;
                match ev {
                    EngineEvent::Ready {
                        fallback_cfproxy,
                        version,
                        ..
                    } => {
                        s.listening = true;
                        s.fallback_cfproxy = fallback_cfproxy;
                        s.version = Some(version);
                    }
                    EngineEvent::Status { listening, stats } => {
                        s.listening = listening;
                        apply_stats(s, stats);
                    }
                    EngineEvent::Stopped { stats } => {
                        s.listening = false;
                        apply_stats(s, stats);
                    }
                    EngineEvent::Error { code, message } => {
                        s.error_code = Some(code);
                        s.last_error = Some(message);
                    }
                }
                if s.state != ProxyState::Error {
                    s.state = derive_state(s);
                }
                meaningful(&before, &g.status)
            };
            if changed {
                (self.notify)();
            }
        }
    }

    fn read_stderr(&self, generation: u64, err: std::process::ChildStderr) {
        for line in BufReader::new(err).lines() {
            let Ok(line) = line else { break };
            let mut g = self.inner.lock().expect("lock");
            if g.generation != generation {
                break;
            }
            let red = match &g.secret {
                Some(s) => s.redact(&line),
                None => line,
            };
            let red: String = red.chars().take(500).collect();
            g.log.push_back(red);
            while g.log.len() > LOG_LINES {
                g.log.pop_front();
            }
        }
    }

    /// Detects the exit of the process (requested or not) and start time-outs.
    fn watch(&self, generation: u64) {
        loop {
            std::thread::sleep(Duration::from_millis(200));
            let mut g = self.inner.lock().expect("lock");
            if g.generation != generation {
                return;
            }
            let Some(run) = g.run.as_mut() else { return };
            match run.child.try_wait() {
                Ok(Some(exit)) => {
                    let stopping = run.stopping;
                    #[cfg(windows)]
                    run.job.terminate(); // the Python child of the bootloader too
                    let code = exit.code();
                    let run_status = g.status.clone();
                    g.run = None;
                    g.status = if stopping {
                        ProxyStatus {
                            state: ProxyState::Stopped,
                            version: run_status.version,
                            host: run_status.host,
                            port: run_status.port,
                            exit_code: code,
                            ..Default::default()
                        }
                    } else {
                        let (c, m) = exit_reason(code);
                        ProxyStatus {
                            state: ProxyState::Error,
                            error_code: run_status.error_code.clone().or(Some(c.into())),
                            last_error: Some(match run_status.last_error {
                                Some(e) if !e.is_empty() => format!("{m}: {e}"),
                                _ => m,
                            }),
                            exit_code: code,
                            listening: false,
                            pid: None,
                            ..run_status
                        }
                    };
                    drop(g);
                    (self.notify)();
                    return;
                }
                Ok(None) => {
                    let timed_out = !run.stopping && run.started.elapsed() > START_TIMEOUT;
                    if timed_out && g.status.state == ProxyState::Starting {
                        if let Some(run) = g.run.as_mut() {
                            run.kill();
                        }
                        g.status.error_code = Some("startTimeout".into());
                        g.status.last_error = Some(
                            "Telegram Proxy не начал принимать подключения за 30 секунд".into(),
                        );
                    }
                }
                Err(_) => {}
            }
        }
    }

    /// Graceful stop (stdin closed), then termination after `STOP_TIMEOUT`.
    /// Returns when the process has ended.
    pub fn stop(&self) {
        {
            let mut g = self.inner.lock().expect("lock");
            let Some(run) = g.run.as_mut() else { return };
            run.stopping = true;
            if let Some(mut s) = run.stdin.take() {
                let _ = s.write_all(b"stop\n");
                let _ = s.flush();
            }
        }
        let deadline = Instant::now() + STOP_TIMEOUT;
        while Instant::now() < deadline {
            if !self.is_running() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some(run) = self.inner.lock().expect("lock").run.as_mut() {
            run.kill();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && self.is_running() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn restart(
        &self,
        component: &Component,
        expected_sha256: &str,
        settings: &TelegramProxySettings,
        secret: &Secret,
    ) -> Result<(), String> {
        self.stop();
        self.start(component, expected_sha256, settings, secret)
    }

    /// Does the listener on the port belong to this run? None: unknown here.
    pub fn listener_is_ours(&self) -> Option<bool> {
        #[cfg(windows)]
        {
            let port = self.status().port;
            let owner = crate::win::listener_pid(port)?;
            Some(self.owned_pids().contains(&owner))
        }
        #[cfg(not(windows))]
        None
    }
}

fn apply_stats(s: &mut ProxyStatus, stats: EngineStats) {
    s.stats = stats;
    s.clients_active = stats.connections_active;
    s.relayed = stats.relayed();
    s.data_from_telegram = stats.bytes_down > 0 && stats.relayed() > 0;
}

/// Changes worth a UI update (not every counter tick).
fn meaningful(a: &ProxyStatus, b: &ProxyStatus) -> bool {
    a.state != b.state
        || a.listening != b.listening
        || a.clients_active != b.clients_active
        || (a.relayed == 0) != (b.relayed == 0)
        || a.data_from_telegram != b.data_from_telegram
        || a.last_error != b.last_error
        || a.version != b.version
}
