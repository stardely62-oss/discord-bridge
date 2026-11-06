//! Named-pipe server and command dispatcher.

use crate::bundles::{self, Bundle};
use crate::engine::{Engine, Resolver};
use crate::hstate::{self, HelperState};
use crate::paths::Paths;
use crate::policy::{
    authorize, decide_tcp_change, ClientIdentity, ConnectionLimiter, Owner, TcpDecision,
};
use crate::txn::Controller;
use crate::win::{
    client_identity, process_image, read_file_as_client, same_path, session_alive, wide, Owned,
    SecDesc,
};
use crate::{info, system, warn};
use bridge_core::ipc::{
    self, BundleInfo, EngineState, EngineStatus, HelperDiagnostics, Request, Response,
    RuntimeOptions, SwitchReport, SwitchResult, PIPE_NAME, PROTOCOL_VERSION,
};
use bridge_core::lists::runtime_list_files;
use bridge_core::manifest::TRUSTED_KEYS;
use bridge_core::strategy::render::render_args;
use bridge_core::strategy::validate::validate;
use bridge_core::strategy::Strategy;
use std::fs::File;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    FlushFileBuffers, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
    PIPE_WAIT,
};

/// Owner SYSTEM (explicit: a LocalSystem token defaults to Administrators as owner).
/// SYSTEM and Administrators: full; interactive users: read + write data,
/// but *not* FILE_CREATE_PIPE_INSTANCE (0x4), so they cannot add server
/// instances to our pipe name.
const PIPE_SDDL: &str = "O:SYD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;IU)";

/// Concurrent clients served at once; further connections are refused.
const MAX_CLIENTS: usize = 8;
/// A connection is force-closed after this time, so a silent or stuck
/// client cannot hold a slot forever.
const CONNECTION_LIFETIME: Duration = Duration::from_secs(90);
/// Largest update archive accepted.
const MAX_ARCHIVE: u64 = 64 * 1024 * 1024;

pub struct Helper {
    pub paths: Paths,
    pub ctl: Mutex<Controller<Engine>>,
    state: Mutex<HelperState>,
    owner: Mutex<Option<Owner>>,
    last_failure: Mutex<Option<String>>,
    limiter: Arc<ConnectionLimiter>,
    /// Successful Hello handshakes (one per authenticated connection).
    handshakes: AtomicU64,
}

