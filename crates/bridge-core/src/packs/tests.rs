use super::*;
use crate::manifest::sign_bytes;
use ed25519_dalek::SigningKey;
use rand_core::OsRng;

const APP: &str = "0.1.0-alpha.9";

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    b.extend_from_slice(&w.to_be_bytes());
    b.extend_from_slice(&h.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b
}

fn icon(name: &str, bytes: &[u8]) -> IconFile {
    IconFile {
        name: name.into(),
        sha256: sha256_hex(bytes),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

fn l(s: &str) -> Localized {
    Localized {
        ru: s.into(),
        en: s.into(),
    }
}

pub(crate) fn pack(id: &str, version: u32, domains: &[&str], ips: &[&str]) -> Pack {
    Pack {
        schema: 1,
        id: id.into(),
        version,
        updated: "2026-11-06".into(),
        name: l(id),
        description: l("description"),
        category: PackCategory::Launcher,
        source: PackSource::Official,
        icon: Some(format!("{id}.png")),
        domains: domains.iter().map(|s| s.to_string()).collect(),
        ips: ips.iter().map(|s| s.to_string()).collect(),
        provenance: if ips.is_empty() {
            vec![]
        } else {
            vec![Provenance {
                kind: ProvenanceKind::Ips,
                url: "https://example.org/cidr.txt".into(),
                note: None,
            }]
        },
        notes: None,
        partial_coverage: false,
        requires: Requirements::default(),
        ports: None,
        flowseal_list: None,
        min_app_version: None,
    }
}

pub(crate) fn data(serial: u64, packs: Vec<Pack>) -> CatalogData {
    let icons = packs
        .iter()
        .map(|p| icon(p.icon.as_deref().unwrap(), &png(64, 64)))
        .collect();
    CatalogData {
        format: 1,
        serial,
        packs,
        icons,
    }
}

pub(crate) fn publish(key: &SigningKey, d: &CatalogData) -> (Vec<u8>, Vec<u8>) {
    let data_bytes = serde_json::to_vec(d).unwrap();
    let m = CatalogManifest {
        kind: CATALOG_KIND.into(),
        format: 1,
        serial: d.serial,
        published: "2026-11-06T00:00:00Z".into(),
        pack_schema: 1,
        min_app_version: None,
        data: FileRef {
            name: "catalog-data.json".into(),
            sha256: sha256_hex(&data_bytes),
            size: data_bytes.len() as u64,
        },
        packs: d.packs.len(),
        icons: d.icons.len(),
    };
    let signed = sign_bytes(&serde_json::to_vec(&m).unwrap(), key);
    (serde_json::to_vec(&signed).unwrap(), data_bytes)
}

pub(crate) fn key() -> (SigningKey, String) {
    let k = SigningKey::generate(&mut OsRng);
    let pk = base64::engine::general_purpose::STANDARD.encode(k.verifying_key().as_bytes());
    (k, pk)
}

#[test]
fn valid_signed_catalog_loads() {
    let (k, pk) = key();
    let d = data(
        100,
        vec![
            pack("steam", 1, &["steampowered.com"], &[]),
            pack("telegram", 2, &["t.me"], &["149.154.160.0/20"]),
        ],
    );
    let (s, b) = publish(&k, &d);
    let v = verify_catalog(&s, &b, &[&pk], APP).unwrap();
    assert_eq!(v.manifest.serial, 100);
    assert_eq!(v.data.packs.len(), 2);
}

#[test]
fn changed_signature_or_unknown_key_is_rejected() {
    let (k, pk) = key();
    let (s, b) = publish(
        &k,
        &data(1, vec![pack("steam", 1, &["steampowered.com"], &[])]),
    );
    let mut env: SignedManifest = serde_json::from_slice(&s).unwrap();
    let mut sig = base64::engine::general_purpose::STANDARD
        .decode(&env.signature)
        .unwrap();
    sig[0] ^= 1;
    env.signature = base64::engine::general_purpose::STANDARD.encode(sig);
    let bad_sig = serde_json::to_vec(&env).unwrap();
    assert!(matches!(
        verify_catalog(&bad_sig, &b, &[&pk], APP),
        Err(CatalogError::Signature(_))
    ));
    // The Flowseal key never verifies a catalog, and vice versa.
    assert!(matches!(
        verify_catalog(&s, &b, crate::manifest::TRUSTED_KEYS, APP),
        Err(CatalogError::Signature(_))
    ));
    let (_, other) = key();
    assert!(matches!(
        verify_catalog(&s, &b, &[&other], APP),
        Err(CatalogError::Signature(_))
    ));
}

#[test]
fn a_document_of_another_kind_is_rejected() {
    let (k, pk) = key();
    let doc = serde_json::json!({"kind": "flowseal-bundle", "format": 1});
    let signed = serde_json::to_vec(&sign_bytes(&serde_json::to_vec(&doc).unwrap(), &k)).unwrap();
    assert!(matches!(
        verify_manifest(&signed, &[&pk], APP),
        Err(CatalogError::Signature(_))
    ));
}

#[test]
fn replaced_pack_or_icon_is_rejected() {
    let (k, pk) = key();
    let d = data(5, vec![pack("steam", 1, &["steampowered.com"], &[])]);
    let (s, _b) = publish(&k, &d);
    // Pack data changed after signing.
    let mut d2 = d.clone();
    d2.packs[0].domains.push("evil.example".into());
    let b2 = serde_json::to_vec(&d2).unwrap();
    assert!(matches!(
        verify_catalog(&s, &b2, &[&pk], APP),
        Err(CatalogError::HashMismatch(_))
    ));
    // Icon bytes replaced (re-signed so only the icon check can catch it).
    let mut d3 = d.clone();
    d3.icons[0].data = base64::engine::general_purpose::STANDARD.encode(png(32, 32));
    let (s3, b3) = publish(&k, &d3);
    assert!(matches!(
        verify_catalog(&s3, &b3, &[&pk], APP),
        Err(CatalogError::BadIcon { .. })
    ));
}

#[test]
fn unsupported_format_is_reported() {
    let (k, pk) = key();
    let doc = serde_json::json!({"kind": CATALOG_KIND, "format": 2});
    let signed = serde_json::to_vec(&sign_bytes(&serde_json::to_vec(&doc).unwrap(), &k)).unwrap();
    assert_eq!(
        verify_manifest(&signed, &[&pk], APP),
        Err(CatalogError::UnsupportedFormat(2))
    );
}

/// Forward compatibility: a newer catalog may add packs of a newer schema or
/// descriptive fields. The catalog still verifies; newer packs are left out,
/// unknown fields are ignored; everything else keeps working.
#[test]
fn newer_pack_schemas_are_left_out_and_unknown_fields_ignored() {
    let (k, pk) = key();
    let mut d = data(
        1,
        vec![
            pack("steam", 1, &["steampowered.com"], &[]),
            pack("future", 1, &["future.example"], &[]),
        ],
    );
    d.packs[1].schema = 2;
    let mut v = serde_json::to_value(&d).unwrap();
    v["packs"][0]["domainGroups"] =
        serde_json::json!([{"name": "store", "domains": ["steampowered.com"]}]);
    v["packs"][1]["somethingNew"] = serde_json::json!({"a": 1});
    let bytes = serde_json::to_vec(&v).unwrap();
    let d2: CatalogData = serde_json::from_slice(&bytes).unwrap();
    let (s, _) = publish(&k, &d2);
    // Publish the exact bytes with the extra fields (the hash covers them).
    let m: SignedManifest = serde_json::from_slice(&s).unwrap();
    let mut man: CatalogManifest =
        serde_json::from_slice(&verify_signed_bytes(&m, &[&pk]).unwrap()).unwrap();
    man.data.sha256 = sha256_hex(&bytes);
    man.data.size = bytes.len() as u64;
    man.pack_schema = 2;
    let signed = serde_json::to_vec(&sign_bytes(&serde_json::to_vec(&man).unwrap(), &k)).unwrap();
    let got = verify_catalog(&signed, &bytes, &[&pk], APP).unwrap();
    let ids: Vec<&str> = got
        .data
        .usable_packs(APP)
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(ids, vec!["steam"]);
    // A schema 0 pack is still an error.
    let mut bad = data(1, vec![pack("steam", 1, &["steampowered.com"], &[])]);
    bad.packs[0].schema = 0;
    let (s, b) = publish(&k, &bad);
    assert!(matches!(
        verify_catalog(&s, &b, &[&pk], APP),
        Err(CatalogError::BadPack { .. })
    ));
}

#[test]
fn older_serial_never_replaces_a_newer_one() {
    assert!(check_not_rollback(10, 10).is_ok());
    assert!(check_not_rollback(11, 10).is_ok());
    assert_eq!(
        check_not_rollback(9, 10),
        Err(CatalogError::Rollback { got: 9, have: 10 })
    );
}

#[test]
fn invalid_entries_are_rejected() {
    let ok = pack("steam", 1, &["steampowered.com"], &[]);
    let cases: Vec<(&str, Pack)> = vec![
        (
            "bad domain",
            Pack {
                domains: vec!["not a domain".into()],
                ..ok.clone()
            },
        ),
        (
            "bad cidr",
            Pack {
                ips: vec!["1.2.3.4/40".into()],
                ..pack("x", 1, &["a.com"], &["1.1.1.0/24"])
            },
        ),
        (
            "too wide",
            Pack {
                ips: vec!["0.0.0.0/0".into()],
                ..pack("x", 1, &["a.com"], &["1.1.1.0/24"])
            },
        ),
        (
            "svg icon",
            Pack {
                icon: Some("steam.svg".into()),
                ..ok.clone()
            },
        ),
        (
            "path icon",
            Pack {
                icon: Some("../steam.png".into()),
                ..ok.clone()
            },
        ),
        (
            "http url",
            Pack {
                provenance: vec![Provenance {
                    kind: ProvenanceKind::Domains,
                    url: "http://x.org".into(),
                    note: None,
                }],
                ..ok.clone()
            },
        ),
        (
            "cred url",
            Pack {
                provenance: vec![Provenance {
                    kind: ProvenanceKind::Domains,
                    url: "https://u:p@x.org/".into(),
                    note: None,
                }],
                ..ok.clone()
            },
        ),
        (
            "ips without source",
            Pack {
                provenance: vec![],
                ..pack("x", 1, &["a.com"], &["1.1.1.0/24"])
            },
        ),
        (
            "bad id",
            Pack {
                id: "Steam!".into(),
                ..ok.clone()
            },
        ),
        (
            "empty",
            Pack {
                domains: vec![],
                ..ok.clone()
            },
        ),
        (
            "dup",
            Pack {
                domains: vec!["a.com".into(), "A.com".into()],
                ..ok.clone()
            },
        ),
        (
            "flowseal list",
            Pack {
                flowseal_list: Some("../x.txt".into()),
                ..ok.clone()
            },
        ),
    ];
    for (what, p) in cases {
        assert!(p.validate().is_err(), "{what}");
    }
    assert!(ok.validate().is_ok());
}

#[test]
fn only_png_and_webp_headers_are_accepted() {
    assert_eq!(image_size(&png(64, 48)).unwrap(), ("image/png", 64, 48));
    assert!(image_size(b"<svg xmlns='http://www.w3.org/2000/svg'/>").is_err());
    assert!(image_size(b"GIF89a....................................").is_err());
    let mut webp = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\0\0\0\0".to_vec();
    webp.extend_from_slice(&[63, 0, 0, 31, 0, 0]);
    assert_eq!(image_size(&webp).unwrap(), ("image/webp", 64, 32));
    // Oversized image is rejected by the catalog check.
    let big = icon("steam.png", &png(4096, 4096));
    let mut d = data(1, vec![pack("steam", 1, &["steampowered.com"], &[])]);
    d.icons = vec![big];
    assert!(matches!(d.validate(APP), Err(CatalogError::BadIcon { .. })));
}

#[test]
fn install_keeps_version_and_addresses() {
    let steam = pack("steam", 3, &["steampowered.com", "steamcommunity.com"], &[]);
    let v = install(&[], &steam).unwrap();
    assert_eq!(v[0].version.as_deref(), Some("3"));
    assert_eq!(v[0].domains, steam.domains);
    assert_eq!(pack_state(&steam, &v), PackState::Current);
    assert_eq!(pack_state(&steam, &[]), PackState::Available);
}

#[test]
fn update_diff_and_rollback() {
    let v1 = pack(
        "telegram",
        1,
        &["t.me", "telegram.org"],
        &["149.154.160.0/20"],
    );
    let v2 = pack(
        "telegram",
        2,
        &["t.me", "telegram.me"],
        &["149.154.160.0/20", "2001:b28:f23d::/48"],
    );
    let inst = install(&[], &v1).unwrap();
    assert_eq!(pack_state(&v2, &inst), PackState::UpdateAvailable);
    let d = pack_diff(7, inst.first(), &v2);
    assert_eq!(d.added_domains, vec!["telegram.me"]);
    assert_eq!(d.removed_domains, vec!["telegram.org"]);
    assert_eq!(d.added_ips, vec!["2001:b28:f23d::/48"]);
    assert!(d.removed_ips.is_empty());
    assert_eq!(
        (d.from_version.as_deref(), d.to_version.as_str()),
        (Some("1"), "2")
    );
    let up = install(&inst, &v2).unwrap();
    assert_eq!(up[0].version.as_deref(), Some("2"));
    assert_eq!(up[0].previous.as_ref().unwrap().domains, v1.domains);
    let back = rollback(&up, "telegram").unwrap();
    assert_eq!(back[0].domains, v1.domains);
    assert_eq!(back[0].ips, v1.ips);
    assert_eq!(back[0].version.as_deref(), Some("1"));
    // The rollback can be undone.
    let again = rollback(&back, "telegram").unwrap();
    assert_eq!(again[0].domains, v2.domains);
    assert!(rollback(&install(&[], &v1).unwrap(), "telegram").is_err());
}

#[test]
fn shared_addresses_and_manual_entries_survive_removal() {
    use crate::lists::{effective_lists, UserLists};
    let riot = pack("riot", 1, &["riotgames.com", "pvp.net"], &["1.1.1.0/24"]);
    let val = pack(
        "valorant",
        1,
        &["playvalorant.com", "riotgames.com"],
        &["1.1.1.0/24"],
    );
    let manual = UserLists {
        general: vec!["pvp.net".into()],
        ipset_include: vec!["9.9.9.0/24".into()],
        ..Default::default()
    };
    let both = install(&install(&[], &riot).unwrap(), &val).unwrap();
    let after = remove(&both, "riot");
    let eff = effective_lists(&manual, &after);
    assert!(
        eff.general.contains(&"riotgames.com".to_string()),
        "shared domain kept"
    );
    assert!(
        eff.general.contains(&"pvp.net".to_string()),
        "manual domain kept"
    );
    assert!(
        eff.ipset_include.contains(&"1.1.1.0/24".to_string()),
        "shared IP kept"
    );
    assert!(
        eff.ipset_include.contains(&"9.9.9.0/24".to_string()),
        "manual IP kept"
    );
    assert_eq!(after.len(), 1);
}

#[test]
fn pre_catalog_snapshots_are_compared_by_content() {
    // alpha.8 stored a date as the version; nothing is changed automatically.
    let steam = pack("steam", 1, &["steampowered.com"], &[]);
    let legacy = InstalledPack {
        id: "steam".into(),
        version: Some("2026-11-05".into()),
        domains: vec!["steampowered.com".into()],
        ..Default::default()
    };
    assert_eq!(
        pack_state(&steam, std::slice::from_ref(&legacy)),
        PackState::Current
    );
    let old = InstalledPack {
        domains: vec!["old.example".into()],
        ..legacy
    };
    assert_eq!(pack_state(&steam, &[old]), PackState::UpdateAvailable);
}

#[test]
fn flowseal_covered_packs_are_never_installed() {
    let mut yt = pack("youtube", 1, &["youtube.com"], &[]);
    yt.flowseal_list = Some("list-google.txt".into());
    assert_eq!(pack_state(&yt, &[]), PackState::Flowseal);
    assert!(install(&[], &yt).is_err());
}

#[test]
fn packs_requiring_a_newer_app_are_hidden() {
    let mut p = pack("future", 1, &["future.example"], &[]);
    p.min_app_version = Some("9.0.0".into());
    let d = data(1, vec![p, pack("steam", 1, &["steampowered.com"], &[])]);
    assert!(d.validate(APP).is_ok());
    assert_eq!(
        d.usable_packs(APP)
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        vec!["steam"]
    );
}

// ------------------------------------------- confirmation of exact snapshots

#[test]
fn pack_sha256_is_deterministic_and_covers_every_field() {
    let a = pack("steam", 1, &["steampowered.com"], &["1.1.1.0/24"]);
    assert_eq!(pack_sha256(&a), pack_sha256(&a.clone()));
    assert_eq!(pack_sha256(&a).len(), 64);
    // Same bytes after a JSON round trip (key order in the source document
    // does not matter: the hash is over the canonical serialization).
    let mut v: serde_json::Value = serde_json::to_value(&a).unwrap();
    let obj = v.as_object_mut().unwrap();
    let reordered: serde_json::Map<_, _> = obj.clone().into_iter().rev().collect();
    *obj = reordered;
    let back: Pack = serde_json::from_value(v).unwrap();
    assert_eq!(pack_sha256(&back), pack_sha256(&a));
    // Addresses, version and metadata each change the hash.
    let mut d = a.clone();
    d.domains.push("steamcommunity.com".into());
    let mut i = a.clone();
    i.ips = vec!["1.1.2.0/24".into()];
    let mut ver = a.clone();
    ver.version = 2;
    let mut meta = a.clone();
    meta.description.ru = "другое".into();
    for other in [d, i, ver, meta] {
        assert_ne!(pack_sha256(&other), pack_sha256(&a));
    }
}

#[test]
fn unchanged_snapshot_installs_exactly_what_was_confirmed() {
    let a = pack("steam", 1, &["steampowered.com"], &[]);
    let c = PackConfirm::of(100, &a, &[]);
    let v = install_confirmed(&[], 100, &a, &c).unwrap();
    assert_eq!(v, install(&[], &a).unwrap());
}

#[test]
fn same_version_different_addresses_is_rejected_as_stale() {
    // User reviewed catalog A; catalog B has the same pack version with other
    // addresses (and, in a separate case, even the same serial).
    let a = pack("steam", 1, &["steampowered.com"], &[]);
    let b = pack("steam", 1, &["steampowered.com", "evil.example"], &[]);
    let c = PackConfirm::of(100, &a, &[]);
    let before = vec![InstalledPack {
        id: "manual-like".into(),
        domains: vec!["x.example".into()],
        ..Default::default()
    }];
    for serial in [100, 200] {
        let err = install_confirmed(&before, serial, &b, &c).unwrap_err();
        assert!(matches!(err, CatalogError::Stale { .. }), "{err}");
    }
    // Nothing returned → the caller has nothing to store.
}

#[test]
fn a_new_version_requires_a_new_review() {
    let v1 = pack("steam", 1, &["steampowered.com"], &[]);
    let v2 = pack("steam", 2, &["steampowered.com", "steamcommunity.com"], &[]);
    let inst = install(&[], &v1).unwrap();
    let seen = pack_diff(100, inst.first(), &v1).confirm;
    let err = install_confirmed(&inst, 200, &v2, &seen).unwrap_err();
    assert!(matches!(err, CatalogError::Stale { ref what, .. } if what == "версия набора"));
    // The fresh diff of v2 carries its own token, which then installs.
    let fresh = pack_diff(200, inst.first(), &v2);
    assert_eq!(fresh.added_domains, vec!["steamcommunity.com"]);
    let up = install_confirmed(&inst, 200, &v2, &fresh.confirm).unwrap();
    assert_eq!(up[0].version.as_deref(), Some("2"));
    assert_eq!(up[0].previous.as_ref().unwrap().domains, v1.domains);
}

#[test]
fn a_changed_installed_copy_requires_a_new_review() {
    // The diff was computed against v1; meanwhile the copy was rolled back /
    // changed elsewhere. The shown diff no longer describes the change.
    let v1 = pack("steam", 1, &["steampowered.com"], &[]);
    let v2 = pack("steam", 2, &["steamcommunity.com"], &[]);
    let inst = install(&[], &v1).unwrap();
    let d = pack_diff(200, inst.first(), &v2);
    let changed = remove(&inst, "steam");
    assert!(matches!(
        install_confirmed(&changed, 200, &v2, &d.confirm),
        Err(CatalogError::Stale { .. })
    ));
    // A first-install token does not apply once the pack got installed.
    let first = PackConfirm::of(200, &v2, &[]);
    assert!(install_confirmed(&inst, 200, &v2, &first).is_err());
}

#[test]
fn confirmed_update_keeps_rollback_and_other_entries() {
    let other = pack("riot", 1, &["riotgames.com"], &["1.1.1.0/24"]);
    let v1 = pack("steam", 1, &["steampowered.com"], &[]);
    let v2 = pack("steam", 2, &["steampowered.com", "steamcommunity.com"], &[]);
    let inst = install(&install(&[], &other).unwrap(), &v1).unwrap();
    let c = pack_diff(5, inst.iter().find(|i| i.id == "steam"), &v2).confirm;
    let up = install_confirmed(&inst, 5, &v2, &c).unwrap();
    assert_eq!(
        up.iter().find(|i| i.id == "riot"),
        inst.iter().find(|i| i.id == "riot")
    );
    let back = rollback(&up, "steam").unwrap();
    assert_eq!(
        back.iter().find(|i| i.id == "steam").unwrap().domains,
        v1.domains
    );
}

#[test]
fn alpha8_snapshots_can_be_updated_with_a_confirmation() {
    // alpha.8 stored a date as the version and no title.
    let legacy = InstalledPack {
        id: "steam".into(),
        version: Some("2026-11-05".into()),
        domains: vec!["steampowered.com".into()],
        ips: vec![],
        title: None,
        previous: None,
    };
    let json = serde_json::to_string(&legacy).unwrap();
    let parsed: InstalledPack = serde_json::from_str(&json).unwrap();
    let p = pack("steam", 2, &["steampowered.com", "steamcommunity.com"], &[]);
    let d = pack_diff(9, Some(&parsed), &p);
    assert_eq!(d.confirm.installed_sha256, Some(installed_sha256(&legacy)));
    let up = install_confirmed(&[parsed], 9, &p, &d.confirm).unwrap();
    assert_eq!(
        up[0].previous.as_ref().unwrap().version.as_deref(),
        Some("2026-11-05")
    );
}

#[test]
fn ports_are_validated_and_older_hashes_do_not_change() {
    let a = pack("photon", 1, &["photonengine.io"], &["216.120.180.0/23"]);
    // A pack without ports serializes exactly as before the field existed.
    let v = serde_json::to_value(&a).unwrap();
    assert!(v.get("ports").is_none());
    let mut ok = a.clone();
    ok.ports = Some(PackPorts {
        tcp: Some("4530,4531,4533".into()),
        udp: Some("5055-5058".into()),
    });
    ok.validate().unwrap();
    let needs = ok.ports.as_ref().unwrap().needs();
    assert_eq!(
        needs
            .iter()
            .map(|n| (n.protocol, n.text, n.ranges.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                "tcp",
                "4530,4531,4533",
                vec![(4530, 4530), (4531, 4531), (4533, 4533)]
            ),
            ("udp", "5055-5058", vec![(5055, 5058)])
        ]
    );
    assert_ne!(pack_sha256(&ok), pack_sha256(&a));
    for bad in [
        PackPorts::default(),
        PackPorts {
            udp: Some("".into()),
            ..Default::default()
        },
        PackPorts {
            udp: Some("0-10".into()),
            ..Default::default()
        },
        PackPorts {
            udp: Some("5058-5055".into()),
            ..Default::default()
        },
        PackPorts {
            udp: Some("70000".into()),
            ..Default::default()
        },
        PackPorts {
            udp: Some("%GameFilterUDP%".into()),
            ..Default::default()
        },
        PackPorts {
            udp: Some("5055, 5056".into()),
            ..Default::default()
        },
        PackPorts {
            tcp: Some("--wf-raw=x".into()),
            ..Default::default()
        },
        PackPorts {
            udp: Some(
                (1..=33)
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            ..Default::default()
        },
    ] {
        let mut p = a.clone();
        p.ports = Some(bad.clone());
        assert!(p.validate().is_err(), "{bad:?}");
    }
    // Unknown keys inside `ports` are an error (like `requires`).
    let mut v = serde_json::to_value(&ok).unwrap();
    v["ports"]["sctp"] = serde_json::json!("1");
    assert!(serde_json::from_value::<Pack>(v).is_err());
}
