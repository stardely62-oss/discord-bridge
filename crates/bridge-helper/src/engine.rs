//! Lifecycle of the single winws process owned by the helper.
//!
//! * The process is created suspended, put into a Job Object with
//!   `KILL_ON_JOB_CLOSE`, then resumed: if the helper dies, Windows kills
//!   winws with it, so no orphaned engine can remain.
//! * The engine binary, its DLLs and the WinDivert driver are opened with a
//!   deny-write share mode and hash-verified through those handles before
//!   start; the handles are held for the lifetime of the process.
//! * Our process is identified by PID **and** job membership, never by name.

use crate::bundles::{lock_and_verify, LockedFiles};
use crate::hlog::now_unix;
use crate::txn::{Backend, LaunchSpec};
use crate::win::{resume_process, Owned};
use crate::{info, warn};
use bridge_core::ipc::{EngineState, EngineStatus};
use bridge_core::strategy::render::ResourceResolver;
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};

const LOG_LINES: usize = 400;
/// How long the engine must survive after start before it is reported running.
const STARTUP_GRACE: Duration = Duration::from_millis(1500);

pub struct Resolver {
    pub bin: PathBuf,
    pub lists: PathBuf,
}

impl ResourceResolver for Resolver {
    fn fake_path(&self, name: &str) -> Option<String> {
        let p = self.bin.join(name);
        p.is_file().then(|| p.to_string_lossy().into_owned())
    }
    fn list_path(&self, name: &str) -> Option<String> {
        let p = self.lists.join(name);
        p.is_file().then(|| p.to_string_lossy().into_owned())
    }
}

struct Running {
    child: Child,
    job: Owned,
    pid: u32,
    _locks: LockedFiles,
    strategy_id: String,
    strategy_name: String,
    started_at: u64,
    args: Vec<String>,
}

pub struct Engine {
    running: Option<Running>,
    last_error: Option<String>,
    last_exit: Option<i32>,
    failed: bool,
    bundle_version: Option<String>,
    log: Arc<Mutex<VecDeque<String>>>,
}

impl Default for Engine {
    fn default() -> Self {
        Engine {
            running: None,
            last_error: None,
            last_exit: None,
            failed: false,
            bundle_version: None,
            log: Arc::new(Mutex::new(VecDeque::new())),
        }
    }
}

fn push_log(log: &Arc<Mutex<VecDeque<String>>>, line: String) {
    if let Ok(mut l) = log.lock() {
        if l.len() >= LOG_LINES {
            l.pop_front();
        }
        l.push_back(line);
    }
}

fn new_job() -> std::io::Result<Owned> {
    unsafe {
        let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if h.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Owned(h);
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        if SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }
}

impl Engine {
    /// True if `pid` belongs to our job (i.e. is our engine).
    pub fn owns(&self, pid: u32) -> bool {
        let Some(r) = &self.running else { return false };
        if r.pid != pid {
            return false;
        }
        let mut res = 0;
        unsafe { IsProcessInJob(r.child.as_raw_handle() as _, r.job.0, &mut res) != 0 && res != 0 }
    }

    pub fn log_lines(&self) -> Vec<String> {
        self.log
            .lock()
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn status(&mut self) -> EngineStatus {
        self.reap();
        let r = self.running.as_ref();
        EngineStatus {
            state: if r.is_some() {
                EngineState::Running
            } else if self.failed {
                EngineState::Failed
            } else {
                EngineState::Stopped
            },
            pid: r.map(|r| r.pid),
            strategy_id: r.map(|r| r.strategy_id.clone()),
            strategy_name: r.map(|r| r.strategy_name.clone()),
            started_at: r.map(|r| r.started_at),
            exit_code: self.last_exit,
            last_error: self.last_error.clone(),
            bundle_version: self.bundle_version.clone(),
            args_preview: r.map(|r| r.args.clone()).unwrap_or_default(),
            config_hash: None,
            trial_active: false,
            owner_sid: None,
            can_control: false,
        }
    }

    /// Detect an engine that exited on its own.
    fn reap(&mut self) {
        let exited = match self.running.as_mut() {
            Some(r) => r.child.try_wait().ok().flatten(),
            None => None,
        };
        if let Some(code) = exited {
            let r = self.running.take().expect("checked");
            self.failed = true;
            self.last_exit = code.code();
            let tail = self
                .log_lines()
                .into_iter()
                .rev()
                .take(3)
                .collect::<Vec<_>>();
            self.last_error = Some(format!(
                "winws завершился с кодом {:?}. {}",
                code.code(),
                tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
            ));
            warn!(
                "engine pid {} exited unexpectedly: {:?}",
                r.pid,
                code.code()
            );
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut r) = self.running.take() {
            unsafe { TerminateJobObject(r.job.0, 0) };
            let _ = r.child.wait();
            info!("engine pid {} stopped", r.pid);
        }
        self.failed = false;
        self.last_error = None;
        self.last_exit = None;
    }

    /// Start the engine for a fully prepared launch spec. The caller (the
    /// transaction controller) has already stopped any previous engine.
    pub fn start(&mut self, spec: &LaunchSpec) -> Result<u32, String> {
        if self.running.is_some() {
            // Defensive: never run two engines.
            self.stop();
        }
        let bundle = &spec.bundle;
        let strategy = &spec.strategy;
        let args = spec.args.clone();
        self.bundle_version = Some(bundle.version().to_string());
        let locks = lock_and_verify(bundle)
            .map_err(|e| format!("проверка целостности не пройдена: {e}"))?;
        let exe = bundle.bin_dir().join("winws.exe");
        let job = new_job().map_err(|e| format!("не удалось создать Job Object: {e}"))?;
        let mut child = Command::new(&exe)
            .args(&args)
            .current_dir(bundle.bin_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
            .spawn()
            .map_err(|e| format!("не удалось запустить winws: {e}"))?;
        let pid = child.id();
        let assigned = unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle() as _) } != 0;
        if !assigned {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "не удалось поместить winws в Job Object: {}",
                std::io::Error::last_os_error()
            ));
        }
        if let Ok(mut l) = self.log.lock() {
            l.clear();
        }
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let log = self.log.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    push_log(&log, line);
                }
            });
        }
        if !resume_process(pid) {
            unsafe { TerminateJobObject(job.0, 1) };
            let _ = child.wait();
            return Err("не удалось возобновить процесс winws".into());
        }
        info!("engine pid {pid} started with strategy {}", strategy.id);
        self.running = Some(Running {
            child,
            job,
            pid,
            _locks: locks,
            strategy_id: strategy.id.clone(),
            strategy_name: strategy.name.clone(),
            started_at: now_unix(),
            args,
        });
        self.failed = false;
        self.last_error = None;
        self.last_exit = None;
        std::thread::sleep(STARTUP_GRACE);
        self.reap();
        if self.running.is_none() {
            return Err(self
                .last_error
                .clone()
                .unwrap_or_else(|| "winws завершился сразу после запуска".into()));
        }
        Ok(pid)
    }

    pub fn is_running(&mut self) -> bool {
        self.reap();
        self.running.is_some()
    }
}

impl Backend for Engine {
    fn start(&mut self, spec: &LaunchSpec) -> Result<u32, String> {
        Engine::start(self, spec)
    }
    fn stop(&mut self) {
        Engine::stop(self)
    }
    fn alive(&mut self) -> bool {
        self.is_running()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}
