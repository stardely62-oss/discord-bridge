//! Protocol between the unprivileged UI and the privileged helper service.
//!
//! Transport: a local named pipe (`PIPE_NAME`) that rejects remote clients.
//! Framing: 4-byte little-endian length followed by a JSON document. Only the
//! fixed set of commands below exists; there is no "run this" command. The
//! helper authenticates the client by the image path of the connecting
//! process and re-validates every strategy before use.

use crate::lists::{IpsetMode, UserLists};
use crate::manifest::SignedManifest;
use crate::ports::GameFilter;
use crate::strategy::validate::Issue;
use crate::strategy::Strategy;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const PIPE_NAME: &str = r"\\.\pipe\DiscordBridgeHelper.v1";
/// 3: IP sets (`UserLists::ipset_include`, new `IpsetMode` values).
pub const PROTOCOL_VERSION: u32 = 3;
/// Upper bound for one frame. Large payloads (update archives) never travel
/// through the pipe; they are passed as a file path and read by the helper
/// while impersonating the client.
pub const MAX_MESSAGE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOptions {
    #[serde(default)]
    pub game_filter: GameFilter,
    #[serde(default)]
    pub ipset_mode: IpsetMode,
    #[serde(default)]
    pub user_lists: UserLists,
    /// Start even if foreign winws/GoodbyeDPI instances are detected.
    /// Only set after the user explicitly acknowledged the conflict.
    #[serde(default)]
    pub ignore_conflicts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "camelCase")]
pub enum Request {
    Hello {
        protocol: u32,
    },
    Status,
    Start {
        strategy: Box<Strategy>,
        runtime: RuntimeOptions,
    },
    Stop,
    Diagnose,
    /// Changes a global Windows TCP setting; requires explicit user consent.
    SetTcpTimestamps {
        enable: bool,
        user_confirmed: bool,
    },
    ListBundles,
    /// Install a Flowseal release: signed manifest + path to the official
    /// archive. The helper opens the file while impersonating the client, so
    /// it can only read what the user could read anyway.
    InstallBundle {
        signed: SignedManifest,
        archive_path: String,
    },
    /// Start a trial session (Auto mode): the current launch spec is kept as
    /// a baseline until `EndTrial`.
    BeginTrial,
    /// End the trial: `commit = false` restores the baseline exactly (or
    /// stops the engine if it was stopped before).
    EndTrial {
        commit: bool,
    },
    /// Restore TCP timestamps to the value recorded before Discord Bridge
    /// changed it (only if the current value is still the one we set).
    RestoreTcpTimestamps,
    /// Switch the active bundle (used for rollback).
    SetActiveBundle {
        version: String,
    },
    EngineLog,
    /// Read-only: one list file (`lists/<name>`) of an installed Flowseal
    /// version, checked against that version's manifest hash. Returned in
    /// chunks of at most [`LIST_CHUNK`] bytes starting at `offset`.
    ReadBundleList {
        version: String,
        name: String,
        #[serde(default)]
        offset: u64,
    },
}

