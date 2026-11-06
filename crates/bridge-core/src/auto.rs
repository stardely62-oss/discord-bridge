//! Automatic strategy selection ("Auto"), independent of Tauri and Windows.
//!
//! Contract:
//! 1. `begin` starts a trial in the helper (the current state — running
//!    configuration snapshot or "stopped" — becomes the baseline).
//! 2. If the current configuration already works, it is kept.
//! 3. Candidates are tried in order, each at most once, up to
//!    `max_attempts`, within `total_timeout`, checking cancellation before
//!    every attempt.
//! 4. Success commits the trial. **Every other ending** — cancellation,
//!    timeout, exhausted candidates, helper/IPC failure, unexpected answers —
//!    ends the trial without commit, which restores the baseline exactly
//!    (or returns to "stopped"). The result of that restore is reported.
//!
//! "Works" is decided by the environment; the documented criterion used by
//! the app is: the engine is running and the Discord API answers with a
//! valid gateway JSON over HTTPS. Voice cannot be verified automatically.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryOutcome {
    /// Engine running and the connectivity criterion passed.
    Working,
    /// Engine running, criterion failed (or attempt timed out).
    NotWorking(String),
    /// The engine could not start with this candidate.
    EngineFailed(String),
    /// Continuing makes no sense (conflict, helper unavailable, IPC error).
    Fatal(String),
}

#[allow(async_fn_in_trait)]
pub trait AutoEnv {
    async fn begin(&mut self) -> Result<(), String>;
    /// Id of the running strategy if it already meets the criterion.
    async fn current_works(&mut self) -> Option<String>;
    async fn try_strategy(&mut self, id: &str) -> TryOutcome;
    /// End the trial; returns a description of the resulting state.
    async fn end(&mut self, commit: bool) -> Result<String, String>;
    fn cancelled(&self) -> bool;
    fn elapsed(&self) -> Duration;
    fn log(&mut self, line: String);
}

