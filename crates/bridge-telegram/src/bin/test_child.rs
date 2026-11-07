//! Test stand-in for the headless engine (never bundled). Same protocol:
//! one JSON config line on stdin, JSON events on stdout, stop on EOF/"stop".
//! Behaviour from `test-mode.txt` in the working directory (the manager
//! starts the engine in its own directory):
//!   normal       listen, report stats like the real runner: every TCP client
//!                counts in connections_active (as upstream, before any
//!                check); a client sending "auth" is relayed to "Telegram"
//!                (relays_active while open, cumulative ws/relays_total and
//!                data back); one sending "bad!" fails the check
//!                (connections_bad) and stays open until it closes
//!   crash        ready, then exit 7
//!   no-ready     never listen
//!   ignore-stop  listen, ignore EOF (must be killed)
//!   leak         print the secret on stderr (must be redacted by the parent)
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn emit(v: serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn main() {
    let mode = std::fs::read_to_string("test-mode.txt")
        .map(|m| m.trim().to_string())
        .unwrap_or_else(|_| "normal".into());
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).unwrap();
    let cfg: serde_json::Value = serde_json::from_str(&line).unwrap();
    let port = cfg["port"].as_u64().unwrap() as u16;
    if mode == "leak" {
        eprintln!(
            "Secret: {} link dd{}",
            cfg["secret"].as_str().unwrap(),
            cfg["secret"].as_str().unwrap()
        );
    }
    if mode == "no-ready" {
        std::thread::sleep(Duration::from_secs(600));
        return;
    }
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|_| {
        emit(serde_json::json!({"event":"error","code":"addressInUse","message":"bind"}));
        std::process::exit(3);
    });
    emit(
        serde_json::json!({"event":"ready","host":"127.0.0.1","port":port,"version":"1.11.4","runner":"test","fallbackCfproxy":cfg["fallbackCfproxy"]}),
    );
    if mode == "crash" {
        std::thread::sleep(Duration::from_millis(300));
        std::process::exit(7);
    }
    let total = Arc::new(AtomicU64::new(0));
    let active = Arc::new(AtomicU64::new(0));
    let ws = Arc::new(AtomicU64::new(0));
    let relays = Arc::new(AtomicU64::new(0));
    let bad = Arc::new(AtomicU64::new(0));
    {
        let (total, active, ws, relays, bad) = (
            total.clone(),
            active.clone(),
            ws.clone(),
            relays.clone(),
            bad.clone(),
        );
        std::thread::spawn(move || {
            for c in listener.incoming().flatten() {
                total.fetch_add(1, Ordering::SeqCst);
                active.fetch_add(1, Ordering::SeqCst);
                let (active, ws, relays, bad) =
                    (active.clone(), ws.clone(), relays.clone(), bad.clone());
                std::thread::spawn(move || {
                    let mut c = c;
                    let mut b = [0u8; 4];
                    if c.read_exact(&mut b).is_ok() {
                        let mut rest = [0u8; 1];
                        if &b == b"auth" {
                            ws.fetch_add(1, Ordering::SeqCst);
                            relays.fetch_add(1, Ordering::SeqCst);
                            let _ = c.read(&mut rest); // relayed until the client closes
                            relays.fetch_sub(1, Ordering::SeqCst);
                        } else {
                            bad.fetch_add(1, Ordering::SeqCst);
                            let _ = c.read(&mut rest); // drained until the client closes
                        }
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
    }
    let (t2, a2, w2, r2, b2) = (
        total.clone(),
        active.clone(),
        ws.clone(),
        relays.clone(),
        bad.clone(),
    );
    std::thread::spawn(move || loop {
        let w = w2.load(Ordering::SeqCst);
        emit(
            serde_json::json!({"event":"status","listening":true,"stats":{
            "connections_total": t2.load(Ordering::SeqCst),
            "connections_active": a2.load(Ordering::SeqCst),
            "connections_bad": b2.load(Ordering::SeqCst),
            "connections_ws": w, "bytes_down": w * 100,
            "relays_active": r2.load(Ordering::SeqCst), "relays_total": w}}),
        );
        std::thread::sleep(Duration::from_millis(100));
    });
    // Wait for EOF or "stop" on stdin.
    for l in std::io::stdin().lock().lines() {
        match l {
            Ok(l) if l.trim() == "stop" => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    if mode == "ignore-stop" {
        std::thread::sleep(Duration::from_secs(600));
    }
    emit(
        serde_json::json!({"event":"stopped","stats":{"connections_total": total.load(Ordering::SeqCst)}}),
    );
}
