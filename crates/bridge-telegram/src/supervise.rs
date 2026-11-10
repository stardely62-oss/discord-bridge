//! When to restart the proxy by itself after an unexpected exit.
//!
//! Bridge sits in the tray, so a crashed proxy can go unnoticed while
//! Telegram silently stops connecting. The rules are deliberately narrow:
//!
//! * only a process that **had been listening** is restarted — a start that
//!   fails (a busy port, a damaged component, wrong settings) is the user's
//!   to see and is never retried;
//! * only while the user wants the proxy running: after «Остановить» or a
//!   full exit nothing is restarted;
//! * at most [`MAX_RESTARTS`] attempts, after [`DELAYS`]; a restarted
//!   process that does not reach listening ends the attempts;
//! * the count starts again once the proxy has been listening for
//!   [`STABLE`] without interruption.
//!
//! The decision is a pure state machine over the observed state, so it is
//! tested without a process.

use crate::manager::ProxyState;
use std::time::{Duration, Instant};

pub const MAX_RESTARTS: u32 = 2;
pub const DELAYS: [Duration; MAX_RESTARTS as usize] =
    [Duration::from_secs(3), Duration::from_secs(20)];
pub const STABLE: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Default)]
pub struct Supervisor {
    /// The current process reached listening at least once.
    seen_listening: bool,
    attempts: u32,
    listening_since: Option<Instant>,
    restart_at: Option<Instant>,
}

impl Supervisor {
    /// A restart is scheduled and has not happened yet.
    pub fn pending(&self) -> bool {
        self.restart_at.is_some()
    }

    /// Feed the state seen now. `wanted`: the user started the proxy and has
    /// not stopped it. Returns true when the proxy should be started again
    /// right now.
    pub fn observe(&mut self, state: ProxyState, wanted: bool, now: Instant) -> bool {
        if !wanted {
            *self = Supervisor::default();
            return false;
        }
        match state {
            ProxyState::Listening | ProxyState::Connected => {
                self.seen_listening = true;
                self.restart_at = None;
                let since = *self.listening_since.get_or_insert(now);
                if now.duration_since(since) >= STABLE {
                    self.attempts = 0;
                }
                false
            }
            ProxyState::Error => {
                self.listening_since = None;
                if !self.seen_listening || self.attempts >= MAX_RESTARTS {
                    self.restart_at = None;
                    return false;
                }
                let at = *self
                    .restart_at
                    .get_or_insert(now + DELAYS[self.attempts as usize]);
                if now < at {
                    return false;
                }
                self.restart_at = None;
                self.attempts += 1;
                self.seen_listening = false;
                true
            }
            ProxyState::Starting => {
                self.listening_since = None;
                false
            }
            ProxyState::Stopped | ProxyState::NotInstalled => {
                self.listening_since = None;
                self.restart_at = None;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProxyState::*;

    fn at(t0: Instant, s: u64) -> Instant {
        t0 + Duration::from_secs(s)
    }

    #[test]
    fn a_crash_after_listening_is_restarted_after_the_delay() {
        let t = Instant::now();
        let mut s = Supervisor::default();
        assert!(!s.observe(Starting, true, t));
        assert!(!s.observe(Listening, true, at(t, 1)));
        assert!(!s.observe(Error, true, at(t, 10)));
        assert!(s.pending());
        assert!(!s.observe(Error, true, at(t, 12)));
        assert!(s.observe(Error, true, at(t, 13)));
        assert!(!s.pending());
    }

    #[test]
    fn a_failed_start_is_never_retried() {
        let t = Instant::now();
        let mut s = Supervisor::default();
        assert!(!s.observe(Starting, true, t));
        for i in 1..120 {
            assert!(!s.observe(Error, true, at(t, i)));
            assert!(!s.pending());
        }
    }

    #[test]
    fn a_restart_that_does_not_reach_listening_ends_the_attempts() {
        let t = Instant::now();
        let mut s = Supervisor::default();
        s.observe(Connected, true, t);
        s.observe(Error, true, at(t, 1));
        assert!(s.observe(Error, true, at(t, 4)));
        s.observe(Starting, true, at(t, 5));
        for i in 6..200 {
            assert!(!s.observe(Error, true, at(t, i)));
        }
    }

    #[test]
    fn at_most_two_restarts_until_the_proxy_has_been_stable() {
        let t = Instant::now();
        let mut s = Supervisor::default();
        s.observe(Listening, true, t);
        s.observe(Error, true, at(t, 1));
        assert!(s.observe(Error, true, at(t, 4)));
        s.observe(Listening, true, at(t, 6));
        s.observe(Error, true, at(t, 10));
        assert!(!s.observe(Error, true, at(t, 29)));
        assert!(s.observe(Error, true, at(t, 30)));
        s.observe(Listening, true, at(t, 32));
        // Third crash soon after: no more attempts.
        for i in 40..400 {
            assert!(!s.observe(Error, true, at(t, i)));
        }
        // After a long stable run the count starts again.
        let mut s = Supervisor::default();
        s.observe(Listening, true, t);
        s.observe(Error, true, at(t, 1));
        assert!(s.observe(Error, true, at(t, 4)));
        s.observe(Listening, true, at(t, 5));
        s.observe(Error, true, at(t, 6));
        assert!(s.observe(Error, true, at(t, 26)));
        s.observe(Listening, true, at(t, 30));
        s.observe(Listening, true, at(t, 30 + STABLE.as_secs()));
        s.observe(Error, true, at(t, 400));
        assert!(s.observe(Error, true, at(t, 403)));
    }

    #[test]
    fn nothing_is_restarted_once_the_user_stopped_the_proxy() {
        let t = Instant::now();
        let mut s = Supervisor::default();
        s.observe(Listening, true, t);
        s.observe(Error, true, at(t, 1));
        assert!(s.pending());
        assert!(!s.observe(Error, false, at(t, 2)));
        assert!(!s.pending());
        assert!(!s.observe(Error, false, at(t, 60)));
        // A manual stop shows as Stopped and also cancels a pending restart.
        let mut s = Supervisor::default();
        s.observe(Listening, true, t);
        s.observe(Error, true, at(t, 1));
        assert!(!s.observe(Stopped, true, at(t, 2)));
        assert!(!s.pending());
    }
}
