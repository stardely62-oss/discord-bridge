//! Read-only inspection of the system (conflicts, services, TCP settings) and
//! the single consented system change (TCP timestamps).

use crate::win::{list_processes, system_dir};
use bridge_core::ipc::Conflict;
use std::os::windows::process::CommandExt;
use std::process::Command;
use windows_service::service::{ServiceAccess, ServiceState};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

/// Executables of other DPI-bypass tools that use WinDivert.
const FOREIGN_ENGINES: &[&str] = &["winws.exe", "winws2.exe", "goodbyedpi.exe"];
/// Services Flowseal itself treats as conflicting, plus its own service.
const FOREIGN_SERVICES: &[&str] = &[
    "zapret",
    "GoodbyeDPI",
    "winws1",
    "winws2",
    "discordfix_zapret",
];

pub fn service_state(name: &str) -> Option<ServiceState> {
    let mgr = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT).ok()?;
    let svc = mgr.open_service(name, ServiceAccess::QUERY_STATUS).ok()?;
    svc.query_status().ok().map(|s| s.current_state)
}

pub fn state_name(s: ServiceState) -> &'static str {
    match s {
        ServiceState::Running => "running",
        ServiceState::Stopped => "stopped",
        ServiceState::StartPending => "start_pending",
        ServiceState::StopPending => "stop_pending",
        ServiceState::Paused => "paused",
        ServiceState::PausePending => "pause_pending",
        ServiceState::ContinuePending => "continue_pending",
    }
}

/// Foreign engines/services. `is_ours` identifies our own engine by PID and
/// job membership; nothing is ever stopped here.
pub fn find_conflicts(is_ours: impl Fn(u32) -> bool) -> Vec<Conflict> {
    let mut out = Vec::new();
    for p in list_processes() {
        let lower = p.name.to_ascii_lowercase();
        if FOREIGN_ENGINES.contains(&lower.as_str()) && !is_ours(p.pid) {
            out.push(Conflict {
                kind: "process".into(),
                name: p.name.clone(),
                pid: Some(p.pid),
                detail: "запущен сторонний экземпляр движка обхода DPI; одновременная работа двух движков приводит к двойной обработке пакетов".into(),
            });
        }
    }
    for s in FOREIGN_SERVICES {
        if let Some(st) = service_state(s) {
            if matches!(st, ServiceState::Running | ServiceState::StartPending) {
                out.push(Conflict {
                    kind: "service".into(),
                    name: s.to_string(),
                    pid: None,
                    detail: format!("служба {s} запущена (состояние: {})", state_name(st)),
                });
            }
        }
    }
    out
}

fn decode_oem(bytes: &[u8]) -> String {
    // netsh writes in the OEM code page. Handle CP866 (Russian) explicitly,
    // everything else as lossy UTF-8/ASCII.
    let oem = unsafe { windows_sys::Win32::Globalization::GetOEMCP() };
    if oem != 866 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    bytes
        .iter()
        .map(|&b| match b {
            0x00..=0x7f => b as char,
            0x80..=0xaf => char::from_u32(0x0410 + (b - 0x80) as u32).unwrap_or('?'),
            0xe0..=0xef => char::from_u32(0x0440 + (b - 0xe0) as u32).unwrap_or('?'),
            0xf0 => 'Ё',
            0xf1 => 'ё',
            _ => '?',
        })
        .collect()
}

/// Parses `netsh interface tcp show global`. Returns None if undetermined.
pub fn parse_timestamps(out: &str) -> Option<bool> {
    for line in out.lines() {
        let l = line.to_lowercase();
        if l.contains("timestamps") || l.contains("1323") {
            let v = l.rsplit(':').next().unwrap_or("").trim().to_string();
            if v.starts_with("enabled") || v.starts_with("включ") {
                return Some(true);
            }
            if v.starts_with("disabled") || v.starts_with("отключ") || v.starts_with("выключ")
            {
                return Some(false);
            }
        }
    }
    None
}

fn netsh(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new(system_dir().join("netsh.exe"))
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
}

pub fn tcp_timestamps() -> Option<bool> {
    let out = netsh(&["interface", "tcp", "show", "global"]).ok()?;
    parse_timestamps(&decode_oem(&out.stdout))
}

pub fn set_tcp_timestamps(enable: bool) -> Result<(), String> {
    let v = if enable {
        "timestamps=enabled"
    } else {
        "timestamps=disabled"
    };
    let out = netsh(&["interface", "tcp", "set", "global", v]).map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(decode_oem(&out.stdout).trim().to_string())
    }
}
