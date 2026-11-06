//! Transactional engine switching (platform independent).
//!
//! A switch is: **prepare** the new configuration completely (validated
//! strategy, rendered arguments, lists materialised into a *fresh generation
//! directory*), and only then **stop** the old engine and **start** the new
//! one. If the new engine fails, the *exact* previous [`LaunchSpec`] — the
//! strategy as it was when it was started, its runtime options, its Flowseal
//! bundle and its own lists directory — is started again. Nothing is looked
//! up by strategy id, so editing a strategy cannot corrupt the rollback.
//!
//! Generation directories are never shared between two launches and are
//! removed only when no running engine (and no trial baseline) uses them.

use crate::bundles::Bundle;
use bridge_core::ipc::{RuntimeOptions, SwitchReport, SwitchResult};
use bridge_core::strategy::Strategy;
use std::path::{Path, PathBuf};

/// Everything needed to start (or restart) one engine instance. Some fields
/// are only part of the snapshot (kept so a rollback is complete and for
/// diagnostics), not read by the controller itself.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LaunchSpec {
    pub strategy: Strategy,
    pub runtime: RuntimeOptions,
    pub bundle: Bundle,
    pub generation: u64,
    pub gen_dir: PathBuf,
    pub lists_dir: PathBuf,
    pub args: Vec<String>,
    pub config_hash: String,
    /// SID of the Windows account that requested this launch.
    pub owner_sid: Option<String>,
}

/// The process side, implemented by the Windows engine and by test fakes.
pub trait Backend {
    /// Start the engine for `spec` and confirm it survived the startup
    /// grace period. Must not leave a process behind on error.
    fn start(&mut self, spec: &LaunchSpec) -> Result<u32, String>;
    /// Stop our engine (no-op if not running).
    fn stop(&mut self);
    /// Whether our engine process is alive.
    fn alive(&mut self) -> bool;
}

struct Trial {
    baseline: Option<LaunchSpec>,
}

pub struct Controller<B: Backend> {
    pub backend: B,
    root: PathBuf,
    current: Option<LaunchSpec>,
    next_gen: u64,
    trial: Option<Trial>,
}

fn report(
    result: SwitchResult,
    error: Option<String>,
    rollback_error: Option<String>,
    running: Option<&LaunchSpec>,
) -> SwitchReport {
    SwitchReport {
        result,
        error,
        rollback_error,
        running_name: running.map(|s| s.strategy.name.clone()),
    }
}

impl<B: Backend> Controller<B> {
    /// `root` is the protected runtime directory. Leftover generations from a
    /// previous service run are removed (no engine can be running at start:
    /// the job object killed it together with the old service process).
    pub fn new(backend: B, root: PathBuf) -> Self {
        let mut next_gen = 1;
        if let Ok(rd) = std::fs::read_dir(&root) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if let Some(n) = name
                    .strip_prefix("gen-")
                    .and_then(|n| n.parse::<u64>().ok())
                {
                    next_gen = next_gen.max(n + 1);
                }
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        Controller {
            backend,
            root,
            current: None,
            next_gen,
            trial: None,
        }
    }

    pub fn current(&self) -> Option<&LaunchSpec> {
        self.current.as_ref()
    }

    pub fn trial_active(&self) -> bool {
        self.trial.is_some()
    }

    fn in_use(&self, dir: &Path) -> bool {
        self.current.as_ref().is_some_and(|c| c.gen_dir == dir)
            || self
                .trial
                .as_ref()
                .and_then(|t| t.baseline.as_ref())
                .is_some_and(|b| b.gen_dir == dir)
    }

    fn discard(&self, spec: &LaunchSpec) {
        if !self.in_use(&spec.gen_dir) {
            let _ = std::fs::remove_dir_all(&spec.gen_dir);
        }
    }

