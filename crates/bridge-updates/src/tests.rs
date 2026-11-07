//! Network tests against a local HTTP server (GitHub is never contacted).
use super::*;
use base64::Engine;
use bridge_core::manifest::{sha256_hex, sign, BundleManifest, FileEntry};
use bridge_core::updates::UpdateStatus;
use ed25519_dalek::SigningKey;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const APP: &str = "0.1.0-alpha.10";

#[derive(Clone, Default)]
struct Route {
    status: u16,
    body: Vec<u8>,
    headers: Vec<(String, String)>,
    delay_ms: u64,
}

fn ok(body: impl Into<Vec<u8>>) -> Route {
    Route {
        status: 200,
        body: body.into(),
        ..Default::default()
    }
}

#[derive(Clone, Default)]
struct Server {
    routes: Arc<std::sync::Mutex<HashMap<String, Route>>>,
    hits: Arc<std::sync::Mutex<HashMap<String, usize>>>,
    total: Arc<AtomicUsize>,
}

impl Server {
    fn route(&self, path: &str, r: Route) {
        self.routes.lock().unwrap().insert(path.into(), r);
    }
    fn hits(&self, path: &str) -> usize {
        *self.hits.lock().unwrap().get(path).unwrap_or(&0)
    }
    /// Minimal HTTP/1.1 server: GET by path; unknown paths → 404.
    async fn start(&self) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let me = me.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut tmp = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = sock.read(&mut tmp).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&tmp[..n]);
                    }
                    let head = String::from_utf8_lossy(&buf).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                    me.total.fetch_add(1, Ordering::SeqCst);
                    *me.hits.lock().unwrap().entry(path.clone()).or_default() += 1;
                    let route = me.routes.lock().unwrap().get(&path).cloned();
                    let r = route.unwrap_or(Route {
                        status: 404,
                        ..Default::default()
                    });
                    if r.delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(r.delay_ms)).await;
                    }
                    let mut h = format!(
                        "HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n",
                        r.status,
                        r.body.len()
                    );
                    for (k, v) in &r.headers {
                        h.push_str(&format!("{k}: {v}\r\n"));
                    }
                    h.push_str("\r\n");
                    let mut v = h.into_bytes();
                    v.extend_from_slice(&r.body);
                    let _ = sock.write_all(&v).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        format!("http://{addr}")
    }
}

const APP_PATH: &str = "/repos/stardely62-oss/discord-bridge/releases?per_page=100";
const FLOWSEAL_PATH: &str = "/repos/Flowseal/zapret-discord-youtube/releases/latest";
fn manifest_path(v: &str) -> String {
    format!("/m/flowseal-{v}/flowseal-{v}.signed.json")
}

fn key() -> (SigningKey, String) {
    let k = SigningKey::from_bytes(&[9u8; 32]);
    let pk = base64::engine::general_purpose::STANDARD.encode(k.verifying_key().as_bytes());
    (k, pk)
}

async fn center(s: &Server) -> UpdateCenter {
    let base = s.start().await;
    let (_, pk) = key();
    UpdateCenter::new(
        Endpoints {
            api: base.clone(),
            manifest_base: format!("{base}/m"),
        },
        &[&pk],
        APP,
    )
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().build().unwrap()
}

fn releases(tags: &[(&str, bool)]) -> Vec<u8> {
    let v: Vec<_> = tags
        .iter()
        .map(|(t, pre)| serde_json::json!({"tag_name": t, "body": format!("notes {t}"), "draft": false, "prerelease": pre, "html_url": "https://evil.example/x"}))
        .collect();
    serde_json::to_vec(&v).unwrap()
}

