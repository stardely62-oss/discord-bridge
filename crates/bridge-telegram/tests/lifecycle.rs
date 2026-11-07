//! Lifecycle of the proxy process with a stand-in engine (same protocol).
use bridge_telegram::component::{locate, Manifest};
use bridge_telegram::{
    ProxyManager, ProxyState, Secret, TelegramProxySettings, EXE_NAME, MANIFEST_NAME,
};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn install(dir: &Path, mode: &str) -> String {
    let body = std::fs::read(env!("CARGO_BIN_EXE_tgproxy-test-child")).unwrap();
    let exe = dir.join(EXE_NAME);
    std::fs::write(&exe, &body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(dir.join("test-mode.txt"), mode).unwrap();
    let sha = hex::encode(Sha256::digest(&body));
    let m = Manifest {
        component: "tg-ws-proxy-headless".into(),
        version: "1.11.4".into(),
        upstream_commit: "2085861957070eeea07990a522b29f42e5230210".into(),
        runner: "test".into(),
        python: String::new(),
        sha256: sha.clone(),
        size: body.len() as u64,
    };
    std::fs::write(dir.join(MANIFEST_NAME), serde_json::to_vec(&m).unwrap()).unwrap();
    sha
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for(m: &ProxyManager, what: &str, f: impl Fn(&bridge_telegram::ProxyStatus) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if f(&m.status()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("timeout waiting for {what}: {:?}", m.status());
}

struct Setup {
    _dir: tempfile::TempDir,
    m: ProxyManager,
    notified: Arc<AtomicUsize>,
    comp: bridge_telegram::Component,
    sha: String,
    settings: TelegramProxySettings,
    secret: Secret,
}

fn setup(mode: &str) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let sha = install(dir.path(), mode);
    let comp = locate(dir.path(), Some(&sha)).unwrap();
    let notified = Arc::new(AtomicUsize::new(0));
    let n = notified.clone();
    let m = ProxyManager::new(move || {
        n.fetch_add(1, Ordering::SeqCst);
    });
    Setup {
        _dir: dir,
        m,
        notified,
        comp,
        sha,
        settings: TelegramProxySettings {
            port: free_port(),
            ..Default::default()
        },
        secret: Secret::generate().unwrap(),
    }
}

#[test]
fn start_listen_connect_stop() {
    let s = setup("normal");
    assert_eq!(s.m.status().state, ProxyState::Stopped);
    s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).unwrap();
    assert!(
        s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).is_err(),
        "no second instance"
    );
    wait_for(&s.m, "listening", |st| st.state == ProxyState::Listening);
    let st = s.m.status();
    assert_eq!((st.host.as_str(), st.port), ("127.0.0.1", s.settings.port));
    assert_eq!(st.version.as_deref(), Some("1.11.4"));
    assert!(st.pid.is_some());
    // An open port alone is not "connected".
    let idle = TcpStream::connect(("127.0.0.1", s.settings.port)).unwrap();
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(s.m.status().state, ProxyState::Listening);
    drop(idle);
    // An authenticated, relayed client is.
    let mut c = TcpStream::connect(("127.0.0.1", s.settings.port)).unwrap();
    c.write_all(b"auth").unwrap();
    wait_for(&s.m, "connected", |st| {
        st.state == ProxyState::Connected && st.relays_active == 1
    });
    drop(c);
    wait_for(&s.m, "client gone", |st| st.state == ProxyState::Listening);
    s.m.stop();
    let st = s.m.status();
    assert_eq!(st.state, ProxyState::Stopped);
    assert_eq!(st.exit_code, Some(0), "graceful stop, not killed");
    assert!(!s.m.is_running());
    assert!(s.notified.load(Ordering::SeqCst) >= 3);
    // The port is free again and a new start works.
    s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).unwrap();
    wait_for(&s.m, "listening again", |st| {
        st.state == ProxyState::Listening
    });
    s.m.restart(&s.comp, &s.sha, &s.settings, &s.secret)
        .unwrap();
    wait_for(&s.m, "listening after restart", |st| {
        st.state == ProxyState::Listening
    });
    s.m.stop();
}