#[derive(Debug, Clone)]
pub struct AutoConfig {
    pub candidates: Vec<String>,
    pub max_attempts: usize,
    pub total_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoEnd {
    AlreadyWorking(String),
    Found(String),
    Cancelled,
    TimedOut,
    Exhausted,
    Fatal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoReport {
    pub end: AutoEnd,
    pub attempts: usize,
    /// Outcome of `end(false)` for unsuccessful runs (None if not called).
    pub restore: Option<Result<String, String>>,
}

impl AutoReport {
    pub fn success_id(&self) -> Option<&str> {
        match &self.end {
            AutoEnd::AlreadyWorking(id) | AutoEnd::Found(id) => Some(id),
            _ => None,
        }
    }
}

pub async fn run_auto<E: AutoEnv>(env: &mut E, cfg: &AutoConfig) -> AutoReport {
    if let Err(e) = env.begin().await {
        env.log(format!("не удалось начать подбор: {e}"));
        return AutoReport {
            end: AutoEnd::Fatal(e),
            attempts: 0,
            restore: None,
        };
    }
    env.log("проверка текущего подключения".into());
    if let Some(id) = env.current_works().await {
        env.log(format!(
            "текущая стратегия {id} уже работает — переключение не требуется"
        ));
        let r = env.end(true).await;
        return AutoReport {
            end: AutoEnd::AlreadyWorking(id),
            attempts: 0,
            restore: r.err().map(Err),
        };
    }
    let mut seen = std::collections::HashSet::new();
    let mut attempts = 0usize;
    let mut end = AutoEnd::Exhausted;
    for id in &cfg.candidates {
        if attempts >= cfg.max_attempts {
            break;
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        if env.cancelled() {
            end = AutoEnd::Cancelled;
            break;
        }
        if env.elapsed() >= cfg.total_timeout {
            end = AutoEnd::TimedOut;
            break;
        }
        attempts += 1;
        env.log(format!(
            "попытка {attempts}/{}: {id}",
            cfg.max_attempts.min(cfg.candidates.len())
        ));
        match env.try_strategy(id).await {
            TryOutcome::Working => {
                env.log(format!("{id}: Discord отвечает — выбрана"));
                end = AutoEnd::Found(id.clone());
                break;
            }
            TryOutcome::NotWorking(why) => env.log(format!("{id}: не подходит ({why})")),
            TryOutcome::EngineFailed(why) => env.log(format!("{id}: не запустилась ({why})")),
            TryOutcome::Fatal(why) => {
                env.log(format!("остановлено: {why}"));
                end = AutoEnd::Fatal(why);
                break;
            }
        }
    }
    // A cancellation that arrived during the last attempt still wins over "exhausted".
    if end == AutoEnd::Exhausted && env.cancelled() {
        end = AutoEnd::Cancelled;
    }
    match &end {
        AutoEnd::Found(_) => {
            let r = env.end(true).await;
            AutoReport {
                end,
                attempts,
                restore: r.err().map(Err),
            }
        }
        _ => {
            let r = env.end(false).await;
            match &r {
                Ok(desc) => env.log(format!("исходное состояние восстановлено: {desc}")),
                Err(e) => env.log(format!("НЕ удалось восстановить исходное состояние: {e}")),
            }
            AutoReport {
                end,
                attempts,
                restore: Some(r),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn block_on<F: Future>(f: F) -> F::Output {
        fn noop(_: *const ()) {}
        fn clone(p: *const ()) -> RawWaker {
            RawWaker::new(p, &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut f = pin!(f);
        loop {
            if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    /// Simulated helper: tracks what is running, the trial baseline and
    /// whether the baseline was restored.
    struct Env {
        running: Option<String>,
        baseline: Option<Option<String>>,
        works: HashMap<String, TryOutcome>,
        current_ok: bool,
        cancel_after: Option<usize>,
        tries: usize,
        time_per_try: Duration,
        begin_err: bool,
        end_err: bool,
        ended: Vec<bool>,
        log: Vec<String>,
    }

    impl Env {
        fn new(running: Option<&str>) -> Self {
            Env {
                running: running.map(str::to_string),
                baseline: None,
                works: HashMap::new(),
                current_ok: false,
                cancel_after: None,
                tries: 0,
                time_per_try: Duration::from_secs(5),
                begin_err: false,
                end_err: false,
                ended: vec![],
                log: vec![],
            }
        }
    }

    impl AutoEnv for Env {
        async fn begin(&mut self) -> Result<(), String> {
            if self.begin_err {
                return Err("helper unavailable".into());
            }
            self.baseline = Some(self.running.clone());
            Ok(())
        }
        async fn current_works(&mut self) -> Option<String> {
            if self.current_ok {
                self.running.clone()
            } else {
                None
            }
        }
        async fn try_strategy(&mut self, id: &str) -> TryOutcome {
            self.tries += 1;
            let o = self
                .works
                .get(id)
                .cloned()
                .unwrap_or(TryOutcome::NotWorking("https".into()));
            if !matches!(o, TryOutcome::EngineFailed(_) | TryOutcome::Fatal(_)) {
                self.running = Some(id.to_string());
            }
            o
        }
        async fn end(&mut self, commit: bool) -> Result<String, String> {
            self.ended.push(commit);
            if !commit {
                if self.end_err {
                    return Err("restore failed".into());
                }
                self.running = self.baseline.take().flatten();
            }
            Ok(format!("{:?}", self.running))
        }
        fn cancelled(&self) -> bool {
            self.cancel_after.is_some_and(|n| self.tries >= n)
        }
        fn elapsed(&self) -> Duration {
            self.time_per_try * self.tries as u32
        }
        fn log(&mut self, line: String) {
            self.log.push(line);
        }
    }

    fn cfg(c: &[&str]) -> AutoConfig {
        AutoConfig {
            candidates: c.iter().map(|s| s.to_string()).collect(),
            max_attempts: 8,
            total_timeout: Duration::from_secs(300),
        }
    }

    #[test]
    fn s01_current_already_works_is_kept() {
        let mut e = Env::new(Some("a"));
        e.current_ok = true;
        let r = block_on(run_auto(&mut e, &cfg(&["b", "c"])));
        assert_eq!(r.end, AutoEnd::AlreadyWorking("a".into()));
        assert_eq!(e.tries, 0);
        assert_eq!(e.ended, vec![true]);
    }

    #[test]
    fn s02_found_on_second_candidate_commits() {
        let mut e = Env::new(Some("a"));
        e.works.insert("c".into(), TryOutcome::Working);
        let r = block_on(run_auto(&mut e, &cfg(&["b", "c", "d"])));
        assert_eq!(r.end, AutoEnd::Found("c".into()));
        assert_eq!(e.running.as_deref(), Some("c"));
        assert_eq!(e.ended, vec![true]);
    }

    #[test]
    fn s03_cancel_before_first_attempt_restores() {
        let mut e = Env::new(Some("a"));
        e.cancel_after = Some(0);
        let r = block_on(run_auto(&mut e, &cfg(&["b"])));
        assert_eq!(r.end, AutoEnd::Cancelled);
        assert_eq!(e.tries, 0);
        assert_eq!(e.running.as_deref(), Some("a"));
    }

    #[test]
    fn s04_cancel_mid_run_restores_baseline_not_last_trial() {
        let mut e = Env::new(Some("a"));
        e.cancel_after = Some(2);
        let r = block_on(run_auto(&mut e, &cfg(&["b", "c", "d"])));
        assert_eq!(r.end, AutoEnd::Cancelled);
        assert_eq!(e.running.as_deref(), Some("a"));
        assert_eq!(e.ended, vec![false]);
    }

    #[test]
    fn s05_cancel_during_last_attempt_is_cancel_not_exhausted() {
        let mut e = Env::new(Some("a"));
        e.cancel_after = Some(1);
        let r = block_on(run_auto(&mut e, &cfg(&["b"])));
        assert_eq!(r.end, AutoEnd::Cancelled);
        assert_eq!(e.running.as_deref(), Some("a"));
    }

    #[test]
    fn s06_total_timeout_restores() {
        let mut e = Env::new(Some("a"));
        e.time_per_try = Duration::from_secs(120);
        let r = block_on(run_auto(&mut e, &cfg(&["b", "c", "d", "e"])));
        assert_eq!(r.end, AutoEnd::TimedOut);
        assert_eq!(e.tries, 3);
        assert_eq!(e.running.as_deref(), Some("a"));
    }

    #[test]
    fn s07_exhausted_from_running_restores_running() {
        let mut e = Env::new(Some("a"));
        let r = block_on(run_auto(&mut e, &cfg(&["b", "c"])));
        assert_eq!(r.end, AutoEnd::Exhausted);
        assert_eq!(e.running.as_deref(), Some("a"));
        assert!(matches!(r.restore, Some(Ok(_))));
    }

    #[test]
    fn s08_exhausted_from_stopped_returns_to_stopped() {
        let mut e = Env::new(None);
        let r = block_on(run_auto(&mut e, &cfg(&["b", "c"])));
        assert_eq!(r.end, AutoEnd::Exhausted);
        assert_eq!(e.running, None, "bypass must not stay enabled");
    }

    #[test]
    fn s09_fatal_helper_error_restores() {
        let mut e = Env::new(Some("a"));
        e.works
            .insert("c".into(), TryOutcome::Fatal("канал службы закрыт".into()));
        let r = block_on(run_auto(&mut e, &cfg(&["b", "c", "d"])));
        assert_eq!(r.end, AutoEnd::Fatal("канал службы закрыт".into()));
        assert_eq!(e.tries, 2);
        assert_eq!(e.running.as_deref(), Some("a"));
    }

    #[test]
    fn s10_begin_failure_touches_nothing() {
        let mut e = Env::new(Some("a"));
        e.begin_err = true;
        let r = block_on(run_auto(&mut e, &cfg(&["b"])));
        assert!(matches!(r.end, AutoEnd::Fatal(_)));
        assert!(e.ended.is_empty());
        assert_eq!(e.tries, 0);
    }

    #[test]
    fn s11_engine_failures_continue_then_restore() {
        let mut e = Env::new(Some("a"));
        for id in ["b", "c"] {
            e.works
                .insert(id.into(), TryOutcome::EngineFailed("exit 1".into()));
        }
        let r = block_on(run_auto(&mut e, &cfg(&["b", "c"])));
        assert_eq!(r.end, AutoEnd::Exhausted);
        assert_eq!(e.tries, 2);
        assert_eq!(e.running.as_deref(), Some("a"));
    }

    #[test]
    fn s12_max_attempts_and_no_repeats() {
        let mut e = Env::new(None);
        let mut c = cfg(&["b", "b", "c", "d", "e", "f"]);
        c.max_attempts = 3;
        let r = block_on(run_auto(&mut e, &c));
        assert_eq!(r.attempts, 3);
        assert_eq!(e.tries, 3);
        assert!(e.log.iter().filter(|l| l.contains(": b")).count() <= 1);
    }

    #[test]
    fn s13_failed_restore_is_reported_not_hidden() {
        let mut e = Env::new(Some("a"));
        e.end_err = true;
        let r = block_on(run_auto(&mut e, &cfg(&["b"])));
        assert_eq!(r.restore, Some(Err("restore failed".into())));
        assert!(e.log.iter().any(|l| l.contains("НЕ удалось")));
    }

    #[test]
    fn s14_empty_candidate_list_restores() {
        let mut e = Env::new(Some("a"));
        let r = block_on(run_auto(&mut e, &cfg(&[])));
        assert_eq!(r.end, AutoEnd::Exhausted);
        assert_eq!(e.ended, vec![false]);
    }
}
