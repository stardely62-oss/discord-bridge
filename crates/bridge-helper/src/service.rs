//! Windows service plumbing: install/uninstall and the service main loop.

use crate::paths::Paths;
use crate::server::{serve, Helper};
use crate::stopcheck::{our_winws, wait_for, StopOutcome};
use crate::win::{list_processes, process_image};
use crate::{hlog, info, warn};
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;
use windows_service::service::{
    ServiceAccess, ServiceAction, ServiceActionType, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceFailureActions, ServiceFailureResetPeriod,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_service::{define_windows_service, service_dispatcher};

pub const SERVICE_NAME: &str = "DiscordBridgeHelper";
const DISPLAY_NAME: &str = "Discord Bridge Helper";
const DESCRIPTION: &str =
    "Запускает проверенный движок Flowseal Zapret (winws) по запросу Bridge. Выполняет только фиксированный набор операций.";

type R = Result<(), Box<dyn std::error::Error>>;

pub fn install() -> R {
    let mgr = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )?;
    let exe = std::env::current_exe()?;
    let info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: exe,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None, // LocalSystem
        account_password: None,
    };
    // Upgrade in place: an existing service is reconfigured, never deleted,
    // so a failed upgrade cannot leave the machine without the service.
    let access = ServiceAccess::CHANGE_CONFIG
        | ServiceAccess::START
        | ServiceAccess::STOP
        | ServiceAccess::QUERY_STATUS;
    let svc = match mgr.open_service(SERVICE_NAME, access) {
        Ok(existing) => {
            if existing
                .query_status()
                .map(|s| s.current_state != ServiceState::Stopped)
                .unwrap_or(false)
            {
                existing.stop()?;
                wait_state(&existing, ServiceState::Stopped, SERVICE_STOP_TIMEOUT)?;
            }
            existing.change_config(&info)?;
            existing
        }
        Err(_) => mgr.create_service(&info, access)?,
    };
    svc.set_description(DESCRIPTION)?;
    svc.update_failure_actions(ServiceFailureActions {
        reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(3600)),
        reboot_msg: None,
        command: None,
        actions: Some(vec![
            ServiceAction {
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(5),
            },
            ServiceAction {
                action_type: ServiceActionType::Restart,
                delay: Duration::from_secs(30),
            },
            ServiceAction {
                action_type: ServiceActionType::None,
                delay: Duration::default(),
            },
        ]),
    })?;
    svc.start::<&str>(&[])?;
    wait_state(&svc, ServiceState::Running, Duration::from_secs(20))?;
    println!("service {SERVICE_NAME} installed and started");
    Ok(())
}

/// Waits for `want`; `Err` with the last seen state on timeout or query failure.
fn wait_state(
    svc: &windows_service::service::Service,
    want: ServiceState,
    timeout: Duration,
) -> Result<(), String> {
    let mut last = None;
    let mut err = None;
    let ok = wait_for(timeout, Duration::from_millis(200), || {
        match svc.query_status() {
            Ok(s) => {
                last = Some(s.current_state);
                s.current_state == want
            }
            Err(e) => {
                err = Some(e.to_string());
                false
            }
        }
    });
    if ok {
        Ok(())
    } else if let Some(e) = err {
        Err(format!("состояние службы недоступно: {e}"))
    } else {
        Err(format!(
            "за {} с служба не перешла в {want:?} (сейчас {last:?})",
            timeout.as_secs()
        ))
    }
}

const SERVICE_STOP_TIMEOUT: Duration = Duration::from_secs(30);
const ENGINE_EXIT_TIMEOUT: Duration = Duration::from_secs(15);
const ERROR_SERVICE_NOT_ACTIVE: i32 = 1062;

fn our_engine_pids(paths: &Paths) -> Vec<u32> {
    let procs: Vec<(u32, Option<std::path::PathBuf>)> = list_processes()
        .into_iter()
        .filter(|p| p.name.eq_ignore_ascii_case("winws.exe"))
        .map(|p| (p.pid, process_image(p.pid)))
        .collect();
    our_winws(&procs, &paths.engine_roots())
}

/// Stops the service (when running) and verifies that it reached STOPPED and
/// that no winws of Bridge remains. Foreign winws are ignored.
pub fn stop_and_verify(
    svc: Option<&windows_service::service::Service>,
    paths: &Paths,
) -> StopOutcome {
    if let Some(svc) = svc {
        let running = svc
            .query_status()
            .map(|s| s.current_state != ServiceState::Stopped)
            .unwrap_or(true);
        if running {
            if let Err(e) = svc.stop() {
                let not_active = matches!(&e, windows_service::Error::Winapi(io) if io.raw_os_error() == Some(ERROR_SERVICE_NOT_ACTIVE));
                if !not_active {
                    return StopOutcome::ServiceNotStopped(format!(
                        "запрос остановки отклонён: {e}"
                    ));
                }
            }
            if let Err(e) = wait_state(svc, ServiceState::Stopped, SERVICE_STOP_TIMEOUT) {
                return StopOutcome::ServiceNotStopped(e);
            }
        }
    }
    // The job object kills our winws when the service process exits, but the
    // termination is asynchronous: wait until the processes are really gone.
    let mut left = Vec::new();
    let gone = wait_for(ENGINE_EXIT_TIMEOUT, Duration::from_millis(250), || {
        left = our_engine_pids(paths);
        left.is_empty()
    });
    if !gone {
        return StopOutcome::EngineStillRunning(left);
    }
    if svc.is_some() {
        StopOutcome::Stopped
    } else {
        StopOutcome::NotInstalled
    }
}