    /// Build a new launch spec in a fresh generation directory. `materialize`
    /// writes the lists, `render` returns the argument vector for that lists
    /// directory. Nothing running is touched; on error the directory is removed.
    pub fn prepare(
        &mut self,
        strategy: Strategy,
        runtime: RuntimeOptions,
        bundle: Bundle,
        owner_sid: Option<String>,
        materialize: impl FnOnce(&Path) -> Result<(), String>,
        render: impl FnOnce(&Path) -> Result<Vec<String>, String>,
    ) -> Result<LaunchSpec, String> {
        let generation = self.next_gen;
        self.next_gen += 1;
        let gen_dir = self.root.join(format!("gen-{generation}"));
        let lists_dir = gen_dir.join("lists");
        let result = (|| {
            std::fs::create_dir_all(&lists_dir)
                .map_err(|e| format!("не удалось создать каталог списков: {e}"))?;
            materialize(&lists_dir)?;
            render(&lists_dir)
        })();
        match result {
            Ok(args) => {
                let config_hash =
                    bridge_core::strategy::config_hash(&strategy, &runtime, bundle.version());
                Ok(LaunchSpec {
                    strategy,
                    runtime,
                    bundle,
                    generation,
                    gen_dir,
                    lists_dir,
                    args,
                    config_hash,
                    owner_sid,
                })
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&gen_dir);
                Err(e)
            }
        }
    }

    /// Stop the old engine, start `new`, roll back to the previous spec on failure.
    pub fn switch(&mut self, new: LaunchSpec) -> SwitchReport {
        let old = self.current.take();
        if old.is_some() {
            self.backend.stop();
        }
        match self.backend.start(&new) {
            Ok(_) => {
                self.current = Some(new);
                if let Some(o) = &old {
                    self.discard(o);
                }
                report(SwitchResult::Started, None, None, self.current.as_ref())
            }
            Err(e) => {
                self.discard(&new);
                match old {
                    None => report(SwitchResult::Failed, Some(e), None, None),
                    Some(o) => match self.backend.start(&o) {
                        Ok(_) => {
                            self.current = Some(o);
                            report(
                                SwitchResult::RolledBack,
                                Some(e),
                                None,
                                self.current.as_ref(),
                            )
                        }
                        Err(e2) => {
                            self.discard(&o);
                            report(SwitchResult::RollbackFailed, Some(e), Some(e2), None)
                        }
                    },
                }
            }
        }
    }

    pub fn stop(&mut self) {
        self.backend.stop();
        if let Some(c) = self.current.take() {
            self.discard(&c);
        }
    }

    /// The engine died on its own: forget the spec (the caller reports it).
    pub fn reap_if_dead(&mut self) -> Option<LaunchSpec> {
        if self.current.is_some() && !self.backend.alive() {
            let c = self.current.take();
            if let Some(c) = &c {
                self.discard(c);
            }
            return c;
        }
        None
    }

    /// Remember the current state as the baseline of a trial (Auto mode).
    /// A trial that is already active keeps its original baseline.
    pub fn begin_trial(&mut self) {
        if self.trial.is_none() {
            self.trial = Some(Trial {
                baseline: self.current.clone(),
            });
        }
    }

    /// End a trial. `commit` keeps whatever runs now; otherwise the baseline
    /// is restored exactly (or the engine is stopped if nothing ran before).
    pub fn end_trial(&mut self, commit: bool) -> SwitchReport {
        let Some(trial) = self.trial.take() else {
            return report(SwitchResult::Started, None, None, self.current.as_ref());
        };
        if commit {
            if let Some(b) = &trial.baseline {
                self.discard(b);
            }
            return report(SwitchResult::Started, None, None, self.current.as_ref());
        }
        match trial.baseline {
            None => {
                self.stop();
                report(SwitchResult::Started, None, None, None)
            }
            Some(b) => {
                let same = self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.gen_dir == b.gen_dir)
                    && self.backend.alive();
                if same {
                    return report(SwitchResult::Started, None, None, self.current.as_ref());
                }
                // Restore the baseline spec itself (its directory was kept).
                let old = self.current.take();
                self.backend.stop();
                if let Some(o) = &old {
                    self.discard(o);
                }
                match self.backend.start(&b) {
                    Ok(_) => {
                        self.current = Some(b);
                        report(SwitchResult::RolledBack, None, None, self.current.as_ref())
                    }
                    Err(e) => {
                        self.discard(&b);
                        report(SwitchResult::RollbackFailed, None, Some(e), None)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundles::Bundle;
    use bridge_core::manifest::BundleManifest;
    use bridge_core::Arg;
    use std::collections::HashSet;

    /// Fake engine: fails to start specs whose strategy id is in `fail`, can
    /// be made to fail all starts, and checks that its lists dir exists.
    #[derive(Default)]
    struct Fake {
        running: Option<(String, PathBuf, String)>,
        fail: HashSet<String>,
        fail_hash: HashSet<String>,
        fail_all: bool,
        starts: Vec<String>,
        stops: usize,
        processes: usize,
    }

    impl Backend for Fake {
        fn start(&mut self, spec: &LaunchSpec) -> Result<u32, String> {
            assert!(self.running.is_none(), "two engines at once");
            self.starts.push(spec.strategy.id.clone());
            if !spec.lists_dir.join("list-general.txt").exists() {
                return Err("lists missing".into());
            }
            if self.fail_all
                || self.fail.contains(&spec.strategy.id)
                || self.fail_hash.contains(&spec.config_hash)
            {
                return Err(format!("{} exited", spec.strategy.id));
            }
            self.running = Some((
                spec.strategy.id.clone(),
                spec.lists_dir.clone(),
                spec.config_hash.clone(),
            ));
            self.processes += 1;
            Ok(1000 + self.starts.len() as u32)
        }
        fn stop(&mut self) {
            if self.running.take().is_some() {
                self.processes -= 1;
            }
            self.stops += 1;
        }
        fn alive(&mut self) -> bool {
            self.running.is_some()
        }
    }

    fn bundle(v: &str) -> Bundle {
        Bundle {
            dir: PathBuf::from(format!("/nonexistent/{v}")),
            builtin: true,
            manifest: BundleManifest {
                format: 1,
                flowseal_version: v.into(),
                winws_version: "v72.9".into(),
                source_url: String::new(),
                source_sha256: String::new(),
                archive_root: String::new(),
                min_app_version: "0.1.0".into(),
                files: vec![],
            },
        }
    }

    fn strat(id: &str, desync: &str) -> Strategy {
        let mut s = Strategy::new_empty(id, id);
        s.rules[0].args[2] = Arg::text("--dpi-desync", desync);
        s
    }

    fn lists_ok(dir: &Path) -> Result<(), String> {
        std::fs::write(dir.join("list-general.txt"), "discord.com\n").map_err(|e| e.to_string())
    }

    fn prep(c: &mut Controller<Fake>, s: Strategy) -> LaunchSpec {
        c.prepare(
            s,
            RuntimeOptions::default(),
            bundle("1.10.4"),
            None,
            lists_ok,
            |d| Ok(vec![d.display().to_string()]),
        )
        .unwrap()
    }

    fn ctl() -> (tempfile::TempDir, Controller<Fake>) {
        let t = tempfile::tempdir().unwrap();
        let c = Controller::new(Fake::default(), t.path().join("runtime"));
        (t, c)
    }

    fn gens(c: &Controller<Fake>) -> usize {
        std::fs::read_dir(&c.root).map(|r| r.count()).unwrap_or(0)
    }

    #[test]
    fn switch_a_to_b_success_cleans_old_generation() {
        let (_t, mut c) = ctl();
        let a = prep(&mut c, strat("a", "fake"));
        assert_eq!(c.switch(a).result, SwitchResult::Started);
        let b = prep(&mut c, strat("b", "multisplit"));
        let r = c.switch(b);
        assert_eq!(r.result, SwitchResult::Started);
        assert_eq!(c.current().unwrap().strategy.id, "b");
        assert_eq!(gens(&c), 1, "only B's generation remains");
        assert_eq!(c.backend.processes, 1);
    }

    #[test]
    fn prepare_failure_never_touches_running_engine() {
        let (_t, mut c) = ctl();
        let a = prep(&mut c, strat("a", "fake"));
        c.switch(a);
        let err = c.prepare(
            strat("b", "fake"),
            RuntimeOptions::default(),
            bundle("1.10.4"),
            None,
            |_| Err("ресурс B не читается".into()),
            |_| Ok(vec![]),
        );
        assert!(err.is_err());
        assert_eq!(c.backend.stops, 0, "A was not stopped");
        assert_eq!(c.backend.running.as_ref().unwrap().0, "a");
        assert_eq!(gens(&c), 1, "failed generation removed, A's kept");
        let a_lists = c.current().unwrap().lists_dir.clone();
        assert!(
            a_lists.join("list-general.txt").exists(),
            "A's lists intact"
        );
    }

    #[test]
    fn render_failure_is_a_prepare_failure() {
        let (_t, mut c) = ctl();
        let a = prep(&mut c, strat("a", "fake"));
        c.switch(a);
        let r = c.prepare(
            strat("b", "fake"),
            RuntimeOptions::default(),
            bundle("1.10.4"),
            None,
            lists_ok,
            |_| Err("ресурс не найден".into()),
        );
        assert!(r.is_err());
        assert!(c.backend.alive());
    }

    #[test]
    fn start_failure_rolls_back_to_exact_previous_spec() {
        let (_t, mut c) = ctl();
        let a = prep(&mut c, strat("a", "fake"));
        let a_hash = a.config_hash.clone();
        c.switch(a);
        c.backend.fail.insert("b".into());
        let b = prep(&mut c, strat("b", "fake"));
        let r = c.switch(b);
        assert_eq!(r.result, SwitchResult::RolledBack);
        assert_eq!(r.running_name.as_deref(), Some("a"));
        assert_eq!(c.backend.running.as_ref().unwrap().2, a_hash);
        assert_eq!(gens(&c), 1);
        assert_eq!(c.backend.processes, 1);
    }

    #[test]
    fn restart_of_edited_same_id_rolls_back_to_old_parameters() {
        // Custom A runs; the user edits A (same id) into a broken config and restarts.
        let (_t, mut c) = ctl();
        let a_v1 = prep(&mut c, strat("a", "fake"));
        let v1_hash = a_v1.config_hash.clone();
        c.switch(a_v1);
        let a_v2 = prep(&mut c, strat("a", "multidisorder"));
        assert_ne!(a_v2.config_hash, v1_hash);
        c.backend.fail_hash.insert(a_v2.config_hash.clone());
        let r = c.switch(a_v2);
        assert_eq!(r.result, SwitchResult::RolledBack);
        assert_eq!(
            c.current().unwrap().config_hash,
            v1_hash,
            "old parameters, not the edited ones"
        );
        assert_eq!(c.backend.running.as_ref().unwrap().2, v1_hash);
        assert_eq!(c.backend.processes, 1);
    }

    #[test]
    fn rollback_failure_is_reported_honestly() {
        let (_t, mut c) = ctl();
        let a = prep(&mut c, strat("a", "fake"));
        c.switch(a);
        c.backend.fail_all = true;
        let b = prep(&mut c, strat("b", "fake"));
        let r = c.switch(b);
        assert_eq!(r.result, SwitchResult::RollbackFailed);
        assert!(r.running_name.is_none());
        assert!(r.rollback_error.is_some());
        assert!(c.current().is_none());
        assert_eq!(c.backend.processes, 0, "no leftover processes");
        assert_eq!(gens(&c), 0);
    }

    #[test]
    fn first_start_failure_leaves_nothing() {
        let (_t, mut c) = ctl();
        c.backend.fail.insert("a".into());
        let a = prep(&mut c, strat("a", "fake"));
        assert_eq!(c.switch(a).result, SwitchResult::Failed);
        assert_eq!(c.backend.processes, 0);
        assert_eq!(gens(&c), 0);
    }

    #[test]
    fn rapid_switches_never_run_two_engines_and_keep_one_generation() {
        let (_t, mut c) = ctl();
        for i in 0..20 {
            let s = prep(
                &mut c,
                strat(
                    &format!("s{i}"),
                    if i % 2 == 0 { "fake" } else { "multisplit" },
                ),
            );
            assert_eq!(c.switch(s).result, SwitchResult::Started);
        }
        assert_eq!(c.backend.processes, 1);
        assert_eq!(gens(&c), 1);
        c.stop();
        assert_eq!(c.backend.processes, 0);
        assert_eq!(gens(&c), 0);
    }

    #[test]
    fn trial_cancel_restores_baseline_even_after_several_attempts() {
        let (_t, mut c) = ctl();
        let a = prep(&mut c, strat("a", "fake"));
        let a_hash = a.config_hash.clone();
        c.switch(a);
        c.begin_trial();
        for id in ["t1", "t2", "t3"] {
            let s = prep(&mut c, strat(id, "multisplit"));
            c.switch(s);
        }
        let r = c.end_trial(false);
        assert_eq!(r.result, SwitchResult::RolledBack);
        assert_eq!(c.current().unwrap().config_hash, a_hash);
        assert!(
            c.current()
                .unwrap()
                .lists_dir
                .join("list-general.txt")
                .exists(),
            "baseline lists survived the trial"
        );
        assert_eq!(gens(&c), 1);
        assert!(!c.trial_active());
    }

    #[test]
    fn trial_from_stopped_state_returns_to_stopped() {
        let (_t, mut c) = ctl();
        c.begin_trial();
        let s = prep(&mut c, strat("t1", "fake"));
        c.switch(s);
        c.end_trial(false);
        assert!(c.current().is_none());
        assert_eq!(c.backend.processes, 0);
        assert_eq!(gens(&c), 0);
    }

    #[test]
    fn trial_commit_keeps_new_and_drops_baseline() {
        let (_t, mut c) = ctl();
        let a = prep(&mut c, strat("a", "fake"));
        c.switch(a);
        c.begin_trial();
        let s = prep(&mut c, strat("t1", "multisplit"));
        c.switch(s);
        c.end_trial(true);
        assert_eq!(c.current().unwrap().strategy.id, "t1");
        assert_eq!(gens(&c), 1);
    }

    #[test]
    fn restart_after_service_restart_cleans_leftovers() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("runtime");
        std::fs::create_dir_all(root.join("gen-7/lists")).unwrap();
        let c = Controller::new(Fake::default(), root.clone());
        assert_eq!(c.next_gen, 8);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
    }

    #[test]
    fn engine_death_is_reaped() {
        let (_t, mut c) = ctl();
        let a = prep(&mut c, strat("a", "fake"));
        c.switch(a);
        c.backend.running = None;
        c.backend.processes = 0;
        assert!(c.reap_if_dead().is_some());
        assert!(c.current().is_none());
        assert_eq!(gens(&c), 0);
    }
}
