//! Network-side tests against a local HTTP server (no GitHub involved).

use super::*;
use base64::Engine;
use bridge_core::manifest::{sha256_hex, sign_bytes};
use bridge_core::packs::{
    CatalogData, CatalogManifest, FileRef, IconFile, Localized, PackCategory, PackSource,
    Requirements,
};
use ed25519_dalek::SigningKey;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const APP: &str = "0.1.0-alpha.9";

#[derive(Clone)]
struct Route {
    status: u16,
    body: Vec<u8>,
    etag: Option<String>,
}

type Routes = Arc<std::sync::Mutex<HashMap<String, Route>>>;

/// Minimal HTTP/1.1 server: GET by path, ETag / If-None-Match → 304.
async fn server(routes: Routes) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let routes = routes.clone();
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
                let inm = head
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("if-none-match:"))
                    .map(|l| l.split_once(':').unwrap().1.trim().to_string());
                let route = routes.lock().unwrap().get(&path).cloned();
                let resp = match route {
                    Some(r) if r.etag.is_some() && r.etag == inm => b"HTTP/1.1 304 Not Modified\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec(),
                    Some(r) => {
                        let mut h = format!("HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n", r.status, r.body.len());
                        if let Some(e) = &r.etag {
                            h.push_str(&format!("etag: {e}\r\n"));
                        }
                        h.push_str("\r\n");
                        let mut v = h.into_bytes();
                        v.extend_from_slice(&r.body);
                        v
                    }
                    None => b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec(),
                };
                let _ = sock.write_all(&resp).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    format!("http://{addr}")
}

fn key() -> (SigningKey, String) {
    let k = SigningKey::from_bytes(&[7u8; 32]);
    let pk = base64::engine::general_purpose::STANDARD.encode(k.verifying_key().as_bytes());
    (k, pk)
}

fn png() -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend_from_slice(&64u32.to_be_bytes());
    b.extend_from_slice(&64u32.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b
}

fn catalog(serial: u64, steam_version: u32, steam_domains: &[&str]) -> CatalogData {
    let l = |s: &str| Localized {
        ru: s.into(),
        en: s.into(),
    };
    let icon = png();
    CatalogData {
        format: 1,
        serial,
        packs: vec![bridge_core::packs::Pack {
            schema: 1,
            id: "steam".into(),
            version: steam_version,
            updated: "2026-11-06".into(),
            name: l("Steam"),
            description: l("Steam store"),
            category: PackCategory::Launcher,
            source: PackSource::Official,
            icon: Some("steam.png".into()),
            domains: steam_domains.iter().map(|s| s.to_string()).collect(),
            ips: vec![],
            provenance: vec![],
            notes: None,
            partial_coverage: false,
            requires: Requirements::default(),
            flowseal_list: None,
            min_app_version: None,
        }],
        icons: vec![IconFile {
            name: "steam.png".into(),
            sha256: sha256_hex(&icon),
            data: base64::engine::general_purpose::STANDARD.encode(&icon),
        }],
    }
}

fn publish(k: &SigningKey, d: &CatalogData) -> (Vec<u8>, Vec<u8>) {
    let data = serde_json::to_vec(d).unwrap();
    let m = CatalogManifest {
        kind: "discord-bridge-catalog".into(),
        format: 1,
        serial: d.serial,
        published: "2026-11-06T00:00:00Z".into(),
        pack_schema: 1,
        min_app_version: None,
        data: FileRef {
            name: "catalog-data.json".into(),
            sha256: sha256_hex(&data),
            size: data.len() as u64,
        },
        packs: d.packs.len(),
        icons: d.icons.len(),
    };
    (
        serde_json::to_vec(&sign_bytes(&serde_json::to_vec(&m).unwrap(), k)).unwrap(),
        data,
    )
}