/// `--stop-engine`: used by the installer before replacing files.
pub fn stop_service() -> StopOutcome {
    let paths = Paths::detect();
    let mgr = match ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT) {
        Ok(m) => m,
        Err(e) => {
            return StopOutcome::ServiceNotStopped(format!("диспетчер служб недоступен: {e}"))
        }
    };
    let svc = mgr
        .open_service(
            SERVICE_NAME,
            ServiceAccess::STOP | ServiceAccess::QUERY_STATUS,
        )
        .ok();
    stop_and_verify(svc.as_ref(), &paths)
}

/// `--uninstall-service`: verified stop, then delete. Foreign processes and
/// the shared WinDivert driver are left alone.
pub fn uninstall() -> Result<StopOutcome, Box<dyn std::error::Error>> {
    let paths = Paths::detect();
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    match mgr.open_service(
        SERVICE_NAME,
        ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
    ) {
        Ok(svc) => {
            let out = stop_and_verify(Some(&svc), &paths);
            if out.exit_code() != 0 {
                return Ok(out);
            }
            svc.delete()?;
            println!("service {SERVICE_NAME} removed");
            Ok(out)
        }
        Err(_) => {
            println!("service {SERVICE_NAME} not installed");
            Ok(stop_and_verify(None, &paths))
        }
    }
}

/// Uninstaller query. Reads only the state file — never constructs the
/// engine controller, so the running engine and its runtime directories are
/// untouched.
pub fn tcp_changed_by_us() -> bool {
    crate::hstate::tcp_changed_by_us(
        &Paths::detect().state_file(),
        crate::system::tcp_timestamps(),
    )
}

pub fn restore_tcp() -> R {
    let state = Paths::detect().state_file();
    crate::hstate::tcp_restore(
        &state,
        crate::system::tcp_timestamps(),
        crate::system::set_tcp_timestamps,
    )
    .map(|msg| println!("{msg}"))
    .map_err(|e| e.into())
}

define_windows_service!(ffi_service_main, service_main);

pub fn run_dispatcher() -> R {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

/// Removes `<install>\flowseal.old-*` left by the installer; returns how many.
fn remove_moved_aside(install_dir: &std::path::Path) -> usize {
    let Ok(rd) = std::fs::read_dir(install_dir) else {
        return 0;
    };
    rd.flatten()
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("flowseal.old-") && e.path().is_dir()
        })
        .filter(|e| std::fs::remove_dir_all(e.path()).is_ok())
        .count()
}

fn service_main(_args: Vec<OsString>) {
    if let Err(e) = run_service() {
        warn!("service failed: {e}");
    }
}

fn run_service() -> R {
    let paths = Paths::detect();
    let _ = std::fs::create_dir_all(&paths.data_dir);
    if let Err(e) = crate::win::ensure_protected_dir(&paths.data_dir) {
        // Refuse to run with a writable-by-users state directory.
        return Err(format!("cannot protect {}: {e}", paths.data_dir.display()).into());
    }
    hlog::init(paths.log_file());
    info!(
        "helper {} starting, install dir {}",
        env!("CARGO_PKG_VERSION"),
        paths.install_dir.display()
    );
    // Nothing runs yet: drop old engine copies and the install-time
    // `flowseal.old-*` directories the installer moved aside (best effort:
    // anything still held by a loaded driver stays until a later start).
    let removed = crate::stage::gc_engines(&paths.engines_dir(), &[]);
    let old_dirs = remove_moved_aside(&paths.install_dir);
    if removed + old_dirs > 0 {
        info!("cleaned {removed} old engine copies, {old_dirs} old install directories");
    }
    let helper = Arc::new(Helper::new(paths));

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let handle = service_control_handler::register(SERVICE_NAME, move |ev| match ev {
        ServiceControl::Stop | ServiceControl::Shutdown | ServiceControl::Preshutdown => {
            let _ = tx.send(());
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    let status = |state: ServiceState, accept: ServiceControlAccept, code: u32| ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(code),
        checkpoint: 0,
        wait_hint: Duration::from_secs(10),
        process_id: None,
    };
    handle.set_service_status(status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        0,
    ))?;

    let server_helper = helper.clone();
    let (err_tx, err_rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        if let Err(e) = serve(server_helper) {
            let _ = err_tx.send(e.to_string());
        }
    });
    // Wait for a stop request or a fatal server error.
    let mut exit_code = 0;
    loop {
        if rx.recv_timeout(Duration::from_millis(500)).is_ok() {
            info!("service stop requested");
            break;
        }
        if let Ok(e) = err_rx.try_recv() {
            warn!("pipe server failed: {e}");
            exit_code = 1;
            break;
        }
    }
    handle.set_service_status(status(
        ServiceState::StopPending,
        ServiceControlAccept::empty(),
        0,
    ))?;
    // Terminates the job object → our winws (and only it) exits.
    helper.ctl.lock().unwrap_or_else(|p| p.into_inner()).stop();
    handle.set_service_status(status(
        ServiceState::Stopped,
        ServiceControlAccept::empty(),
        exit_code,
    ))?;
    Ok(())
}