struct Ctx {
    pipe: HANDLE,
    client: ClientIdentity,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Helper {
    pub fn new(paths: Paths) -> Self {
        let state = HelperState::load(&paths.state_file());
        let ctl = Controller::new(Engine::default(), paths.runtime_root());
        Helper {
            paths,
            ctl: Mutex::new(ctl),
            state: Mutex::new(state),
            owner: Mutex::new(None),
            last_failure: Mutex::new(None),
            limiter: Arc::new(ConnectionLimiter::new(MAX_CLIENTS)),
            handshakes: AtomicU64::new(0),
        }
    }

    /// Number of successful Hello handshakes since start (diagnostics, tests).
    pub fn handshakes(&self) -> u64 {
        self.handshakes.load(Ordering::Relaxed)
    }

    fn bundles(&self) -> Vec<Bundle> {
        bundles::discover(&self.paths.builtin_bundles(), &self.paths.updates_dir())
    }

    fn active_bundle(&self) -> Option<Bundle> {
        let all = self.bundles();
        let want = lock(&self.state).active_bundle.clone();
        if let Some(v) = want {
            if let Some(b) = all.iter().find(|b| b.version() == v) {
                return Some(b.clone());
            }
        }
        all.iter()
            .find(|b| b.builtin)
            .cloned()
            .or_else(|| all.first().cloned())
    }

    fn save_state(&self) -> Result<(), String> {
        lock(&self.state).save(&self.paths.state_file())
    }

    /// Writes every list the engine reads into `dir` (a fresh generation).
    fn materialize_lists(bundle: &Bundle, rt: &RuntimeOptions, dir: &Path) -> Result<(), String> {
        for f in bundle
            .manifest
            .files
            .iter()
            .filter(|f| f.path.starts_with("lists/"))
        {
            let name = &f.path["lists/".len()..];
            let data =
                std::fs::read(bundle.path_of(&f.path)).map_err(|e| format!("{}: {e}", f.path))?;
            bundle
                .manifest
                .verify_file(&f.path, &data)
                .map_err(|e| e.to_string())?;
            std::fs::write(dir.join(name), data).map_err(|e| e.to_string())?;
        }
        // User lists and ipset-all.txt (IP sets: user + catalog packs +
        // Flowseal backup, exclusions win) — built and validated in bridge-core.
        let backup = bundle.read_backup_ipset();
        for (name, content) in runtime_list_files(&rt.user_lists, rt.ipset_mode, backup.as_deref())?
        {
            std::fs::write(dir.join(name), content).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn status(&self, client: Option<&ClientIdentity>) -> EngineStatus {
        let mut ctl = lock(&self.ctl);
        if let Some(dead) = ctl.reap_if_dead() {
            warn!("engine for {} exited unexpectedly", dead.strategy.id);
            *lock(&self.owner) = None;
        }
        let mut st = ctl.backend.status();
        let cur = ctl.current();
        st.strategy_id = cur.map(|c| c.strategy.id.clone());
        st.strategy_name = cur.map(|c| c.strategy.name.clone());
        st.config_hash = cur.map(|c| c.config_hash.clone());
        st.bundle_version = cur
            .map(|c| c.bundle.version().to_string())
            .or(st.bundle_version);
        st.trial_active = ctl.trial_active();
        if st.state != EngineState::Running {
            if let Some(f) = lock(&self.last_failure).clone() {
                st.state = EngineState::Failed;
                st.last_error = st.last_error.or(Some(f));
            }
        }
        let owner = lock(&self.owner).clone();
        st.owner_sid = owner.as_ref().map(|o| o.sid.clone());
        st.can_control = client
            .is_some_and(|c| authorize(&Request::Stop, c, owner.as_ref(), session_alive).is_ok());
        st
    }

    fn start(&self, strategy: &Strategy, rt: &RuntimeOptions, ctx: &Ctx) -> Response {
        let Some(bundle) = self.active_bundle() else {
            return err(
                "no_bundle",
                "компоненты Flowseal не найдены — переустановите Discord Bridge",
            );
        };
        let report = validate(strategy, Some(&bundle.resource_index()));
        if report.has_errors() {
            return Response::Invalid {
                issues: report.issues,
            };
        }
        if let Err(e) = rt.game_filter.validate() {
            return err("bad_game_filter", &e.to_string());
        }
        let mut ctl = lock(&self.ctl);
        if !rt.ignore_conflicts {
            let conflicts = system::find_conflicts(|pid| ctl.backend.owns(pid));
            if !conflicts.is_empty() {
                return Response::Conflicts { conflicts };
            }
        }
        // Everything that can fail is done before the running engine is touched.
        let problems = bundle.check_integrity();
        if !problems.is_empty() {
            return err(
                "integrity",
                &format!(
                    "целостность Flowseal {} нарушена: {}",
                    bundle.version(),
                    problems.join("; ")
                ),
            );
        }
        // Run from an immutable copy under ProgramData, never from the install
        // directory: the installer can then replace its files at any time.
        let bundle = match crate::stage::stage_bundle(&bundle, &self.paths.engines_dir()) {
            Ok(b) => b,
            Err(e) => return err("stage", &e),
        };
        let gf = rt.game_filter.ports();
        let spec = ctl.prepare(
            strategy.clone(),
            rt.clone(),
            bundle.clone(),
            Some(ctx.client.sid.clone()),
            |dir| Self::materialize_lists(&bundle, rt, dir),
            |dir| {
                render_args(
                    strategy,
                    &gf,
                    &Resolver {
                        bin: bundle.bin_dir(),
                        lists: dir.to_path_buf(),
                    },
                )
                .map_err(|e| e.to_string())
            },
        );
        let spec = match spec {
            Ok(s) => s,
            Err(e) => {
                warn!("prepare failed for {}: {e}", strategy.id);
                let running_name = ctl.current().map(|c| c.strategy.name.clone());
                drop(ctl);
                return Response::Switched {
                    report: SwitchReport {
                        result: SwitchResult::PrepareFailed,
                        error: Some(e),
                        rollback_error: None,
                        running_name,
                    },
                    status: self.status(Some(&ctx.client)),
                };
            }
        };
        let report = ctl.switch(spec);
        match report.result {
            SwitchResult::Started => {
                *lock(&self.owner) = Some(Owner {
                    sid: ctx.client.sid.clone(),
                    session_id: ctx.client.session_id,
                });
                *lock(&self.last_failure) = None;
                info!("strategy {} started", strategy.id);
            }
            SwitchResult::RolledBack => {
                *lock(&self.last_failure) = None;
                warn!(
                    "strategy {} failed, previous configuration restored: {:?}",
                    strategy.id, report.error
                );
            }
            SwitchResult::RollbackFailed | SwitchResult::Failed => {
                *lock(&self.owner) = None;
                *lock(&self.last_failure) = report.error.clone();
                warn!(
                    "strategy {} failed: {:?} / rollback {:?}",
                    strategy.id, report.error, report.rollback_error
                );
            }
            SwitchResult::PrepareFailed => {}
        }
        drop(ctl);
        Response::Switched {
            report,
            status: self.status(Some(&ctx.client)),
        }
    }

    fn diagnose(&self) -> HelperDiagnostics {
        let bundle = self.active_bundle();
        let problems = bundle
            .as_ref()
            .map(|b| b.check_integrity())
            .unwrap_or_else(|| vec!["набор Flowseal не найден".into()]);
        let ctl = lock(&self.ctl);
        HelperDiagnostics {
            helper_version: env!("CARGO_PKG_VERSION").into(),
            bundle_version: bundle.as_ref().map(|b| b.version().to_string()),
            integrity_ok: problems.is_empty(),
            integrity_problems: problems,
            tcp_timestamps: system::tcp_timestamps(),
            base_filtering_engine_running: system::service_state("BFE")
                .map(|s| s == windows_service::service::ServiceState::Running),
            windivert_service: system::service_state("WinDivert")
                .map(|s| system::state_name(s).to_string()),
            conflicts: system::find_conflicts(|pid| ctl.backend.owns(pid)),
        }
    }

    fn list_bundles(&self) -> Vec<BundleInfo> {
        let active = self.active_bundle().map(|b| b.version().to_string());
        self.bundles()
            .into_iter()
            .map(|b| {
                let mut problems = b.check_integrity();
                if let Err(e) = bundles::check_app_compat(&b.manifest) {
                    problems.push(e);
                }
                let idx = b.resource_index();
                BundleInfo {
                    version: b.version().to_string(),
                    winws_version: b.manifest.winws_version.clone(),
                    active: active.as_deref() == Some(b.version()),
                    builtin: b.builtin,
                    ok: problems.is_empty(),
                    problems,
                    fakes: idx.fakes.into_iter().collect(),
                    lists: idx.lists.into_iter().collect(),
                    strategy_files: b.strategy_files(),
                }
            })
            .collect()
    }

    fn tcp_response(&self, message: String) -> Response {
        let changed_by_us = lock(&self.state).tcp_we_set.is_some();
        Response::TcpTimestamps {
            current: system::tcp_timestamps(),
            changed_by_us,
            message,
        }
    }

    fn set_tcp(&self, enable: bool) -> Response {
        let current = system::tcp_timestamps();
        let recorded = lock(&self.state).tcp_we_set.is_some();
        match decide_tcp_change(enable, current, recorded) {
            TcpDecision::Unknown => err(
                "tcp_unknown",
                "не удалось определить текущее состояние TCP timestamps — изменение не выполнено",
            ),
            TcpDecision::NoChange => {
                self.tcp_response("уже в нужном состоянии, ничего не изменено".into())
            }
            TcpDecision::Change { record_original } => {
                if let Err(e) = system::set_tcp_timestamps(enable) {
                    return err("netsh", &e);
                }
                {
                    let mut st = lock(&self.state);
                    if let Some(o) = record_original {
                        st.tcp_original = Some(o);
                    }
                    st.tcp_we_set = Some(enable);
                }
                if let Err(e) = self.save_state() {
                    warn!("{e}");
                }
                info!("TCP timestamps changed to {enable}");
                match system::tcp_timestamps() {
                    Some(v) if v == enable => self.tcp_response("изменено".into()),
                    other => err(
                        "tcp_verify",
                        &format!("после изменения состояние {other:?} — проверьте вручную"),
                    ),
                }
            }
        }
    }

    /// Restore TCP timestamps (IPC command). Same code path as the
    /// uninstaller's `--restore-tcp`; only the state file is touched.
    fn restore_tcp(&self) -> Response {
        let state_file = self.paths.state_file();
        let r = hstate::tcp_restore(
            &state_file,
            system::tcp_timestamps(),
            system::set_tcp_timestamps,
        );
        // Keep the in-memory copy in sync with what was written.
        *lock(&self.state) = HelperState::load(&state_file);
        match r {
            Ok(msg) => {
                info!("TCP timestamps restore: {msg}");
                self.tcp_response(msg)
            }
            Err(e) => err("tcp_changed", &e),
        }
    }

    fn handle(&self, req: Request, ctx: &Ctx) -> Response {
        let owner = lock(&self.owner).clone();
        if let Err(e) = authorize(&req, &ctx.client, owner.as_ref(), session_alive) {
            warn!("denied control request from {}", ctx.client.sid);
            return err("forbidden", &e);
        }
        match req {
            Request::Hello { protocol } => {
                if protocol != PROTOCOL_VERSION {
                    return err(
                        "protocol",
                        "версии приложения и службы не совпадают — переустановите Discord Bridge",
                    );
                }
                self.handshakes.fetch_add(1, Ordering::Relaxed);
                Response::Hello {
                    protocol: PROTOCOL_VERSION,
                    helper_version: env!("CARGO_PKG_VERSION").into(),
                }
            }
            Request::Status => Response::Status {
                status: self.status(Some(&ctx.client)),
            },
            Request::Start { strategy, runtime } => self.start(&strategy, &runtime, ctx),
            Request::Stop => {
                lock(&self.ctl).stop();
                *lock(&self.owner) = None;
                *lock(&self.last_failure) = None;
                Response::Ok
            }
            Request::BeginTrial => {
                lock(&self.ctl).begin_trial();
                Response::Ok
            }
            Request::EndTrial { commit } => {
                let report = {
                    let mut ctl = lock(&self.ctl);
                    let r = ctl.end_trial(commit);
                    if ctl.current().is_none() {
                        *lock(&self.owner) = None;
                    }
                    r
                };
                Response::Switched {
                    report,
                    status: self.status(Some(&ctx.client)),
                }
            }
            Request::Diagnose => Response::Diagnose {
                diagnostics: self.diagnose(),
            },
            Request::SetTcpTimestamps {
                enable,
                user_confirmed,
            } => {
                if !user_confirmed {
                    return err(
                        "consent",
                        "изменение системной настройки требует подтверждения пользователя",
                    );
                }
                self.set_tcp(enable)
            }
            Request::RestoreTcpTimestamps => self.restore_tcp(),
            Request::ListBundles => Response::Bundles {
                bundles: self.list_bundles(),
            },
            Request::InstallBundle {
                signed,
                archive_path,
            } => {
                let archive =
                    match read_file_as_client(ctx.pipe, Path::new(&archive_path), MAX_ARCHIVE) {
                        Ok(a) => a,
                        Err(e) => return err("archive", &e),
                    };
                match bundles::install(&signed, &archive, &self.paths.updates_dir(), TRUSTED_KEYS) {
                    Ok(v) => {
                        info!("installed Flowseal bundle {v}");
                        Response::Ok
                    }
                    Err(e) => err("install", &e.to_string()),
                }
            }
            Request::SetActiveBundle { version } => {
                let Some(b) = self.bundles().into_iter().find(|b| b.version() == version) else {
                    return err("no_bundle", "такой версии Flowseal нет");
                };
                if let Err(e) = bundles::check_app_compat(&b.manifest) {
                    return err("incompatible", &e);
                }
                let problems = b.check_integrity();
                if !problems.is_empty() {
                    return err("integrity", &problems.join("; "));
                }
                let prev = lock(&self.state).active_bundle.replace(version);
                if let Err(e) = self.save_state() {
                    lock(&self.state).active_bundle = prev;
                    return err("state", &e);
                }
                Response::Ok
            }
            Request::EngineLog => Response::EngineLog {
                lines: lock(&self.ctl).backend.log_lines(),
            },
        }
    }
}

fn err(code: &str, msg: &str) -> Response {
    Response::Error {
        code: code.into(),
        message: msg.into(),
    }
}

fn create_instance(pipe_name: &str, first: bool, sd: &SecDesc) -> std::io::Result<Owned> {
    let name = wide(pipe_name);
    let sa = sd.attributes();
    let mode = PIPE_ACCESS_DUPLEX
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    let h = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            &sa,
        )
    };
    if h == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Owned(h))
}

