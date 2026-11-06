//! Shared application state.

use crate::configuration::ConfigurationManager;
use crate::diagnostics::DiagnosticsReport;
use crate::platform::logging::LogStore;
use crate::privileges::HelperClient;
use crate::strategies::StrategyManager;
use bridge_core::atomic::write_atomic;
use bridge_core::ipc::BundleInfo;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BundleState {
    pub version: Option<String>,
    pub winws_version: Option<String>,
    /// "helper" (verified by the service) or "resources" (read-only fallback).
    pub source: String,
    pub bundles: Vec<BundleSummary>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleSummary {
    pub version: String,
    pub winws_version: String,
    pub active: bool,
    pub builtin: bool,
    pub ok: bool,
    pub problems: Vec<String>,
}

impl From<&BundleInfo> for BundleSummary {
    fn from(b: &BundleInfo) -> Self {
        BundleSummary {
            version: b.version.clone(),
            winws_version: b.winws_version.clone(),
            active: b.active,
            builtin: b.builtin,
            ok: b.ok,
            problems: b.problems.clone(),
        }
    }
}

#[derive(Default)]
pub struct AutoControl {
    pub running: AtomicBool,
    pub cancel: AtomicBool,
    pub log: Mutex<Vec<String>>,
}

pub struct AppState {
    pub config: Mutex<ConfigurationManager>,
    pub strategies: Mutex<StrategyManager>,
    pub helper: HelperClient,
    pub logs: LogStore,
    pub http: reqwest::Client,
    pub history: Mutex<Vec<DiagnosticsReport>>,
    pub history_path: PathBuf,
    pub bundle: Mutex<BundleState>,
    /// Resource names of every installed Flowseal version (compatibility checks).
    pub bundle_resources:
        Mutex<std::collections::HashMap<String, bridge_core::strategy::validate::ResourceIndex>>,
    pub auto: AutoControl,
    /// Serializes start/stop/switch operations.
    pub op_lock: tokio::sync::Mutex<()>,
    pub resource_dir: PathBuf,
    pub quitting: AtomicBool,
    /// Verified remote catalog of address packs (see `catalog_sync`).
    pub catalog: crate::catalog_sync::CatalogRuntime,
}

pub const HISTORY_LIMIT: usize = 20;

impl AppState {
    pub fn settings(&self) -> crate::configuration::Settings {
        self.config.lock().expect("config lock").settings.clone()
    }

    pub fn update_settings(
        &self,
        f: impl FnOnce(&mut crate::configuration::Settings),
    ) -> Result<(), String> {
        let mut c = self.config.lock().expect("config lock");
        let mut s = c.settings.clone();
        f(&mut s);
        c.update(s)
    }

    pub fn push_history(&self, r: DiagnosticsReport) {
        let mut h = self.history.lock().expect("history lock");
        h.push(r);
        let n = h.len();
        if n > HISTORY_LIMIT {
            h.drain(..n - HISTORY_LIMIT);
        }
        if let Ok(data) = serde_json::to_vec_pretty(&*h) {
            let _ = write_atomic(&self.history_path, &data);
        }
    }

    pub fn load_history(path: &PathBuf) -> Vec<DiagnosticsReport> {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
}