#[test]
fn an_unexpected_exit_is_an_error_without_automatic_restart() {
    let s = setup("crash");
    s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).unwrap();
    wait_for(&s.m, "error", |st| st.state == ProxyState::Error);
    let st = s.m.status();
    assert_eq!(st.exit_code, Some(7));
    assert!(st.last_error.unwrap().contains("неожиданно"));
    std::thread::sleep(Duration::from_secs(1));
    assert!(!s.m.is_running(), "no restart loop");
    assert_eq!(s.m.status().state, ProxyState::Error);
}

#[test]
fn a_process_that_ignores_stop_is_terminated() {
    let s = setup("ignore-stop");
    s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).unwrap();
    wait_for(&s.m, "listening", |st| st.state == ProxyState::Listening);
    let t = Instant::now();
    s.m.stop();
    assert!(!s.m.is_running());
    assert_eq!(s.m.status().state, ProxyState::Stopped);
    assert!(t.elapsed() >= bridge_telegram::manager::STOP_TIMEOUT);
    assert!(
        TcpListener::bind(("127.0.0.1", s.settings.port)).is_ok(),
        "port released"
    );
}

#[test]
fn a_busy_port_is_reported_and_the_other_program_is_left_alone() {
    let s = setup("normal");
    let other = TcpListener::bind(("127.0.0.1", s.settings.port)).unwrap();
    let e =
        s.m.start(&s.comp, &s.sha, &s.settings, &s.secret)
            .unwrap_err();
    assert!(e.contains("занят"));
    let st = s.m.status();
    assert_eq!(
        (st.state, st.error_code.as_deref()),
        (ProxyState::Error, Some("addressInUse"))
    );
    assert!(!s.m.is_running(), "nothing was started");
    // The other listener still accepts connections.
    let _c = TcpStream::connect(("127.0.0.1", s.settings.port)).unwrap();
    other.accept().unwrap();
}

#[test]
fn a_changed_executable_is_never_started() {
    let s = setup("normal");
    let mut body = std::fs::read(&s.comp.exe).unwrap();
    let last = body.len() - 1;
    body[last] ^= 0xff;
    std::fs::write(&s.comp.exe, &body).unwrap();
    let e =
        s.m.start(&s.comp, &s.sha, &s.settings, &s.secret)
            .unwrap_err();
    assert!(e.contains("SHA-256"));
    assert_eq!(s.m.status().error_code.as_deref(), Some("integrity"));
    assert!(!s.m.is_running());
}

#[test]
fn the_secret_is_redacted_from_the_engine_log() {
    let s = setup("leak");
    s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).unwrap();
    wait_for(&s.m, "listening", |st| st.state == ProxyState::Listening);
    std::thread::sleep(Duration::from_millis(300));
    let log = s.m.log_tail().join("\n");
    assert!(log.contains("<secret>"), "{log}");
    assert!(!log.contains(&s.secret.expose_hex()));
    let status = serde_json::to_string(&s.m.status()).unwrap();
    assert!(!status.contains(&s.secret.expose_hex()));
    s.m.stop();
}

#[test]
fn an_engine_that_never_listens_times_out_once() {
    // Uses the real timeout only when explicitly asked (30 s).
    if std::env::var("TGPROXY_SLOW_TESTS").is_err() {
        return;
    }
    let s = setup("no-ready");
    s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).unwrap();
    std::thread::sleep(bridge_telegram::manager::START_TIMEOUT + Duration::from_secs(2));
    assert_eq!(s.m.status().state, ProxyState::Error);
    assert_eq!(s.m.status().error_code.as_deref(), Some("startTimeout"));
}

