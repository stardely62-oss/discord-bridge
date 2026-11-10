//! Authorization policy of the helper (platform independent).
//!
//! Authentication (who is connecting) is done by the Windows layer: the pipe
//! client must be the installed `discord-bridge.exe`, and its identity is
//! taken from the **client token** obtained by impersonating the pipe client
//! (not from a PID, so PID reuse cannot change it).
//!
//! Authorization — the network state of the machine is a shared resource:
//!
//! * **Read-only** commands (status, diagnostics, bundle list, engine log) are
//!   allowed for any authenticated client.
//! * **Control** commands (start/stop/trial, TCP timestamps, Flowseal bundle
//!   install/activation) are allowed when
//!   - nobody owns the engine, or
//!   - the client is the owner (same account SID), or
//!   - the owner's logon session no longer exists (user logged off), or
//!   - the client token is elevated (an administrator may always take over).
//! * The owner is the account that started the running engine. It is cleared
//!   when the engine stops.
//!
//! On a single-user machine this is invisible: the only user is always the
//! owner. With several users, one non-elevated user cannot stop or change the
//! bypass another logged-on user is running.

use bridge_core::ipc::Request;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    /// String SID of the client token user, e.g. `S-1-5-21-…-1001`.
    pub sid: String,
    pub session_id: u32,
    pub elevated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub sid: String,
    pub session_id: u32,
}

pub fn is_control(req: &Request) -> bool {
    !matches!(
        req,
        Request::Hello { .. }
            | Request::Status
            | Request::Diagnose
            | Request::ListBundles
            | Request::EngineLog
            | Request::ReadBundleList { .. }
    )
}