/// Closes a connection after its lifetime unless it finished first. The
/// handle is only touched while `done == false` under the mutex, so it is
/// never used after the serving thread closed it.
struct Watchdog {
    done: Mutex<bool>,
    cv: Condvar,
}

/// Accept loop with bounded concurrency. Each client is authenticated by the
/// image path of the connecting process (the installed `discord-bridge.exe`)
/// and identified by its token (impersonation) for authorization.
pub fn serve(helper: Arc<Helper>) -> std::io::Result<()> {
    serve_on(helper, PIPE_NAME, PIPE_SDDL)
}

/// Accept loop on an explicit pipe name / security descriptor (tests use a
/// private pipe; the service uses `PIPE_NAME` and `PIPE_SDDL`).
pub fn serve_on(helper: Arc<Helper>, pipe_name: &str, sddl: &str) -> std::io::Result<()> {
    let sd = SecDesc::from_sddl(sddl)?;
    let mut first = true;
    loop {
        let pipe = create_instance(pipe_name, first, &sd)?;
        first = false;
        let ok = unsafe { ConnectNamedPipe(pipe.0, std::ptr::null_mut()) } != 0
            || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
        if !ok {
            continue;
        }
        let raw = pipe.0;
        std::mem::forget(pipe);
        // SAFETY: we own the handle (forgotten above) and File closes it.
        let mut file = unsafe { File::from_raw_handle(raw as _) };
        let Some(slot) = ConnectionLimiter::try_acquire_owned(&helper.limiter) else {
            let _ = ipc::write_frame(&mut file, &err("busy", "служба занята, повторите запрос"));
            unsafe { DisconnectNamedPipe(raw) };
            continue;
        };
        let wd = Arc::new(Watchdog {
            done: Mutex::new(false),
            cv: Condvar::new(),
        });
        {
            let wd = wd.clone();
            let raw_addr = raw as usize;
            std::thread::spawn(move || {
                let g = lock(&wd.done);
                let (g, timeout) = wd
                    .cv
                    .wait_timeout_while(g, CONNECTION_LIFETIME, |d| !*d)
                    .unwrap_or_else(|p| p.into_inner());
                if timeout.timed_out() && !*g {
                    warn!("pipe client exceeded {CONNECTION_LIFETIME:?}, disconnecting");
                    unsafe { DisconnectNamedPipe(raw_addr as HANDLE) };
                }
            });
        }
        let helper2 = helper.clone();
        let raw_addr = raw as usize;
        std::thread::spawn(move || {
            let raw = raw_addr as HANDLE;
            serve_client(&helper2, &mut file, raw);
            {
                let mut d = lock(&wd.done);
                *d = true;
                wd.cv.notify_all();
                unsafe {
                    FlushFileBuffers(raw);
                    DisconnectNamedPipe(raw);
                }
            }
            drop(file);
            drop(slot);
        });
    }
}

