//! The catalog repository's Node scripts (build.mjs + sign.mjs) produce
//! files that bridge_core::packs verifies; the built-in snapshot shipped with
//! the app is the published catalog 1794006227 (four Flowseal service cards).

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

/// The built-in snapshot is the published catalog 1794006227: the four
/// Flowseal service cards, information only (nothing to install, no IPs).
#[test]
fn builtin_catalog_has_the_four_flowseal_cards() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/catalog/builtin-catalog.json");
    let d: CatalogData = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    d.validate(APP).unwrap();
    let mut ids: Vec<&str> = d.packs.iter().map(|p| p.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["cloudflare", "discord", "google", "youtube"]);
    for p in &d.packs {
        assert_eq!(
            bridge_core::packs::pack_state(p, &[]),
            PackState::Flowseal,
            "{}",
            p.id
        );
        assert!(p.ips.is_empty());
    }
    let list = |id: &str| {
        d.packs
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .flowseal_list
            .clone()
            .unwrap()
    };
    assert_eq!(list("discord"), "list-general.txt");
    assert_eq!(list("cloudflare"), "list-general.txt");
    assert_eq!(list("youtube"), "list-google.txt");
    assert_eq!(list("google"), "list-google.txt");
    // Older apps accept it too (they understand the same schema).
    d.validate("0.1.0-alpha.10").unwrap();
}

/// A pack installed in alpha.8 (date version, same addresses) is current.
#[test]
fn a_legacy_installed_pack_with_the_same_addresses_is_current() {
    let d: CatalogData = serde_json::from_slice(&fixture("catalog-data.json")).unwrap();
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
    assert_eq!(v.manifest.serial, 1_794_006_227);
    assert_eq!(v.data.packs.len(), 4);
    assert_eq!(v.data.icons.len(), 4);
    // Every pack yields a stable confirmation hash (same rule as the app).
    for p in &v.data.packs {
        assert_eq!(
            bridge_core::packs::pack_sha256(p),
            bridge_core::packs::pack_sha256(&p.clone())
        );
    }
}
