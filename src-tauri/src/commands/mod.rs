//! Tauri commands: the complete API used by the UI. Every command maps to a
//! real backend operation. One module per area; shared imports live here and
//! reach the modules through `use super::*`.

use crate::configuration::{LaunchMode, Settings, VoiceConfirmation};
use crate::diagnostics::{self, CheckItem, CheckStatus, DiagnosticsReport, VoiceState};
use crate::discord::{self, DiscordStatus};
use crate::state::{AppState, BundleState};
use crate::strategies::{ImportPreview, StrategyDetails, StrategyListItem, StrategyManager};
use crate::zapret::{self, ActivationResult, AutoProgress, FlowsealAction};
use bridge_core::diff::StrategyDiff;
use bridge_core::ipc::{EngineState, EngineStatus, Request, Response};
use bridge_core::strategy::options::{option_specs, OptionSpec};
use bridge_core::strategy::Strategy;
use bridge_core::updates::{self as upd, UpdateInfo};
use bridge_updates::UpdatesView;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

type R<T> = Result<T, String>;

pub mod app;
pub mod bypass;
pub mod checks;
pub mod launch;
pub mod library;
pub mod settings;
pub mod updates;

pub use app::*;
pub use launch::*;
pub use updates::*;