/// Largest chunk of a list file in one response (well below [`MAX_MESSAGE`]).
pub const LIST_CHUNK: usize = 384 * 1024;
/// Largest list file the helper hands out.
pub const MAX_LIST_FILE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineState {
    Stopped,
    Running,
    /// Process exited unexpectedly or failed to start.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    /// `process` or `service`.
    pub kind: String,
    pub name: String,
    pub pid: Option<u32>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub state: EngineState,
    pub pid: Option<u32>,
    pub strategy_id: Option<String>,
    pub strategy_name: Option<String>,
    /// Unix seconds.
    pub started_at: Option<u64>,
    pub exit_code: Option<i32>,
    pub last_error: Option<String>,
    pub bundle_version: Option<String>,
    pub args_preview: Vec<String>,
    /// Hash of the effective launch configuration (see `strategy::config_hash`).
    #[serde(default)]
    pub config_hash: Option<String>,
    /// An Auto/trial session is active; `EndTrial` restores or commits.
    #[serde(default)]
    pub trial_active: bool,
    /// Windows account (SID) that currently controls the engine, if any.
    #[serde(default)]
    pub owner_sid: Option<String>,
    /// The requesting client is the owner (or no one owns the engine).
    #[serde(default)]
    pub can_control: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleInfo {
    pub version: String,
    pub winws_version: String,
    pub active: bool,
    pub builtin: bool,
    pub ok: bool,
    pub problems: Vec<String>,
    /// Resource names, so the UI can validate strategies against the exact
    /// files the helper will use.
    pub fakes: Vec<String>,
    pub lists: Vec<String>,
    /// Original strategy files (name, content) of this bundle.
    pub strategy_files: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelperDiagnostics {
    pub helper_version: String,
    pub bundle_version: Option<String>,
    pub integrity_ok: bool,
    pub integrity_problems: Vec<String>,
    /// None when it could not be determined.
    pub tcp_timestamps: Option<bool>,
    pub base_filtering_engine_running: Option<bool>,
    pub windivert_service: Option<String>,
    pub conflicts: Vec<Conflict>,
}

/// Result of a strategy switch performed by the helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SwitchResult {
    /// The new configuration is running.
    Started,
    /// Preparation failed; the previous engine was never touched.
    PrepareFailed,
    /// The new engine failed; the exact previous configuration runs again.
    RolledBack,
    /// The new engine failed and so did restoring the previous one.
    RollbackFailed,
    /// The new engine failed and nothing ran before.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchReport {
    pub result: SwitchResult,
    pub error: Option<String>,
    pub rollback_error: Option<String>,
    /// Name of the strategy running after the operation, if any.
    pub running_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Response {
    Hello {
        protocol: u32,
        helper_version: String,
    },
    Status {
        status: EngineStatus,
    },
    Switched {
        report: SwitchReport,
        status: EngineStatus,
    },
    TcpTimestamps {
        current: Option<bool>,
        changed_by_us: bool,
        message: String,
    },
    Diagnose {
        diagnostics: HelperDiagnostics,
    },
    Bundles {
        bundles: Vec<BundleInfo>,
    },
    EngineLog {
        lines: Vec<String>,
    },
    /// A chunk of a verified list file (see `Request::ReadBundleList`).
    BundleList {
        version: String,
        name: String,
        /// SHA-256 of the whole file, as in the bundle manifest.
        sha256: String,
        size: u64,
        offset: u64,
        /// Base64 of the chunk (the file is passed on byte for byte).
        data: String,
    },
    Ok,
    /// Strategy was rejected by the helper's own validation.
    Invalid {
        issues: Vec<Issue>,
    },
    Conflicts {
        conflicts: Vec<Conflict>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("ошибка канала: {0}")]
    Io(#[from] std::io::Error),
    #[error("сообщение слишком большое ({0} байт)")]
    TooLarge(usize),
    #[error("некорректное сообщение: {0}")]
    Json(#[from] serde_json::Error),
}

pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> Result<(), FrameError> {
    let body = serde_json::to_vec(msg)?;
    if body.len() > MAX_MESSAGE {
        return Err(FrameError::TooLarge(body.len()));
    }
    let mut buf = Vec::with_capacity(body.len() + 4);
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    w.write_all(&buf)?;
    w.flush()?;
    Ok(())
}

pub fn read_frame<R: Read, T: for<'de> Deserialize<'de>>(r: &mut R) -> Result<T, FrameError> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_MESSAGE {
        return Err(FrameError::TooLarge(len));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(serde_json::from_slice(&body)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        let mut buf = Vec::new();
        let req = Request::Start {
            strategy: Box::new(Strategy::new_empty("a", "A")),
            runtime: RuntimeOptions::default(),
        };
        write_frame(&mut buf, &req).unwrap();
        let back: Request = read_frame(&mut buf.as_slice()).unwrap();
        assert!(matches!(back, Request::Start { .. }));
    }

    #[test]
    fn rejects_oversized_and_unknown() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&((MAX_MESSAGE as u32) + 1).to_le_bytes());
        assert!(matches!(
            read_frame::<_, Request>(&mut buf.as_slice()),
            Err(FrameError::TooLarge(_))
        ));

        let body = br#"{"cmd":"runShell","command":"calc.exe"}"#;
        let mut buf = (body.len() as u32).to_le_bytes().to_vec();
        buf.extend_from_slice(body);
        assert!(matches!(
            read_frame::<_, Request>(&mut buf.as_slice()),
            Err(FrameError::Json(_))
        ));
    }
}