fn signed_manifest(version: &str, min_app: &str) -> Vec<u8> {
    let m = BundleManifest {
        format: 1,
        flowseal_version: version.into(),
        winws_version: "v72.9".into(),
        source_url: format!("https://github.com/Flowseal/zapret-discord-youtube/releases/download/{version}/zapret-discord-youtube-{version}.zip"),
        source_sha256: "b".repeat(64),
        archive_root: format!("zapret-discord-youtube-{version}/"),
        min_app_version: min_app.into(),
        files: vec![FileEntry {
            path: "bin/winws.exe".into(),
            sha256: sha256_hex(b"x"),
            size: 1,
        }],
    };
    serde_json::to_vec(&sign(&m, &key().0)).unwrap()
}

// ---------------- Bridge ----------------

#[tokio::test]
async fn a_newer_app_release_is_found_with_its_exact_page() {
    let s = Server::default();
    s.route(
        APP_PATH,
        ok(releases(&[
            ("flowseal-1.10.4", false),
            ("v0.1.0-alpha.11", true),
            ("v0.1.0-alpha.10", true),
        ])),
    );
    let c = center(&s).await;
    let u = c.check_app(&http()).await;
    assert_eq!(u.status, UpdateStatus::Available);
    assert_eq!(u.latest.as_deref(), Some("0.1.0-alpha.11"));
    // Built from the validated tag, not from html_url of the response.
    assert_eq!(
        u.url.as_deref(),
        Some("https://github.com/stardely62-oss/discord-bridge/releases/tag/v0.1.0-alpha.11")
    );
    assert_eq!(c.view().app, Some(u));
}

#[tokio::test]
async fn no_newer_app_release() {
    let s = Server::default();
    s.route(
        APP_PATH,
        ok(releases(&[
            ("v0.1.0-alpha.10", true),
            ("v0.1.0-alpha.9", true),
        ])),
    );
    let u = center(&s).await.check_app(&http()).await;
    assert_eq!(u.status, UpdateStatus::UpToDate);
    assert!(!u.newer);
}

#[tokio::test]
async fn github_failures_are_errors_never_up_to_date() {
    let s = Server::default();
    let c = center(&s).await;
    let cases = [
        (
            Route {
                status: 403,
                headers: vec![
                    ("x-ratelimit-remaining".into(), "0".into()),
                    ("x-ratelimit-reset".into(), (now() + 600).to_string()),
                ],
                ..Default::default()
            },
            "лимит",
        ),
        (
            Route {
                status: 503,
                ..Default::default()
            },
            "временно",
        ),
        (ok("<html>not json</html>"), "непонятный ответ"),
        (
            Route {
                status: 404,
                ..Default::default()
            },
            "404",
        ),
    ];
    for (route, why) in cases {
        s.route(APP_PATH, route);
        let u = c.check_app(&http()).await;
        assert_eq!(u.status, UpdateStatus::Error, "{why}");
        assert!(!u.newer);
        assert!(u.error.as_deref().unwrap().contains(why), "{:?}", u.error);
    }
}

#[tokio::test]
async fn no_network_is_an_error() {
    // A port with nothing listening.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let c = UpdateCenter::new(
        Endpoints {
            api: format!("http://127.0.0.1:{port}"),
            manifest_base: format!("http://127.0.0.1:{port}/m"),
        },
        &[],
        APP,
    );
    let v = c.check_all(&http(), Some("1.10.4".into())).await;
    for u in [v.app.unwrap(), v.flowseal.unwrap()] {
        assert_eq!(u.status, UpdateStatus::Error);
        assert!(!u.newer && !u.installable);
        assert!(u.error.unwrap().contains("соединения"));
    }
}

// ---------------- Flowseal ----------------

fn flowseal_release(tag: &str) -> Route {
    ok(serde_json::to_vec(&serde_json::json!({"tag_name": tag, "body": "changes", "draft": false, "prerelease": false})).unwrap())
}