fn auth(port: u16) -> TcpStream {
    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.write_all(b"auth").unwrap();
    c
}
fn bad(port: u16) -> TcpStream {
    let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
    c.write_all(b"bad!").unwrap();
    c
}
fn settle(m: &ProxyManager, what: &str, f: impl Fn(&bridge_telegram::ProxyStatus) -> bool) {
    wait_for(m, what, &f);
    // A few status ticks more: the state must stay as it is.
    std::thread::sleep(Duration::from_millis(400));
    assert!(f(&m.status()), "{what} did not hold: {:?}", m.status());
}

/// Regression: "connected" only for an authorised connection relayed now —
/// never for wrong secrets, never from cumulative counters.
#[test]
fn connected_means_an_authorised_connection_relayed_now() {
    let s = setup("normal");
    let port = s.settings.port;
    s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).unwrap();
    wait_for(&s.m, "listening", |st| st.state == ProxyState::Listening);

    // 1. A wrong secret never gives "connected" (upstream counts it active).
    let b1 = bad(port);
    settle(&s.m, "wrong secret only", |st| {
        st.stats.connections_bad == 1 && st.clients_active == 1 && st.state == ProxyState::Listening
    });

    // 2. A current authorised connection gives "connected".
    let a1 = auth(port);
    settle(&s.m, "authorised", |st| {
        st.relays_active == 1 && st.state == ProxyState::Connected
    });

    // 3. All correct connections closed → not connected (bad one still open).
    drop(a1);
    settle(&s.m, "authorised closed", |st| {
        st.relays_active == 0
            && st.relayed == 1
            && st.clients_active == 1
            && st.state == ProxyState::Listening
    });
    drop(b1);

    // 4. A past success, then a client with a wrong secret: the old rule
    //    (active > 0 && relayed > 0) would say connected; it must not.
    let b2 = bad(port);
    settle(&s.m, "past success + wrong secret", |st| {
        st.clients_active == 1
            && st.relayed == 1
            && st.data_from_telegram
            && st.relays_active == 0
            && st.state == ProxyState::Listening
    });
    drop(b2);

    // 5. Counters of past connections alone never make "connected".
    settle(&s.m, "only history", |st| {
        st.clients_active == 0 && st.relayed == 1 && st.state == ProxyState::Listening
    });

    // And a new authorised connection is "connected" again.
    let a2 = auth(port);
    settle(&s.m, "authorised again", |st| {
        st.state == ProxyState::Connected
    });
    drop(a2);
    s.m.stop();
}

/// Regression: the link and the checks use the port of the running process
/// until a restart applies a changed port.
#[test]
fn the_running_port_is_used_until_a_restart() {
    use bridge_telegram::manager::connection_port;
    let s = setup("normal");
    let old = s.settings.port;
    assert_eq!(s.m.running_port(), None);
    assert_eq!(connection_port(old, s.m.running_port()), old);
    s.m.start(&s.comp, &s.sha, &s.settings, &s.secret).unwrap();
    wait_for(&s.m, "listening", |st| st.state == ProxyState::Listening);
    // The user saves another port without restarting.
    let changed = TelegramProxySettings {
        port: free_port(),
        ..s.settings.clone()
    };
    assert_eq!(s.m.running_port(), Some(old));
    let link_port = connection_port(changed.port, s.m.running_port());
    assert_eq!(
        link_port, old,
        "the link points at the port that really listens"
    );
    assert!(TcpStream::connect(("127.0.0.1", link_port)).is_ok());
    assert!(
        TcpStream::connect(("127.0.0.1", changed.port)).is_err(),
        "nothing listens on the saved port yet"
    );
    // After the restart the new port is used.
    s.m.restart(&s.comp, &s.sha, &changed, &s.secret).unwrap();
    wait_for(&s.m, "listening on the new port", |st| {
        st.state == ProxyState::Listening && st.port == changed.port
    });
    assert_eq!(
        connection_port(changed.port, s.m.running_port()),
        changed.port
    );
    assert!(TcpStream::connect(("127.0.0.1", changed.port)).is_ok());
    s.m.stop();
    assert_eq!(
        connection_port(changed.port, s.m.running_port()),
        changed.port
    );
}