fn serve(routes: &Routes, signed: Vec<u8>, data: Vec<u8>, serial: u64) {
    let mut r = routes.lock().unwrap();
    r.insert(
        "/latest/catalog.signed.json".into(),
        Route {
            status: 200,
            body: signed,
            etag: Some(format!("\"m{serial}\"")),
        },
    );
    r.insert(
        format!("/dl/catalog-{serial}/catalog-data.json"),
        Route {
            status: 200,
            body: data,
            etag: None,
        },
    );
}

async fn setup() -> (tempfile::TempDir, Routes, CatalogRuntime, SigningKey) {
    let routes: Routes = Default::default();
    let base = server(routes.clone()).await;
    let (k, pk) = key();
    let dir = tempfile::tempdir().unwrap();
    let rt = CatalogRuntime::new(
        dir.path().join("catalog"),
        Endpoints {
            manifest: format!("{base}/latest/catalog.signed.json"),
            data_base: format!("{base}/dl"),
        },
        &[&pk],
        APP,
    );
    (dir, routes, rt, k)
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn valid_signed_catalog_is_downloaded_cached_and_then_not_modified() {
    let (_d, routes, rt, k) = setup().await;
    assert_eq!(rt.loaded().origin, CatalogOrigin::Builtin);
    let (s, d) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s, d, 100);
    assert_eq!(rt.sync(&http()).await, SyncOutcome::Updated(100));
    let v = rt.view(&[]);
    assert_eq!((v.origin, v.serial), (CatalogOrigin::Remote, Some(100)));
    assert_eq!(v.packs[0].state, PackState::Available);
    assert!(v.icons["steam.png"].starts_with("data:image/png;base64,"));
    // Conditional request: 304.
    assert_eq!(rt.sync(&http()).await, SyncOutcome::Unchanged);
}

#[tokio::test]
async fn unavailable_server_keeps_the_last_verified_catalog() {
    let (_d, routes, rt, k) = setup().await;
    let (s, d) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s, d, 100);
    rt.sync(&http()).await;
    // Point to a closed port.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap();
    let rt2 = CatalogRuntime::new(
        rt.store.dir().to_path_buf(),
        Endpoints {
            manifest: format!("http://{closed}/m.json"),
            data_base: format!("http://{closed}"),
        },
        &[&key().1],
        APP,
    );
    match rt2.sync(&http()).await {
        SyncOutcome::Failed { offline, .. } => assert!(offline),
        other => panic!("{other:?}"),
    }
    let v = rt2.view(&[]);
    assert_eq!(v.serial, Some(100), "cached catalog still used");
    assert!(v.offline && v.error.is_some());
}

#[tokio::test]
async fn bad_signature_or_replaced_data_is_rejected_without_touching_the_cache() {
    let (_d, routes, rt, k) = setup().await;
    let (s, d) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s, d, 100);
    rt.sync(&http()).await;
    // Replaced data for a newer manifest.
    let (s2, _d2) = publish(&k, &catalog(200, 2, &["steampowered.com", "x.example"]));
    let (_, other) = publish(&k, &catalog(200, 2, &["evil.example"]));
    serve(&routes, s2, other, 200);
    assert!(matches!(rt.sync(&http()).await, SyncOutcome::Failed { .. }));
    // Foreign key.
    let foreign = SigningKey::from_bytes(&[9u8; 32]);
    let (s3, d3) = publish(&foreign, &catalog(300, 3, &["steampowered.com"]));
    serve(&routes, s3, d3, 300);
    assert!(matches!(rt.sync(&http()).await, SyncOutcome::Failed { .. }));
    assert_eq!(rt.view(&[]).serial, Some(100));
}

