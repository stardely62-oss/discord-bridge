//! The catalog repository's Node scripts (build.mjs + sign.mjs) produce
//! files that bridge_core::packs verifies; the built-in snapshot shipped with
//! the app is valid; pack ids and addresses are those of alpha.8.

use bridge_core::packs::{verify_catalog, CatalogData, PackState, CATALOG_TRUSTED_KEYS};
use std::fs;
use std::path::PathBuf;

const APP: &str = "0.1.0-alpha.9";

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/catalog")
            .join(name),
    )
    .unwrap()
}

#[test]
fn node_signed_catalog_verifies_in_rust() {
    let key = String::from_utf8(fixture("test-public-key.b64")).unwrap();
    let v = verify_catalog(
        &fixture("catalog.signed.json"),
        &fixture("catalog-data.json"),
        &[key.trim()],
        APP,
    )
    .unwrap();
    assert_eq!(v.manifest.serial, 1_793_900_000);
    assert_eq!(v.data.packs.len(), 10);
    assert_eq!(v.data.icons.len(), 10);
}

#[test]
fn the_test_key_is_not_trusted_by_the_app() {
    let r = verify_catalog(
        &fixture("catalog.signed.json"),
        &fixture("catalog-data.json"),
        CATALOG_TRUSTED_KEYS,
        APP,
    );
    assert!(r.is_err());
}

#[test]
fn builtin_catalog_is_valid_and_keeps_alpha8_packs() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/catalog/builtin-catalog.json");
    let d: CatalogData = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    d.validate(APP).unwrap();
    let ids: Vec<&str> = d.packs.iter().map(|p| p.id.as_str()).collect();
    for id in [
        "discord",
        "youtube",
        "telegram",
        "steam",
        "riot",
        "valorant",
        "epic",
        "battlenet",
        "roblox",
        "twitch",
    ] {
        assert!(ids.contains(&id), "{id}");
    }
    let tg = d.packs.iter().find(|p| p.id == "telegram").unwrap();
    assert_eq!(tg.ips.len(), 14);
    assert!(tg.ips.contains(&"149.154.160.0/20".to_string()));
    // Flowseal-covered packs keep their special meaning.
    let yt = d.packs.iter().find(|p| p.id == "youtube").unwrap();
    assert_eq!(bridge_core::packs::pack_state(yt, &[]), PackState::Flowseal);
    // A pack installed in alpha.8 (date version, same addresses) is current.
    let steam = d.packs.iter().find(|p| p.id == "steam").unwrap();
    let legacy = bridge_core::lists::InstalledPack {
        id: "steam".into(),
        version: Some("2026-11-05".into()),
        domains: steam.domains.clone(),
        ..Default::default()
    };
    assert_eq!(
        bridge_core::packs::pack_state(steam, &[legacy]),
        PackState::Current
    );
}

/// The built-in snapshot is a real published catalog: its signed manifest
/// (copied from the release) verifies with the key pinned in the app, and
/// the data is byte for byte the published `catalog-data.json`. Offline.
#[test]
fn builtin_snapshot_is_the_published_catalog_signed_with_the_pinned_key() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/catalog");
    let signed = fs::read(dir.join("builtin-catalog.signed.json")).unwrap();
    let data = fs::read(dir.join("builtin-catalog.json")).unwrap();
    let v = verify_catalog(&signed, &data, CATALOG_TRUSTED_KEYS, APP).unwrap();
    assert_eq!(v.data.serial, v.manifest.serial);
    assert_eq!(v.data.packs.len(), 10);
    assert_eq!(v.data.icons.len(), 10);
    // Every pack yields a stable confirmation hash (same rule as the app).
    for p in &v.data.packs {
        assert_eq!(
            bridge_core::packs::pack_sha256(p),
            bridge_core::packs::pack_sha256(&p.clone())
        );
    }
}
