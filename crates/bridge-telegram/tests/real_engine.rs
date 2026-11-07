//! The real headless engine (Windows CI after telegram-proxy/build.ps1):
//!   TGPROXY_ENGINE_DIR=resources/tg-ws-proxy cargo test -p bridge-telegram --test real_engine
//! Skipped when the variable is not set.
use bridge_telegram::component::{locate, Manifest};
use bridge_telegram::{ProxyManager, ProxyState, Secret, TelegramProxySettings, MANIFEST_NAME};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn wait(
    m: &ProxyManager,
    what: &str,
    secs: u64,
    f: impl Fn(&bridge_telegram::ProxyStatus) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if f(&m.status()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "timeout waiting for {what}: {:?}\nlog:\n{}",
        m.status(),
        m.log_tail().join("\n")
    );
}

#[test]
fn the_built_engine_listens_counts_and_stops_with_all_its_processes() {
    let Ok(dir) = std::env::var("TGPROXY_ENGINE_DIR") else {
        eprintln!("TGPROXY_ENGINE_DIR not set: skipped");
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(dir.join(MANIFEST_NAME)).unwrap()).unwrap();
    let comp = locate(&dir, Some(&manifest.sha256)).expect("engine verifies");
    assert_eq!(comp.manifest.version, "1.11.4");
    let port = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let settings = TelegramProxySettings {
        port,
        ..Default::default()
    };
    let secret = Secret::generate().unwrap();
    let m = ProxyManager::new(|| {});
    m.start(&comp, &manifest.sha256, &settings, &secret)
        .unwrap();
    wait(&m, "listening", 90, |s| s.state == ProxyState::Listening);
    let pids = m.owned_pids();
    eprintln!("engine processes in the job: {pids:?}");
    assert!(!pids.is_empty());
    #[cfg(windows)]
    {
        assert!(
            pids.len() >= 2,
            "PyInstaller bootloader and its Python process are both in the job"
        );
        assert_eq!(m.listener_is_ours(), Some(true));
    }
    // A client with a wrong handshake: counted and rejected, nothing relayed.
    {
        let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut junk = [0u8; 64];
        getrandom_fill(&mut junk);
        c.write_all(&junk).unwrap();
        std::thread::sleep(Duration::from_millis(500));
    }
    wait(&m, "the connection counted", 30, |s| {
        s.stats.connections_total >= 1
    });
    let st = m.status();
    assert_eq!(st.relayed, 0);
    assert!(!st.data_from_telegram);
    assert_eq!(
        st.state,
        ProxyState::Listening,
        "an open port is not 'connected'"
    );
    let log = m.log_tail().join("\n");
    assert!(
        !log.to_ascii_lowercase().contains(&secret.expose_hex()),
        "{log}"
    );
    m.stop();
    let st = m.status();
    assert_eq!(st.state, ProxyState::Stopped, "{st:?}");
    assert_eq!(st.exit_code, Some(0), "graceful stop through stdin");
    assert!(
        TcpListener::bind(("127.0.0.1", port)).is_ok(),
        "port released"
    );
    #[cfg(windows)]
    for pid in pids {
        assert!(
            !process_alive(pid),
            "process {pid} of the engine survived the stop"
        );
    }
}

fn getrandom_fill(b: &mut [u8]) {
    let s = Secret::generate().unwrap().expose_hex();
    for (i, x) in b.iter_mut().enumerate() {
        *x = s.as_bytes()[i % s.len()] ^ (i as u8);
    }
}

#[cfg(windows)]
fn process_alive(pid: u32) -> bool {
    let out = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}