/// Connection protocol:
/// 1. Read exactly one bounded frame. It must be `Hello`; nothing is executed
///    for it yet. Reading first is required by the Windows contract:
///    `ImpersonateNamedPipeClient` fails with `ERROR_CANNOT_IMPERSONATE`
///    until the server has read data from the pipe.
/// 2. Authenticate: the client process image must be the installed
///    `discord-bridge.exe`, and its identity is taken from its token by
///    impersonation (never by acting as SYSTEM on the client's behalf).
/// 3. Only then answer the Hello and serve further requests, each authorized
///    by the ownership policy.
fn serve_client(helper: &Helper, file: &mut File, raw: HANDLE) {
    let first: Request = match ipc::read_frame(file) {
        Ok(r) => r,
        Err(ipc::FrameError::Io(_)) => return,
        Err(e) => {
            let _ = ipc::write_frame(file, &err("bad_request", &e.to_string()));
            return;
        }
    };
    if !matches!(first, Request::Hello { .. }) {
        let _ = ipc::write_frame(
            file,
            &err("bad_request", "первым сообщением должен быть Hello"),
        );
        return;
    }
    let mut pid = 0u32;
    let image_ok = unsafe { GetNamedPipeClientProcessId(raw, &mut pid) } != 0
        && process_image(pid).is_some_and(|img| same_path(&img, &helper.paths.client_exe()));
    if !image_ok {
        warn!("rejected pipe client pid {pid} ({:?})", process_image(pid));
        let _ = ipc::write_frame(
            file,
            &err("unauthorized", "клиент не является Discord Bridge"),
        );
        return;
    }
    let client = match client_identity(raw) {
        Ok(c) => c,
        Err(e) => {
            warn!("cannot identify pipe client: {e}");
            let _ = ipc::write_frame(
                file,
                &err("unauthorized", "не удалось определить пользователя клиента"),
            );
            return;
        }
    };
    let ctx = Ctx { pipe: raw, client };
    let mut next = Some(first);
    loop {
        let req = match next.take() {
            Some(r) => r,
            None => match ipc::read_frame(file) {
                Ok(r) => r,
                Err(ipc::FrameError::Io(_)) => break,
                Err(e) => {
                    let _ = ipc::write_frame(file, &err("bad_request", &e.to_string()));
                    break;
                }
            },
        };
        let resp = helper.handle(req, &ctx);
        if ipc::write_frame(file, &resp).is_err() {
            break;
        }
    }
}

