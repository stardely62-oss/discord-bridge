//! FlowsealManager + ZapretProcessManager (UI side).
//!
//! The UI decides *what* to run; the helper runs it transactionally (prepare
//! → stop → start → rollback to the exact previous snapshot). This module
//! maps those results, runs Auto through helper trials, and performs
//! Flowseal updates as a transaction (install → activate → verify → revert).

use crate::diagnostics::{quick_probe, CheckItem};
use crate::state::{AppState, BundleState, BundleSummary};
use bridge_core::auto::{run_auto, AutoConfig, AutoEnd, AutoEnv, TryOutcome};
use bridge_core::catalog::resource_index;
use bridge_core::ipc::{
    Conflict, EngineState, EngineStatus, Request, Response, RuntimeOptions, SwitchResult,
};
use bridge_core::manifest::{self, BundleManifest, SignedManifest, TRUSTED_KEYS};
use bridge_core::strategy::validate::{validate, Issue};
use bridge_core::versions;
use serde::Serialize;
use std::cmp::Ordering as CmpOrdering;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

use bridge_core::updates::MANIFEST_BASE;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivationResult {
    pub ok: bool,
    pub requested_id: Option<String>,
    /// running | rolledBack | rollbackFailed | failed | prepareFailed |
    /// conflicts | invalid | helperUnavailable
    pub outcome: String,
    pub message: Option<String>,
    pub conflicts: Vec<Conflict>,
    pub issues: Vec<Issue>,
    pub https_ok: Option<bool>,
    pub https_items: Vec<CheckItem>,
    /// Name of the configuration that runs after a failed switch.
    pub rolled_back_to: Option<String>,
    pub rollback_error: Option<String>,
    pub status: Option<EngineStatus>,
}

pub fn runtime(state: &AppState, ignore_conflicts: bool) -> RuntimeOptions {
    let s = state.settings();
    RuntimeOptions {
        game_filter: s.game_filter,
        ipset_mode: s.ipset_mode,
        // The engine gets the user's entries plus installed catalog packs.
        user_lists: bridge_core::lists::effective_lists(&s.user_lists, &s.installed_packs),
        ignore_conflicts,
    }
}

/// Manual operations are refused while Auto runs (cancel Auto first).
pub fn ensure_not_auto(state: &AppState) -> Result<(), String> {
    if state.auto.running.load(Ordering::SeqCst) {
        Err("идёт автоподбор стратегии — дождитесь окончания или отмените его".into())
    } else {
        Ok(())
    }
}

/// Load the built-in catalog: preferably from the helper (exactly the files it
/// will run), otherwise read-only from the installed resources.
pub async fn load_bundle(state: &AppState) {
    let mut bs = BundleState::default();
    match state.helper.request(Request::ListBundles).await {
        Ok(Response::Bundles { bundles }) => {
            bs.source = "helper".into();
            bs.bundles = bundles.iter().map(BundleSummary::from).collect();
            if let Some(b) = bundles.iter().find(|b| b.active) {
                bs.version = Some(b.version.clone());
                bs.winws_version = Some(b.winws_version.clone());
                let idx = resource_index(
                    b.fakes.iter().map(String::as_str),
                    b.lists.iter().map(String::as_str),
                );
                state.strategies.lock().expect("lock").set_bundle(
                    &b.version,
                    &b.strategy_files,
                    idx,
                );
            }
            *state.bundle_resources.lock().expect("lock") = bundles
                .iter()
                .map(|b| {
                    (
                        b.version.clone(),
                        resource_index(
                            b.fakes.iter().map(String::as_str),
                            b.lists.iter().map(String::as_str),
                        ),
                    )
                })
                .collect();
        }
        other => {
            bs.error = Some(match other {
                Err(e) => e.to_string(),
                Ok(_) => "неожиданный ответ службы".into(),
            });
            bs.source = "resources".into();
            if let Some((m, files)) =
                read_bundle_from_resources(&state.resource_dir.join("flowseal"))
            {
                bs.version = Some(m.flowseal_version.clone());
                bs.winws_version = Some(m.winws_version.clone());
                bs.bundles = vec![BundleSummary {
                    version: m.flowseal_version.clone(),
                    winws_version: m.winws_version.clone(),
                    active: true,
                    builtin: true,
                    ok: true,
                    problems: vec!["целостность проверяет служба; сейчас она недоступна".into()],
                }];
                let bin: Vec<&str> = m
                    .files
                    .iter()
                    .filter_map(|f| f.path.strip_prefix("bin/"))
                    .collect();
                let lists: Vec<&str> = m
                    .files
                    .iter()
                    .filter_map(|f| f.path.strip_prefix("lists/"))
                    .collect();
                let idx = resource_index(bin, lists);
                state
                    .strategies
                    .lock()
                    .expect("lock")
                    .set_bundle(&m.flowseal_version, &files, idx);
            }
        }
    }
    *state.bundle.lock().expect("lock") = bs;
}