/// `session_alive(id)` reports whether a logon session still exists.
pub fn authorize(
    req: &Request,
    client: &ClientIdentity,
    owner: Option<&Owner>,
    session_alive: impl Fn(u32) -> bool,
) -> Result<(), String> {
    if !is_control(req) {
        return Ok(());
    }
    match owner {
        None => Ok(()),
        Some(o) if o.sid == client.sid => Ok(()),
        Some(_) if client.elevated => Ok(()),
        Some(o) if !session_alive(o.session_id) => Ok(()),
        Some(_) => Err("обходом DPI сейчас управляет другой пользователь Windows. Изменить его состояние может он сам или администратор.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::ipc::RuntimeOptions;

    fn id(sid: &str, session: u32, elevated: bool) -> ClientIdentity {
        ClientIdentity {
            sid: sid.into(),
            session_id: session,
            elevated,
        }
    }
    fn start() -> Request {
        Request::Start {
            strategy: Box::new(bridge_core::Strategy::new_empty("a", "A")),
            runtime: RuntimeOptions::default(),
        }
    }

    #[test]
    fn read_only_always_allowed() {
        let owner = Owner {
            sid: "S-1-5-21-1".into(),
            session_id: 1,
        };
        for r in [
            Request::Status,
            Request::Diagnose,
            Request::ListBundles,
            Request::EngineLog,
            Request::Hello {
                protocol: bridge_core::ipc::PROTOCOL_VERSION,
            },
            Request::ReadBundleList {
                version: "1.10.4".into(),
                name: "list-general.txt".into(),
                offset: 0,
            },
        ] {
            assert!(authorize(&r, &id("S-1-5-21-2", 2, false), Some(&owner), |_| true).is_ok());
        }
    }

    #[test]
    fn single_user_is_never_blocked() {
        let me = id("S-1-5-21-1", 1, false);
        let owner = Owner {
            sid: me.sid.clone(),
            session_id: 1,
        };
        assert!(authorize(&start(), &me, None, |_| true).is_ok());
        assert!(authorize(&Request::Stop, &me, Some(&owner), |_| true).is_ok());
    }

    #[test]
    fn other_user_cannot_control_but_admin_can() {
        let owner = Owner {
            sid: "S-1-5-21-1".into(),
            session_id: 1,
        };
        for r in [
            start(),
            Request::Stop,
            Request::SetTcpTimestamps {
                enable: true,
                user_confirmed: true,
            },
            Request::RestoreTcpTimestamps,
            Request::BeginTrial,
            Request::EndTrial { commit: false },
            Request::SetActiveBundle {
                version: "1".into(),
            },
        ] {
            assert!(authorize(&r, &id("S-1-5-21-2", 2, false), Some(&owner), |_| true).is_err());
            assert!(authorize(&r, &id("S-1-5-21-2", 2, true), Some(&owner), |_| true).is_ok());
        }
    }

    #[test]
    fn same_account_in_another_session_is_the_owner() {
        let owner = Owner {
            sid: "S-1-5-21-1".into(),
            session_id: 1,
        };
        assert!(authorize(
            &Request::Stop,
            &id("S-1-5-21-1", 3, false),
            Some(&owner),
            |_| true
        )
        .is_ok());
    }

    #[test]
    fn ownership_expires_with_logoff() {
        let owner = Owner {
            sid: "S-1-5-21-1".into(),
            session_id: 1,
        };
        assert!(authorize(
            &Request::Stop,
            &id("S-1-5-21-2", 2, false),
            Some(&owner),
            |s| s != 1
        )
        .is_ok());
    }
}

/// Bounded number of concurrently served pipe clients. A slot is released
/// when the returned guard is dropped.
#[derive(Debug, Default)]
pub struct ConnectionLimiter {
    active: std::sync::atomic::AtomicUsize,
    max: usize,
}

pub struct Slot<'a>(&'a ConnectionLimiter);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.0
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Slot that owns a reference to its limiter (can move into a thread).
pub struct OwnedSlot(std::sync::Arc<ConnectionLimiter>);

impl Drop for OwnedSlot {
    fn drop(&mut self) {
        self.0
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl ConnectionLimiter {
    pub fn try_acquire_owned(this: &std::sync::Arc<Self>) -> Option<OwnedSlot> {
        let s = this.try_acquire()?;
        std::mem::forget(s);
        Some(OwnedSlot(this.clone()))
    }

    pub fn new(max: usize) -> Self {
        ConnectionLimiter {
            active: Default::default(),
            max,
        }
    }
    pub fn try_acquire(&self) -> Option<Slot<'_>> {
        use std::sync::atomic::Ordering::SeqCst;
        let mut cur = self.active.load(SeqCst);
        loop {
            if cur >= self.max {
                return None;
            }
            match self.active.compare_exchange(cur, cur + 1, SeqCst, SeqCst) {
                Ok(_) => return Some(Slot(self)),
                Err(v) => cur = v,
            }
        }
    }
    #[cfg(test)]
    pub fn active(&self) -> usize {
        self.active.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// What to do with a TCP timestamps change request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpDecision {
    /// Already in the requested state: change nothing, record nothing.
    NoChange,
    /// Change it; remember `original` (if this is our first change).
    Change { record_original: Option<bool> },
    /// State unknown: refuse rather than guess.
    Unknown,
}

pub fn decide_tcp_change(
    requested: bool,
    current: Option<bool>,
    already_recorded: bool,
) -> TcpDecision {
    match current {
        None => TcpDecision::Unknown,
        Some(c) if c == requested => TcpDecision::NoChange,
        Some(c) => TcpDecision::Change {
            record_original: (!already_recorded).then_some(c),
        },
    }
}

/// Restore only what we changed, and only if nobody changed it since.
pub fn decide_tcp_restore(
    original: Option<bool>,
    we_set: Option<bool>,
    current: Option<bool>,
) -> Result<Option<bool>, String> {
    match (original, we_set, current) {
        (Some(o), Some(w), Some(c)) if c == w => Ok(Some(o)),
        (Some(_), Some(_), Some(_)) => Err(
            "настройку уже изменила другая программа или администратор — Bridge её не трогает"
                .into(),
        ),
        (Some(_), Some(_), None) => {
            Err("не удалось определить текущее состояние TCP timestamps".into())
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    #[test]
    fn limiter_bounds_and_releases() {
        let l = ConnectionLimiter::new(2);
        let a = l.try_acquire().unwrap();
        let _b = l.try_acquire().unwrap();
        assert!(l.try_acquire().is_none());
        drop(a);
        assert!(l.try_acquire().is_some());
        let shared = std::sync::Arc::new(ConnectionLimiter::new(1));
        let owned = ConnectionLimiter::try_acquire_owned(&shared).unwrap();
        assert!(ConnectionLimiter::try_acquire_owned(&shared).is_none());
        std::thread::spawn(move || drop(owned)).join().unwrap();
        assert_eq!(shared.active(), 0);
    }

    #[test]
    fn limiter_under_contention() {
        let l = std::sync::Arc::new(ConnectionLimiter::new(4));
        let max_seen = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hs: Vec<_> = (0..32)
            .map(|_| {
                let (l, m) = (l.clone(), max_seen.clone());
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        if let Some(_s) = l.try_acquire() {
                            m.fetch_max(l.active(), std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        assert!(max_seen.load(std::sync::atomic::Ordering::SeqCst) <= 4);
        assert_eq!(l.active(), 0);
    }

    #[test]
    fn tcp_change_decisions() {
        assert_eq!(
            decide_tcp_change(true, Some(true), false),
            TcpDecision::NoChange
        );
        assert_eq!(
            decide_tcp_change(true, Some(false), false),
            TcpDecision::Change {
                record_original: Some(false)
            }
        );
        assert_eq!(
            decide_tcp_change(true, Some(false), true),
            TcpDecision::Change {
                record_original: None
            }
        );
        assert_eq!(decide_tcp_change(true, None, false), TcpDecision::Unknown);
    }

    #[test]
    fn tcp_restore_decisions() {
        assert_eq!(
            decide_tcp_restore(Some(false), Some(true), Some(true)).unwrap(),
            Some(false)
        );
        assert!(
            decide_tcp_restore(Some(false), Some(true), Some(false)).is_err(),
            "changed by someone else"
        );
        assert_eq!(
            decide_tcp_restore(None, None, Some(true)).unwrap(),
            None,
            "we never changed it"
        );
        assert!(decide_tcp_restore(Some(false), Some(true), None).is_err());
    }
}