/// Real Windows integration test of the pipe protocol: a separate process
/// started with a **non-elevated (LUA) restricted token** connects to a live
/// server on a private pipe, performs Hello and Status; an unauthorized
/// client and a client not starting with Hello are rejected.
#[cfg(test)]
mod win_tests {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::Security::{
        CreateRestrictedToken, GetTokenInformation, TokenElevation, LUA_TOKEN,
        TOKEN_ADJUST_DEFAULT, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess, OpenProcessToken,
        WaitForSingleObject, PROCESS_INFORMATION, STARTUPINFOW,
    };

    const ROLE_ENV: &str = "DB_PIPE_TEST_ROLE";
    /// Test pipe ACL: like production but for Authenticated Users (CI runs
    /// in a service session without the INTERACTIVE group) and without an
    /// explicit SYSTEM owner (the test process is not SYSTEM).
    const TEST_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;AU)";

    fn self_elevated() -> bool {
        unsafe {
            let mut tok = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
                return true;
            }
            let mut e: TOKEN_ELEVATION = std::mem::zeroed();
            let mut len = 0u32;
            let ok = GetTokenInformation(
                tok,
                TokenElevation,
                &mut e as *mut _ as _,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            ) != 0;
            CloseHandle(tok);
            !ok || e.TokenIsElevated != 0
        }
    }

    fn call(f: &mut File, req: &Request) -> Result<Response, String> {
        ipc::write_frame(f, req).map_err(|e| e.to_string())?;
        ipc::read_frame(f).map_err(|e| e.to_string())
    }

    /// Child-process entry point (no-op when run as a normal test).
    #[test]
    fn pipe_client_role() {
        let Ok(spec) = std::env::var(ROLE_ENV) else {
            return;
        };
        let (pipe, role) = spec.split_once('|').expect("pipe|role");
        let code = (|| -> i32 {
            if self_elevated() {
                return 4; // the test must run the client non-elevated
            }
            let mut f = match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(pipe)
            {
                Ok(f) => f,
                Err(_) => return 5,
            };
            match role {
                "allowed" => {
                    match call(
                        &mut f,
                        &Request::Hello {
                            protocol: PROTOCOL_VERSION,
                        },
                    ) {
                        Ok(Response::Hello { .. }) => {}
                        _ => return 6,
                    }
                    match call(&mut f, &Request::Status) {
                        Ok(Response::Status { status })
                            if status.can_control && status.state == EngineState::Stopped =>
                        {
                            0
                        }
                        _ => 7,
                    }
                }
                "unauthorized" => match call(
                    &mut f,
                    &Request::Hello {
                        protocol: PROTOCOL_VERSION,
                    },
                ) {
                    Ok(Response::Error { code, .. }) if code == "unauthorized" => 0,
                    _ => 8,
                },
                "nohello" => match call(&mut f, &Request::Status) {
                    Ok(Response::Error { code, .. }) if code == "bad_request" => 0,
                    _ => 9,
                },
                _ => 10,
            }
        })();
        std::process::exit(code);
    }

    /// Run the test binary itself as a non-elevated child performing `role`.
    fn run_child(pipe: &str, role: &str) -> u32 {
        std::env::set_var(ROLE_ENV, format!("{pipe}|{role}"));
        let exe = std::env::current_exe().unwrap();
        let cmd = format!(
            "\"{}\" server::win_tests::pipe_client_role --exact --nocapture --test-threads=1",
            exe.display()
        );
        let mut cmdw: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let mut tok = null_mut();
            assert!(
                OpenProcessToken(
                    GetCurrentProcess(),
                    TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
                    &mut tok
                ) != 0
            );
            let mut lua = null_mut();
            assert!(
                CreateRestrictedToken(tok, LUA_TOKEN, 0, null(), 0, null(), 0, null(), &mut lua)
                    != 0,
                "CreateRestrictedToken: {}",
                std::io::Error::last_os_error()
            );
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessAsUserW(
                lua,
                null(),
                cmdw.as_mut_ptr(),
                null(),
                null(),
                0,
                0,
                null(),
                null(),
                &si,
                &mut pi,
            );
            assert!(
                ok != 0,
                "CreateProcessAsUserW: {}",
                std::io::Error::last_os_error()
            );
            assert_eq!(
                WaitForSingleObject(pi.hProcess, 60_000),
                WAIT_OBJECT_0,
                "child timed out"
            );
            let mut code = 0u32;
            GetExitCodeProcess(pi.hProcess, &mut code);
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            CloseHandle(lua);
            CloseHandle(tok);
            code
        }
    }

    fn start_server(tag: &str, client_exe: std::path::PathBuf) -> (tempfile::TempDir, String) {
        let t = tempfile::tempdir().unwrap();
        let paths = Paths {
            install_dir: t.path().to_path_buf(),
            data_dir: t.path().join("data"),
            client_exe_override: Some(client_exe),
        };
        let helper = Arc::new(Helper::new(paths));
        let pipe = format!(r"\\.\pipe\DiscordBridgeTest-{}-{tag}", std::process::id());
        let p2 = pipe.clone();
        std::thread::spawn(move || {
            let _ = serve_on(helper, &p2, TEST_SDDL);
        });
        // wait until the pipe exists
        for _ in 0..100 {
            if std::path::Path::new(&pipe).exists() || std::fs::metadata(&pipe).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(200));
        (t, pipe)
    }

    #[test]
    fn non_elevated_client_hello_status_and_rejections() {
        if std::env::var(ROLE_ENV).is_ok() {
            return; // running as the child
        }
        let me = std::env::current_exe().unwrap();
        let (_a, allowed_pipe) = start_server("allowed", me.clone());
        assert_eq!(
            run_child(&allowed_pipe, "allowed"),
            0,
            "authorized non-elevated client: Hello + Status"
        );
        assert_eq!(
            run_child(&allowed_pipe, "nohello"),
            0,
            "first frame must be Hello"
        );

        let (_b, strict_pipe) = start_server(
            "strict",
            std::path::PathBuf::from(r"C:\nonexistent\discord-bridge.exe"),
        );
        assert_eq!(
            run_child(&strict_pipe, "unauthorized"),
            0,
            "foreign executable is rejected"
        );
        std::env::remove_var(ROLE_ENV);
    }
}