#[tokio::test]
async fn a_new_flowseal_without_a_signed_manifest_is_not_installable() {
    let s = Server::default();
    s.route(FLOWSEAL_PATH, flowseal_release("1.10.5"));
    let c = center(&s).await;
    let u = c.check_flowseal(&http(), Some("1.10.4".into())).await;
    assert_eq!(u.status, UpdateStatus::Unavailable);
    assert!(u.newer && !u.installable);
    assert_eq!(s.hits(&manifest_path("1.10.5")), 1);
    assert_eq!(
        u.url.as_deref(),
        Some("https://github.com/Flowseal/zapret-discord-youtube/releases/tag/1.10.5")
    );
}

#[tokio::test]
async fn a_new_flowseal_with_a_verified_manifest_is_installable() {
    let s = Server::default();
    s.route(FLOWSEAL_PATH, flowseal_release("1.10.5"));
    s.route(&manifest_path("1.10.5"), ok(signed_manifest("1.10.5", APP)));
    let u = center(&s)
        .await
        .check_flowseal(&http(), Some("1.10.4".into()))
        .await;
    assert_eq!(u.status, UpdateStatus::Available);
    assert!(u.installable);
}

#[tokio::test]
async fn a_manifest_for_a_newer_app_is_not_installable() {
    let s = Server::default();
    s.route(FLOWSEAL_PATH, flowseal_release("1.10.5"));
    s.route(
        &manifest_path("1.10.5"),
        ok(signed_manifest("1.10.5", "0.1.0-alpha.12")),
    );
    let u = center(&s)
        .await
        .check_flowseal(&http(), Some("1.10.4".into()))
        .await;
    assert_eq!(u.status, UpdateStatus::Unavailable);
    assert!(u.unavailable_reason.unwrap().contains("0.1.0-alpha.12"));
}

#[tokio::test]
async fn an_up_to_date_flowseal_does_not_fetch_the_manifest() {
    let s = Server::default();
    s.route(FLOWSEAL_PATH, flowseal_release("1.10.4"));
    let u = center(&s)
        .await
        .check_flowseal(&http(), Some("1.10.4".into()))
        .await;
    assert_eq!(u.status, UpdateStatus::UpToDate);
    assert_eq!(s.hits(&manifest_path("1.10.4")), 0);
}

#[tokio::test]
async fn a_manifest_server_failure_is_an_error() {
    let s = Server::default();
    s.route(FLOWSEAL_PATH, flowseal_release("1.10.5"));
    s.route(
        &manifest_path("1.10.5"),
        Route {
            status: 500,
            ..Default::default()
        },
    );
    let u = center(&s)
        .await
        .check_flowseal(&http(), Some("1.10.4".into()))
        .await;
    assert_eq!(u.status, UpdateStatus::Error);
    assert!(!u.installable);
}

// ---------------- Session behaviour ----------------

#[tokio::test]
async fn concurrent_checks_share_one_request() {
    let s = Server::default();
    s.route(
        APP_PATH,
        Route {
            delay_ms: 300,
            ..ok(releases(&[("v0.1.0-alpha.11", true)]))
        },
    );
    let c = center(&s).await;
    let h = http();
    let (a, b, d) = tokio::join!(c.check_app(&h), c.check_app(&h), c.check_app(&h));
    assert_eq!(s.hits(APP_PATH), 1);
    assert_eq!(a, b);
    assert_eq!(a, d);
    assert!(!c.view().checking_app);
    // A later explicit check asks again.
    c.check_app(&h).await;
    assert_eq!(s.hits(APP_PATH), 2);
}

#[tokio::test]
async fn the_startup_check_runs_once_and_only_when_enabled() {
    let s = Server::default();
    let c = center(&s).await;
    assert!(!c.claim_startup_check(false));
    assert!(c.claim_startup_check(true));
    assert!(!c.claim_startup_check(true));
    // Disabled: nothing was requested and nothing is known.
    let s2 = Server::default();
    let c2 = center(&s2).await;
    assert!(!c2.claim_startup_check(false));
    assert_eq!(s2.total.load(Ordering::SeqCst), 0);
    assert_eq!(c2.view(), UpdatesView::default());
}