pub fn read_bundle_from_resources(root: &Path) -> Option<(BundleManifest, Vec<(String, String)>)> {
    let mut best: Option<(BundleManifest, std::path::PathBuf)> = None;
    for e in std::fs::read_dir(root).ok()?.flatten() {
        let Ok(text) = std::fs::read_to_string(e.path().join("bundle-manifest.json")) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<BundleManifest>(&text) else {
            continue;
        };
        let newer = best.as_ref().is_none_or(|(b, _)| {
            versions::flowseal_cmp(&m.flowseal_version, &b.flowseal_version)
                == Ok(CmpOrdering::Greater)
        });
        if newer {
            best = Some((m, e.path()));
        }
    }
    let (m, dir) = best?;
    let files = m
        .files
        .iter()
        .filter(|f| !f.path.contains('/') && bridge_core::catalog::is_strategy_file(&f.path))
        .filter_map(|f| {
            std::fs::read(dir.join(&f.path))
                .ok()
                .map(|d| (f.path.clone(), String::from_utf8_lossy(&d).into_owned()))
        })
        .collect();
    Some((m, files))
}

pub async fn engine_status(state: &AppState) -> Result<EngineStatus, String> {
    match state.helper.request(Request::Status).await {
        Ok(Response::Status { status }) => Ok(status),
        Ok(Response::Error { message, .. }) => Err(message),
        Ok(_) => Err("неожиданный ответ службы".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn outcome_name(r: SwitchResult) -> &'static str {
    match r {
        SwitchResult::Started => "running",
        SwitchResult::PrepareFailed => "prepareFailed",
        SwitchResult::RolledBack => "rolledBack",
        SwitchResult::RollbackFailed => "rollbackFailed",
        SwitchResult::Failed => "failed",
    }
}

/// Ask the helper to switch to strategy `id`. No locking, no probe.
async fn start_raw(state: &AppState, id: &str, ignore_conflicts: bool) -> ActivationResult {
    let mut res = ActivationResult {
        requested_id: Some(id.to_string()),
        ..Default::default()
    };
    let strategy = state.strategies.lock().expect("lock").get(id).cloned();
    let Some(strategy) = strategy else {
        res.outcome = "invalid".into();
        res.message = Some("стратегия не найдена".into());
        return res;
    };
    let report = {
        let sm = state.strategies.lock().expect("lock");
        validate(&strategy, Some(&sm.resources))
    };
    if report.has_errors() {
        res.outcome = "invalid".into();
        res.message = Some("в стратегии есть ошибки — исправьте их в редакторе".into());
        res.issues = report.issues;
        return res;
    }
    let req = Request::Start {
        strategy: Box::new(strategy),
        runtime: runtime(state, ignore_conflicts),
    };
    match state.helper.request(req).await {
        Ok(Response::Switched { report, status }) => {
            res.ok = report.result == SwitchResult::Started && status.state == EngineState::Running;
            res.outcome = outcome_name(report.result).into();
            res.message = report.error;
            res.rollback_error = report.rollback_error;
            if report.result == SwitchResult::RolledBack
                || report.result == SwitchResult::PrepareFailed
            {
                res.rolled_back_to = report.running_name;
            }
            res.status = Some(status);
        }
        Ok(Response::Conflicts { conflicts }) => {
            res.outcome = "conflicts".into();
            res.message = Some("обнаружены другие средства обхода DPI".into());
            res.conflicts = conflicts;
        }
        Ok(Response::Invalid { issues }) => {
            res.outcome = "invalid".into();
            res.message = Some("служба отклонила стратегию".into());
            res.issues = issues;
        }
        Ok(Response::Error { message, .. }) => {
            res.outcome = "failed".into();
            res.message = Some(message);
        }
        Ok(_) => {
            res.outcome = "failed".into();
            res.message = Some("неожиданный ответ службы".into());
        }
        Err(e) => {
            res.outcome = "helperUnavailable".into();
            res.message = Some(e.to_string());
        }
    }
    res
}

/// Keep settings in line with what actually runs after an operation.
fn sync_settings(state: &AppState, res: &ActivationResult) {
    let running = res
        .status
        .as_ref()
        .filter(|s| s.state == EngineState::Running)
        .and_then(|s| s.strategy_id.clone());
    let probe_ok = res.https_ok == Some(true);
    let r = state.update_settings(|s| {
        if let Some(id) = &running {
            s.active_strategy_id = Some(id.clone());
            s.bypass_enabled = true;
            if res.ok && probe_ok {
                s.last_working_strategy_id = Some(id.clone());
            }
        } else if res.status.is_some() {
            s.bypass_enabled = false;
        }
    });
    if let Err(e) = r {
        tracing::warn!("settings not saved: {e}");
    }
}

/// Activate a strategy through the helper's transactional switch. With
/// `probe`, the Discord connectivity criterion is checked afterwards; a
/// running process alone is never called "working".
pub async fn activate(
    app: &AppHandle,
    state: &AppState,
    id: &str,
    ignore_conflicts: bool,
    probe: bool,
) -> ActivationResult {
    let _guard = state.op_lock.lock().await;
    let res = activate_locked(state, id, ignore_conflicts, probe).await;
    let _ = app.emit("status-changed", ());
    res
}

async fn activate_locked(
    state: &AppState,
    id: &str,
    ignore_conflicts: bool,
    probe: bool,
) -> ActivationResult {
    tracing::info!("activating strategy {id}");
    let mut res = start_raw(state, id, ignore_conflicts).await;
    if res.ok && probe {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let p = quick_probe(&state.http).await;
        res.https_ok = Some(p.ok);
        res.https_items = p.items;
    }
    tracing::info!(
        "strategy {id}: outcome={} https_ok={:?}",
        res.outcome,
        res.https_ok
    );
    sync_settings(state, &res);
    res
}

pub async fn deactivate(app: &AppHandle, state: &AppState) -> Result<(), String> {
    let _guard = state.op_lock.lock().await;
    let r = match state.helper.request(Request::Stop).await {
        Ok(Response::Ok) => Ok(()),
        Ok(Response::Error { message, .. }) => Err(message),
        Ok(_) => Err("неожиданный ответ службы".into()),
        Err(e) => Err(e.to_string()),
    };
    if r.is_ok() {
        state.update_settings(|s| s.bypass_enabled = false)?;
        tracing::info!("bypass disabled");
    }
    let _ = app.emit("status-changed", ());
    r
}

/// An abandoned trial (app crashed during Auto) is rolled back at startup.
pub async fn recover_abandoned_trial(state: &AppState) {
    if let Ok(st) = engine_status(state).await {
        if st.trial_active {
            tracing::warn!("abandoned Auto trial found, restoring the baseline");
            if let Err(e) = state
                .helper
                .request(Request::EndTrial { commit: false })
                .await
            {
                tracing::warn!("restoring abandoned trial failed: {e}");
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoProgress {
    pub running: bool,
    pub attempt: u32,
    pub max_attempts: u32,
    pub current: Option<String>,
    pub log: Vec<String>,
    pub result: Option<String>,
    pub success_id: Option<String>,
    /// Whether the original state was restored after an unsuccessful run.
    pub restored: Option<bool>,
}

struct AppAuto<'a> {
    app: &'a AppHandle,
    state: &'a AppState,
    started: Instant,
    attempt: u32,
    max: u32,
    current: Option<String>,
}

impl AppAuto<'_> {
    fn emit(&self) {
        let log = self.state.auto.log.lock().expect("lock").clone();
        let _ = self.app.emit(
            "auto-progress",
            AutoProgress {
                running: true,
                attempt: self.attempt,
                max_attempts: self.max,
                current: self.current.clone(),
                log,
                result: None,
                success_id: None,
                restored: None,
            },
        );
    }
}

impl AutoEnv for AppAuto<'_> {
    async fn begin(&mut self) -> Result<(), String> {
        match self.state.helper.request(Request::BeginTrial).await {
            Ok(Response::Ok) => Ok(()),
            Ok(Response::Error { message, .. }) => Err(message),
            Ok(_) => Err("неожиданный ответ службы".into()),
            Err(e) => Err(e.to_string()),
        }
    }
    async fn current_works(&mut self) -> Option<String> {
        let st = engine_status(self.state).await.ok()?;
        if st.state != EngineState::Running {
            return None;
        }
        if quick_probe(&self.state.http).await.ok {
            st.strategy_id
        } else {
            None
        }
    }
    async fn try_strategy(&mut self, id: &str) -> TryOutcome {
        self.attempt += 1;
        self.current = Some(id.to_string());
        self.emit();
        let r = match tokio::time::timeout(
            Duration::from_secs(40),
            activate_locked(self.state, id, false, true),
        )
        .await
        {
            Err(_) => return TryOutcome::NotWorking("тайм-аут попытки (40 с)".into()),
            Ok(r) => r,
        };
        match r.outcome.as_str() {
            "running" if r.https_ok == Some(true) => TryOutcome::Working,
            "running" => TryOutcome::NotWorking("Discord API не ответил корректно".into()),
            "conflicts" | "helperUnavailable" => TryOutcome::Fatal(r.message.unwrap_or_default()),
            _ => TryOutcome::EngineFailed(r.message.unwrap_or_else(|| r.outcome.clone())),
        }
    }
    async fn end(&mut self, commit: bool) -> Result<String, String> {
        match self
            .state
            .helper
            .request(Request::EndTrial { commit })
            .await
        {
            Ok(Response::Switched { report, status }) => {
                let res = ActivationResult {
                    status: Some(status.clone()),
                    ..Default::default()
                };
                sync_settings(self.state, &res);
                match report.result {
                    SwitchResult::RollbackFailed => Err(report
                        .rollback_error
                        .unwrap_or_else(|| "не удалось восстановить исходную конфигурацию".into())),
                    _ => Ok(match status.state {
                        EngineState::Running => {
                            format!("работает «{}»", status.strategy_name.unwrap_or_default())
                        }
                        _ => "обход выключен, как было до подбора".into(),
                    }),
                }
            }
            Ok(Response::Error { message, .. }) => Err(message),
            Ok(_) => Err("неожиданный ответ службы".into()),
            Err(e) => Err(e.to_string()),
        }
    }
    fn cancelled(&self) -> bool {
        self.state.auto.cancel.load(Ordering::SeqCst) || self.state.quitting.load(Ordering::SeqCst)
    }
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
    fn log(&mut self, line: String) {
        tracing::info!("auto: {line}");
        self.state.auto.log.lock().expect("lock").push(line);
        self.emit();
    }
}

/// Limited Auto mode (see `bridge_core::auto` for the contract).
pub async fn auto_select(app: AppHandle, state: &AppState) -> AutoProgress {
    if state.auto.running.swap(true, Ordering::SeqCst) {
        return AutoProgress {
            running: true,
            attempt: 0,
            max_attempts: 0,
            current: None,
            log: vec!["Auto уже выполняется".into()],
            result: None,
            success_id: None,
            restored: None,
        };
    }
    let _guard = state.op_lock.lock().await;
    state.auto.cancel.store(false, Ordering::SeqCst);
    state.auto.log.lock().expect("lock").clear();
    let settings = state.settings();
    let candidates: Vec<String> = {
        let sm = state.strategies.lock().expect("lock");
        let all: Vec<String> = sm.catalog.strategies.iter().map(|s| s.id.clone()).collect();
        let mut c: Vec<String> = if settings.auto_candidates.is_empty() {
            // Strategies reported as working at the user's provider go first;
            // an explicit candidate list keeps the user's own order.
            match settings
                .provider
                .as_deref()
                .and_then(|p| bridge_core::hints::builtin().provider(p))
            {
                Some(p) => bridge_core::hints::order_candidates(all, &p.strategies),
                None => all,
            }
        } else {
            settings
                .auto_candidates
                .iter()
                .filter(|id| sm.get(id).is_some())
                .cloned()
                .collect()
        };
        if let Some(lw) = &settings.last_working_strategy_id {
            if sm.get(lw).is_some() {
                c.retain(|x| x != lw);
                c.insert(0, lw.clone());
            }
        }
        c
    };
    let cfg = AutoConfig {
        candidates,
        max_attempts: settings.auto_max_attempts as usize,
        total_timeout: Duration::from_secs(300),
    };
    let mut env = AppAuto {
        app: &app,
        state,
        started: Instant::now(),
        attempt: 0,
        max: settings.auto_max_attempts,
        current: None,
    };
    let report = run_auto(&mut env, &cfg).await;
    let result = match &report.end {
        AutoEnd::AlreadyWorking(id) => format!(
            "текущая стратегия {id} уже обеспечивает доступ к Discord — переключение не требуется"
        ),
        AutoEnd::Found(id) => {
            format!("выбрана стратегия {id}. Голосовые каналы проверьте вручную.")
        }
        AutoEnd::Cancelled => "подбор отменён".into(),
        AutoEnd::TimedOut => "превышено общее время подбора (5 минут)".into(),
        AutoEnd::Exhausted => "подходящая стратегия не найдена в пределах лимита попыток".into(),
        AutoEnd::Fatal(e) => format!("подбор остановлен: {e}"),
    };
    let result = match &report.restore {
        Some(Ok(desc)) => format!("{result}; исходное состояние восстановлено ({desc})"),
        Some(Err(e)) => {
            format!("{result}; ВНИМАНИЕ: исходное состояние восстановить не удалось: {e}")
        }
        None => result,
    };
    if let Some(id) = report.success_id() {
        let r = state.update_settings(|s| {
            s.active_strategy_id = Some(id.to_string());
            s.last_working_strategy_id = Some(id.to_string());
            s.bypass_enabled = true;
        });
        if let Err(e) = r {
            tracing::warn!("settings not saved: {e}");
        }
    }
    state.auto.running.store(false, Ordering::SeqCst);
    let log = state.auto.log.lock().expect("lock").clone();
    let p = AutoProgress {
        running: false,
        attempt: env.attempt,
        max_attempts: env.max,
        current: None,
        log,
        result: Some(result),
        success_id: report.success_id().map(str::to_string),
        restored: report.restore.as_ref().map(|r| r.is_ok()),
    };
    let _ = app.emit("auto-progress", p.clone());
    let _ = app.emit("status-changed", ());
    p
}

/// Download the signed manifest, the official archive, verify both, store the
/// archive in a temporary file and let the helper read it (impersonating us)
/// and verify again before installing. The active version does not change.
pub async fn install_flowseal(state: &AppState, version: &str) -> Result<(), String> {
    if version.is_empty()
        || version.len() > 32
        || !version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err("некорректная версия".into());
    }
    let signed: SignedManifest = state
        .http
        .get(bridge_core::updates::manifest_url(MANIFEST_BASE, version))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|_| "подписанный манифест для этой версии недоступен".to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let m = manifest::verify(&signed, TRUSTED_KEYS).map_err(|e| e.to_string())?;
    if m.flowseal_version != version {
        return Err("манифест относится к другой версии".into());
    }
    if !versions::app_satisfies(env!("CARGO_PKG_VERSION"), &m.min_app_version)? {
        return Err(format!(
            "эта версия Flowseal требует Bridge {} или новее",
            m.min_app_version
        ));
    }
    let archive = state
        .http
        .get(&m.source_url)
        .timeout(Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;
    if manifest::sha256_hex(&archive) != m.source_sha256 {
        return Err("хэш архива не совпадает с подписанным манифестом".into());
    }
    let tmp = std::env::temp_dir().join(format!(
        "discord-bridge-flowseal-{version}-{}.zip",
        std::process::id()
    ));
    std::fs::write(&tmp, &archive)
        .map_err(|e| format!("не удалось сохранить архив во временный файл: {e}"))?;
    let req = Request::InstallBundle {
        signed,
        archive_path: tmp.to_string_lossy().into_owned(),
    };
    let r = state.helper.request(req).await;
    if let Err(e) = std::fs::remove_file(&tmp) {
        tracing::warn!("temporary archive not removed: {e}");
    }
    match r.map_err(|e| e.to_string())? {
        Response::Ok => Ok(()),
        Response::Error { message, .. } => Err(message),
        _ => Err("неожиданный ответ службы".into()),
    }
}

pub async fn set_active_bundle(state: &AppState, version: &str) -> Result<(), String> {
    match state
        .helper
        .request(Request::SetActiveBundle {
            version: version.into(),
        })
        .await
        .map_err(|e| e.to_string())?
    {
        Response::Ok => Ok(()),
        Response::Error { message, .. } => Err(message),
        _ => Err("неожиданный ответ службы".into()),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatEntry {
    pub id: String,
    pub name: String,
    pub problems: Vec<String>,
}

/// Which user strategies would break with Flowseal `version` (missing
/// fakes/lists, unsupported options). Profiles are never modified.
pub fn compat_report(state: &AppState, version: &str) -> Result<Vec<CompatEntry>, String> {
    let idx = state
        .bundle_resources
        .lock()
        .expect("lock")
        .get(version)
        .cloned()
        .ok_or("эта версия Flowseal не установлена")?;
    let sm = state.strategies.lock().expect("lock");
    Ok(sm
        .custom
        .iter()
        .filter_map(|s| {
            let problems: Vec<String> = validate(s, Some(&idx))
                .errors()
                .map(|i| format!("{}: {}", i.path, i.message))
                .collect();
            (!problems.is_empty()).then(|| CompatEntry {
                id: s.id.clone(),
                name: s.name.clone(),
                problems,
            })
        })
        .collect())
}

/// Outcome of installing or switching Flowseal, for the Updates page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowsealAction {
    pub ok: bool,
    /// `activated` — the version is active (and the running strategy works on it);
    /// `rolledBack` — it did not start, the previous version is active again;
    /// `rollbackFailed` — it did not start and returning failed;
    /// `refused` — nothing was changed (Auto running, strategy missing…);
    /// `installFailed` — download/verification/installation failed, nothing changed.
    pub outcome: String,
    pub message: String,
    pub version: String,
}

impl FlowsealAction {
    fn new(ok: bool, outcome: &str, version: &str, message: impl Into<String>) -> Self {
        FlowsealAction {
            ok,
            outcome: outcome.into(),
            message: message.into(),
            version: version.into(),
        }
    }
}

/// Switch Flowseal to `version` as a transaction: activate, restart the running
/// strategy on it, and if that fails, return to the previous version and
/// configuration.
pub async fn switch_bundle(app: &AppHandle, state: &AppState, version: &str) -> FlowsealAction {
    let refused = |m: String| FlowsealAction::new(false, "refused", version, m);
    if let Err(e) = ensure_not_auto(state) {
        return refused(e);
    }
    let prev = state.bundle.lock().expect("lock").version.clone();
    let running = engine_status(state)
        .await
        .ok()
        .filter(|s| s.state == EngineState::Running)
        .and_then(|s| s.strategy_id);
    if let Err(e) = set_active_bundle(state, version).await {
        return refused(format!("Flowseal {version} не активирован: {e}"));
    }
    load_bundle(state).await;
    let Some(id) = running else {
        let _ = app.emit("strategies-changed", ());
        return FlowsealAction::new(
            true,
            "activated",
            version,
            format!("активирован Flowseal {version}"),
        );
    };
    if state.strategies.lock().expect("lock").get(&id).is_none() {
        if let Some(p) = &prev {
            if let Err(e) = set_active_bundle(state, p).await {
                load_bundle(state).await;
                return FlowsealAction::new(
                    false,
                    "rollbackFailed",
                    version,
                    format!("стратегии «{id}» нет в Flowseal {version}; вернуть версию {p} не удалось: {e}"),
                );
            }
            load_bundle(state).await;
        }
        return refused(format!(
            "стратегии «{id}» нет в Flowseal {version} — версия не переключена"
        ));
    }
    let r = activate(app, state, &id, false, true).await;
    if r.ok {
        let _ = app.emit("strategies-changed", ());
        return FlowsealAction::new(
            true,
            "activated",
            version,
            format!("Flowseal {version} активирован, стратегия «{id}» работает"),
        );
    }
    // New version failed: the helper already restored the previous engine
    // snapshot (old files, old lists). Point the active version back too.
    if let Some(p) = &prev {
        if let Err(e) = set_active_bundle(state, p).await {
            load_bundle(state).await;
            let _ = app.emit("strategies-changed", ());
            return FlowsealAction::new(
                false,
                "rollbackFailed",
                version,
                format!(
                    "Flowseal {version} не запустился ({}); вернуть версию {p} не удалось: {e}",
                    r.message.unwrap_or_default()
                ),
            );
        }
        load_bundle(state).await;
    }
    let _ = app.emit("strategies-changed", ());
    FlowsealAction::new(
        false,
        "rolledBack",
        version,
        format!(
            "Flowseal {version} не запустился: {}. Возвращена версия {}{}",
            r.message.unwrap_or_default(),
            prev.unwrap_or_default(),
            if r.outcome == "rolledBack" {
                ", прежняя конфигурация работает"
            } else {
                ""
            }
        ),
    )
}