#[tokio::test]
async fn an_older_catalog_does_not_roll_back() {
    let (_d, routes, rt, k) = setup().await;
    let (s, d) = publish(&k, &catalog(200, 2, &["steampowered.com"]));
    serve(&routes, s, d, 200);
    assert_eq!(rt.sync(&http()).await, SyncOutcome::Updated(200));
    let (s1, d1) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s1, d1, 100);
    match rt.sync(&http()).await {
        SyncOutcome::Failed { error, .. } => assert!(error.contains("более старый"), "{error}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(rt.view(&[]).serial, Some(200));
}

#[tokio::test]
async fn oversized_manifest_is_rejected() {
    let (_d, routes, rt, _k) = setup().await;
    routes.lock().unwrap().insert(
        "/latest/catalog.signed.json".into(),
        Route {
            status: 200,
            body: vec![b' '; MAX_MANIFEST_BYTES + 10],
            etag: None,
        },
    );
    assert!(matches!(
        rt.sync(&http()).await,
        SyncOutcome::Failed { offline: false, .. }
    ));
    assert_eq!(rt.loaded().origin, CatalogOrigin::Builtin);
}

#[tokio::test]
async fn not_published_yet_is_an_honest_status() {
    let (_d, _routes, rt, _k) = setup().await;
    match rt.sync(&http()).await {
        SyncOutcome::Failed { error, offline } => {
            assert!(error.contains("не опубликован"));
            assert!(!offline);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn update_shows_the_exact_diff_and_rollback_restores() {
    let (_d, routes, rt, k) = setup().await;
    let (s, d) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s, d, 100);
    rt.sync(&http()).await;
    let installed = packs::install(&[], &rt.pack("steam").unwrap()).unwrap();
    let (s2, d2) = publish(
        &k,
        &catalog(200, 2, &["steampowered.com", "steamcommunity.com"]),
    );
    serve(&routes, s2, d2, 200);
    assert_eq!(rt.sync(&http()).await, SyncOutcome::Updated(200));
    let v = rt.view(&installed);
    assert_eq!(v.packs[0].state, PackState::UpdateAvailable);
    assert_eq!(
        v.packs[0].diff.as_ref().unwrap().added_domains,
        vec!["steamcommunity.com"]
    );
    let updated = packs::install(&installed, &rt.pack("steam").unwrap()).unwrap();
    assert_eq!(rt.view(&updated).packs[0].state, PackState::Current);
    let back = packs::rollback(&updated, "steam").unwrap();
    assert_eq!(back[0].domains, vec!["steampowered.com"]);
}

#[test]
fn builtin_catalog_loads() {
    let dir = tempfile::tempdir().unwrap();
    let rt = CatalogRuntime::production(dir.path().join("c"));
    let v = rt.view(&[]);
    assert_eq!(v.origin, CatalogOrigin::Builtin);
    assert_eq!(v.packs.len(), 10);
    assert_eq!(v.icons.len(), 10);
}

// ---------------------------------- confirmed install vs. catalog changes

fn config(dir: &std::path::Path) -> Mutex<ConfigurationManager> {
    Mutex::new(ConfigurationManager::load(dir.join("cfg")))
}

fn stored(cfg: &Mutex<ConfigurationManager>) -> Vec<InstalledPack> {
    cfg.lock().unwrap().settings.installed_packs.clone()
}

fn steam_confirm(rt: &CatalogRuntime, installed: &[InstalledPack]) -> PackConfirm {
    rt.view(installed)
        .packs
        .into_iter()
        .find(|v| v.pack.id == "steam")
        .unwrap()
        .confirm
}

#[tokio::test]
async fn review_of_catalog_a_is_never_installed_with_data_of_catalog_b() {
    let (d, routes, rt, k) = setup().await;
    let cfg = config(d.path());
    let (s, dd) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s, dd, 100);
    rt.sync(&http()).await;
    // The user looks at A (first install) …
    let seen = steam_confirm(&rt, &[]);
    // … the background sync gets B: same pack version, other addresses.
    let (s2, d2) = publish(&k, &catalog(200, 1, &["steampowered.com", "evil.example"]));
    serve(&routes, s2, d2, 200);
    assert_eq!(rt.sync(&http()).await, SyncOutcome::Updated(200));
    let before = std::fs::read(d.path().join("cfg/settings.json")).ok();
    let err = install_confirmed_in(&cfg, &rt, &seen).unwrap_err();
    assert!(err.contains("изменился после просмотра"), "{err}");
    assert!(stored(&cfg).is_empty(), "nothing installed");
    assert_eq!(
        std::fs::read(d.path().join("cfg/settings.json")).ok(),
        before,
        "settings file untouched"
    );
    // A new review of B installs exactly B.
    let fresh = steam_confirm(&rt, &[]);
    install_confirmed_in(&cfg, &rt, &fresh).unwrap();
    assert_eq!(
        stored(&cfg)[0].domains,
        vec!["steampowered.com", "evil.example"]
    );
}

#[tokio::test]
async fn stale_update_confirmation_keeps_installed_packs_unchanged() {
    let (d, routes, rt, k) = setup().await;
    let cfg = config(d.path());
    let (s, dd) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s, dd, 100);
    rt.sync(&http()).await;
    install_confirmed_in(&cfg, &rt, &steam_confirm(&rt, &[])).unwrap();
    let inst = stored(&cfg);
    // Update to v2 reviewed …
    let (s2, d2) = publish(
        &k,
        &catalog(200, 2, &["steampowered.com", "steamcommunity.com"]),
    );
    serve(&routes, s2, d2, 200);
    rt.sync(&http()).await;
    let reviewed = rt.diff(&inst, "steam").unwrap();
    assert_eq!(reviewed.added_domains, vec!["steamcommunity.com"]);
    // … then v2 is re-published with other addresses (same version).
    let (s3, d3) = publish(&k, &catalog(300, 2, &["steampowered.com", "other.example"]));
    serve(&routes, s3, d3, 300);
    assert_eq!(rt.sync(&http()).await, SyncOutcome::Updated(300));
    assert!(install_confirmed_in(&cfg, &rt, &reviewed.confirm).is_err());
    assert_eq!(stored(&cfg), inst);
    // And a newer version (v3) also needs its own review.
    let (s4, d4) = publish(&k, &catalog(400, 3, &["steampowered.com"]));
    serve(&routes, s4, d4, 400);
    rt.sync(&http()).await;
    let r3 = rt.diff(&inst, "steam").unwrap();
    assert!(install_confirmed_in(&cfg, &rt, &reviewed.confirm).is_err());
    install_confirmed_in(&cfg, &rt, &r3.confirm).unwrap();
    assert_eq!(stored(&cfg)[0].version.as_deref(), Some("3"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn background_sync_racing_the_confirmation_never_mixes_snapshots() {
    for round in 0..12u64 {
        let (d, routes, rt, k) = setup().await;
        let rt = Arc::new(rt);
        let cfg = Arc::new(config(d.path()));
        let (s, dd) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
        serve(&routes, s, dd, 100);
        rt.sync(&http()).await;
        let seen = steam_confirm(&rt, &[]);
        let (s2, d2) = publish(&k, &catalog(200, 1, &["steampowered.com", "b.example"]));
        serve(&routes, s2, d2, 200);
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let (rt2, b2) = (rt.clone(), barrier.clone());
        let sync = tokio::spawn(async move {
            b2.wait().await;
            rt2.sync(&http()).await
        });
        let (rt3, cfg3, b3) = (rt.clone(), cfg.clone(), barrier.clone());
        let install = tokio::spawn(async move {
            b3.wait().await;
            if round % 2 == 0 {
                tokio::task::yield_now().await;
            }
            tokio::task::spawn_blocking(move || install_confirmed_in(&cfg3, &rt3, &seen))
                .await
                .unwrap()
        });
        assert_eq!(sync.await.unwrap(), SyncOutcome::Updated(200));
        match install.await.unwrap() {
            // Installed before B became current: exactly A.
            Ok(()) => assert_eq!(stored(&cfg)[0].domains, vec!["steampowered.com"]),
            // B already current: rejected, nothing stored.
            Err(e) => {
                assert!(e.contains("изменился"), "{e}");
                assert!(stored(&cfg).is_empty());
            }
        }
        assert!(stored(&cfg)
            .iter()
            .all(|i| !i.domains.contains(&"b.example".to_string())));
    }
}

#[tokio::test]
async fn failed_settings_write_leaves_installed_packs_unchanged() {
    let (d, routes, rt, k) = setup().await;
    let (s, dd) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s, dd, 100);
    rt.sync(&http()).await;
    let cfg = config(d.path());
    // The settings file cannot be replaced (a directory is in its place).
    std::fs::create_dir_all(d.path().join("cfg/settings.json/x")).unwrap();
    assert!(install_confirmed_in(&cfg, &rt, &steam_confirm(&rt, &[])).is_err());
    assert!(stored(&cfg).is_empty(), "in-memory settings unchanged");
}

#[test]
fn offline_install_from_the_builtin_catalog_works() {
    let dir = tempfile::tempdir().unwrap();
    let rt = CatalogRuntime::production(dir.path().join("c"));
    let cfg = config(dir.path());
    let v = rt.view(&[]);
    assert_eq!(v.origin, CatalogOrigin::Builtin);
    let pick = v
        .packs
        .iter()
        .find(|p| p.state == PackState::Available)
        .expect("an installable built-in pack");
    assert_eq!(pick.confirm.serial, v.data_serial);
    install_confirmed_in(&cfg, &rt, &pick.confirm).unwrap();
    let got = stored(&cfg);
    assert_eq!(got[0].id, pick.pack.id);
    assert_eq!(got[0].domains, pick.pack.domains);
    assert_eq!(
        rt.view(&got)
            .packs
            .iter()
            .find(|p| p.pack.id == pick.pack.id)
            .unwrap()
            .state,
        PackState::Current
    );
}

#[tokio::test]
async fn catalog_sync_never_changes_settings_or_manual_lists() {
    let (d, routes, rt, k) = setup().await;
    let cfg = config(d.path());
    {
        let mut c = cfg.lock().unwrap();
        let mut s = c.settings.clone();
        s.user_lists.general = vec!["my.example".into()];
        s.user_lists.ipset_include = vec!["9.9.9.0/24".into()];
        c.update(s).unwrap();
    }
    let (s, dd) = publish(&k, &catalog(100, 1, &["steampowered.com"]));
    serve(&routes, s, dd, 100);
    rt.sync(&http()).await;
    install_confirmed_in(&cfg, &rt, &steam_confirm(&rt, &[])).unwrap();
    let before = std::fs::read(d.path().join("cfg/settings.json")).unwrap();
    // A newer catalog with other addresses for the pack: the installed
    // snapshot, manual lists and the rest of the settings stay as they are
    // (no automatic update; the engine is not reachable from CatalogRuntime).
    let (s2, d2) = publish(&k, &catalog(200, 2, &["other.example"]));
    serve(&routes, s2, d2, 200);
    assert_eq!(rt.sync(&http()).await, SyncOutcome::Updated(200));
    assert_eq!(
        std::fs::read(d.path().join("cfg/settings.json")).unwrap(),
        before
    );
    let inst = stored(&cfg);
    assert_eq!(inst[0].domains, vec!["steampowered.com"]);
    assert_eq!(rt.view(&inst).packs[0].state, PackState::UpdateAvailable);
    // A confirmed update and a rollback keep the manual lists.
    let r = rt.diff(&inst, "steam").unwrap();
    install_confirmed_in(&cfg, &rt, &r.confirm).unwrap();
    save_packs_in(&cfg, |cur| {
        packs::rollback(cur, "steam").map_err(|e| e.to_string())
    })
    .unwrap();
    let s = cfg.lock().unwrap().settings.clone();
    assert_eq!(s.installed_packs[0].domains, vec!["steampowered.com"]);
    assert_eq!(s.user_lists.general, vec!["my.example"]);
    assert_eq!(s.user_lists.ipset_include, vec!["9.9.9.0/24"]);
}

/// A pack installed from the built-in snapshot (e.g. Telegram in alpha.10)
/// and later removed from the published catalog stays installed: it is shown
/// as an orphan ("not in the catalog"), keeps its addresses in the engine
/// lists and is removed only by the user.
#[tokio::test]
async fn a_pack_removed_from_the_catalog_stays_installed_as_an_orphan() {
    let (d, routes, rt, k) = setup().await;
    let cfg = config(d.path());
    let v = rt.view(&[]);
    assert_eq!(v.origin, CatalogOrigin::Builtin);
    let tg = v
        .packs
        .iter()
        .find(|p| p.pack.id == "telegram")
        .expect("the alpha.10 built-in snapshot still lists Telegram");
    install_confirmed_in(&cfg, &rt, &tg.confirm).unwrap();
    let serial = v.data_serial + 1000;
    let (s, dd) = publish(&k, &catalog(serial, 1, &["steampowered.com"]));
    serve(&routes, s, dd, serial);
    assert_eq!(rt.sync(&http()).await, SyncOutcome::Updated(serial));
    let inst = stored(&cfg);
    let after = rt.view(&inst);
    assert!(after.packs.iter().all(|p| p.pack.id != "telegram"));
    assert_eq!(after.orphans.len(), 1);
    assert_eq!(after.orphans[0].id, "telegram");
    let lists = bridge_core::lists::effective_lists(&Default::default(), &inst);
    assert!(lists.general.iter().any(|x| x == "telegram.org"));
}

/// Network check of the REAL published catalog with the production
/// endpoints and pinned keys (download, signature, data, cache, ETag/304,
/// anti-rollback against the built-in serial, confirmed install). Needs
/// GitHub: `cargo test -p discord-bridge --lib live_ -- --ignored`; run by
/// a separate CI step / workflow, never as part of the offline suite.
#[tokio::test]
#[ignore = "network: GitHub releases"]
async fn live_published_catalog_syncs_with_production_settings() {
    let dir = tempfile::tempdir().unwrap();
    let rt = CatalogRuntime::production(dir.path().join("c"));
    let builtin = rt.view(&[]).data_serial;
    let http = reqwest::Client::builder()
        .user_agent("discord-bridge-ci")
        .build()
        .unwrap();
    match rt.sync(&http).await {
        SyncOutcome::Updated(s) => assert!(s >= builtin, "{s} < built-in {builtin}"),
        other => panic!("live catalog not accepted: {other:?}"),
    }
    let v = rt.view(&[]);
    assert_eq!(v.origin, CatalogOrigin::Remote);
    assert!(v.error.is_none(), "{:?}", v.error);
    assert!(!v.packs.is_empty());
    for p in &v.packs {
        if let Some(i) = &p.pack.icon {
            assert!(
                v.icons.get(i).is_some_and(|d| d.starts_with("data:image/")),
                "{i}"
            );
        }
    }
    // Second check: conditional request or the same serial → unchanged.
    assert_eq!(rt.sync(&http).await, SyncOutcome::Unchanged);
    // A restart (new runtime over the same cache) keeps the remote catalog.
    let again = CatalogRuntime::production(dir.path().join("c"));
    assert_eq!(again.view(&[]).serial, v.serial);
    // Confirmed install of a real pack, if the catalog has an installable one
    // (packs covered by Flowseal lists are shown but not installable).
    let cfg = config(dir.path());
    if let Some(pick) = v.packs.iter().find(|p| p.state == PackState::Available) {
        install_confirmed_in(&cfg, &again, &pick.confirm).unwrap();
        assert_eq!(stored(&cfg)[0].domains, pick.pack.domains);
    }
    let ids: Vec<&str> = v.packs.iter().map(|p| p.pack.id.as_str()).collect();
    println!("live catalog packs: {ids:?}");
    println!(
        "live catalog {} ({} packs, {} icons) verified",
        v.serial.unwrap(),
        v.packs.len(),
        v.icons.len()
    );
}
