//! Fidelity tests against the real strategy files of Flowseal 1.10.4.
//!
//! The expected argument vector is derived from each BAT with an independent,
//! deliberately naive method (join `^` continuations, split on whitespace,
//! strip quotes, unescape `^!`) and compared — order included — with what the
//! importer + renderer produce.

use bridge_core::batparse::{parse_bat, BatOptions};
use bridge_core::catalog::{build_catalog, resource_index};
use bridge_core::diff::diff;
use bridge_core::ports::{GameFilter, GameFilterMode};
use bridge_core::strategy::migrate::load_json_str;
use bridge_core::strategy::render::{render_args, PreviewResolver};
use bridge_core::strategy::validate::{validate, Severity};
use bridge_core::strategy::{ArgValue, ResourceRef, Source};
use std::fs;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flowseal-1.10.4")
}

fn load_all() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = fs::read_dir(fixture_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".bat"))
        .map(|n| {
            let body = fs::read_to_string(fixture_dir().join(&n)).unwrap();
            (n, body)
        })
        .collect();
    v.sort();
    v
}

fn index() -> bridge_core::strategy::validate::ResourceIndex {
    let txt = fs::read_to_string(fixture_dir().join("resources.txt")).unwrap();
    let bin: Vec<&str> = txt.lines().filter_map(|l| l.strip_prefix("bin/")).collect();
    let lists: Vec<&str> = txt
        .lines()
        .filter_map(|l| l.strip_prefix("lists/"))
        .collect();
    resource_index(bin, lists)
}

fn naive_args(bat: &str, gf_tcp: &str, gf_udp: &str) -> Vec<String> {
    let text = bat.replace('\r', "").replace(" ^\n", " ");
    let line = text
        .lines()
        .find(|l| l.starts_with("start "))
        .expect("start line");
    let after = line.split("winws.exe\"").nth(1).expect("exe");
    after
        .split_whitespace()
        .map(|t| {
            t.replace('"', "")
                .replace("^!", "!")
                .replace("%GameFilterTCP%", gf_tcp)
                .replace("%GameFilterUDP%", gf_udp)
        })
        .collect()
}

#[test]
fn all_22_strategies_import_without_unsupported_lines() {
    let files = load_all();
    assert_eq!(files.len(), 22, "Flowseal 1.10.4 ships 22 strategies");
    for (name, body) in &files {
        let r = parse_bat(
            body,
            &BatOptions {
                name: name.clone(),
                ..Default::default()
            },
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(r.unsupported.is_empty(), "{name}: {:?}", r.unsupported);
        assert!(
            r.strategy.requirements.tcp_timestamps,
            "{name}: status_zapret → timestamps"
        );
        assert!(r.strategy.requirements.game_filter);
        assert!(r.strategy.requirements.user_lists);
    }
}

#[test]
fn render_matches_original_argument_order_exactly() {
    for (name, body) in load_all() {
        let imp = parse_bat(
            &body,
            &BatOptions {
                name: name.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        for gf in [
            GameFilter::default(),
            GameFilter {
                mode: GameFilterMode::All,
                tcp_range: "1024-65535".into(),
                udp_range: "1024-65535".into(),
            },
            GameFilter {
                mode: GameFilterMode::Udp,
                tcp_range: "1024-65535".into(),
                udp_range: "2000-3000".into(),
            },
        ] {
            let ports = gf.ports();
            let ours = render_args(&imp.strategy, &ports, &PreviewResolver).unwrap();
            let expected = naive_args(&body, &ports.tcp, &ports.udp);
            assert_eq!(ours, expected, "{name} with {:?}", gf.mode);
        }
    }
}

#[test]
fn builtins_validate_against_real_resources() {
    let idx = index();
    for (name, body) in load_all() {
        let mut imp = parse_bat(
            &body,
            &BatOptions {
                name: name.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        imp.strategy.id = "x".into();
        let rep = validate(&imp.strategy, Some(&idx));
        let errors: Vec<_> = rep
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{name}: {errors:?}");
        for r in imp.strategy.resources() {
            match r {
                ResourceRef::Fake(f) => assert!(idx.fakes.contains(&f), "{name}: {f}"),
                ResourceRef::List(l) => assert!(idx.lists.contains(&l), "{name}: {l}"),
            }
        }
    }
}

#[test]
fn catalog_uses_real_names_in_flowseal_order() {
    let cat = build_catalog("1.10.4", &load_all());
    assert!(cat.errors.is_empty(), "{:?}", cat.errors);
    assert_eq!(cat.strategies.len(), 22);
    let names: Vec<&str> = cat.strategies.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names[0], "general (ALT)");
    let alt2 = names.iter().position(|n| *n == "general (ALT2)").unwrap();
    let alt10 = names.iter().position(|n| *n == "general (ALT10)").unwrap();
    assert!(alt2 < alt10, "natural order");
    assert!(names.contains(&"general"));
    assert!(names.contains(&"general (FAKE TLS AUTO ALT3)"));
    assert!(cat.strategies.iter().all(|s| s.source == Source::Flowseal));
    let ids: std::collections::HashSet<_> = cat.strategies.iter().map(|s| s.id.clone()).collect();
    assert_eq!(ids.len(), 22, "unique ids");
    // ALT5 has fewer rules and EXP/ALT5 use extra options
    let alt5 = cat
        .strategies
        .iter()
        .find(|s| s.name == "general (ALT5)")
        .unwrap();
    assert!(alt5
        .rules
        .iter()
        .any(|r| r.args.iter().any(|a| a.opt == "--filter-l3")));
}

#[test]
fn discord_voice_rule_is_preserved() {
    let cat = build_catalog("1.10.4", &load_all());
    let g = cat.strategies.iter().find(|s| s.name == "general").unwrap();
    let voice = &g.rules[1];
    assert_eq!(voice.args[0].text_value(), Some("19294-19344,50000-50100"));
    assert_eq!(voice.args[1].text_value(), Some("discord,stun"));
    assert!(voice
        .args
        .iter()
        .any(|a| a.opt == "--dpi-desync-fake-discord"
            && a.value
                == Some(ArgValue::Fake {
                    fake: "ACTIVE_DISCORD_UDP.bin".into()
                })));
}

#[test]
fn json_export_reimport_roundtrip_for_every_builtin() {
    let cat = build_catalog("1.10.4", &load_all());
    let gf = GameFilter::default().ports();
    for s in &cat.strategies {
        let json = s.to_json_pretty();
        let back = load_json_str(&json).unwrap().strategy;
        assert_eq!(&back, s);
        assert!(diff(s, &back, &gf).identical);
        let toml_text = s.to_toml().unwrap();
        let back: bridge_core::Strategy = toml::from_str(&toml_text).unwrap();
        assert_eq!(&back, s, "toml {}", s.name);
    }
}

#[test]
fn service_bat_is_rejected() {
    let body = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/service-1.10.4.bat.txt"),
    )
    .unwrap();
    assert!(parse_bat(&body, &BatOptions::default()).is_err());
}

#[test]
fn diff_between_two_real_strategies() {
    let cat = build_catalog("1.10.4", &load_all());
    let a = cat.strategies.iter().find(|s| s.name == "general").unwrap();
    let b = cat
        .strategies
        .iter()
        .find(|s| s.name == "general (ALT)")
        .unwrap();
    let d = diff(a, b, &GameFilter::default().ports());
    assert!(!d.identical);
    assert!(d
        .rules
        .iter()
        .any(|r| r.changes.iter().any(|c| c.field == "desync")));
}

/// Every built-in strategy matches IPs against `ipset-all.txt` and pairs each
/// such rule with the user's IP exclusions, so winws itself also enforces
/// them (on top of the exclusions already cut out of `ipset-all.txt`).
#[test]
fn builtin_ipset_rules_always_skip_user_ip_exclusions() {
    use bridge_core::ipset::strategy_ip_use;
    for (name, body) in load_all() {
        let s = parse_bat(
            &body,
            &BatOptions {
                name: name.clone(),
                ..Default::default()
            },
        )
        .unwrap()
        .strategy;
        let u = strategy_ip_use(&s);
        assert!(u.ipset_rules >= 1, "{name}: no ipset-all rule");
        assert_eq!(
            u.ipset_rules, u.ipset_rules_with_user_exclude,
            "{name}: an ipset rule without ipset-exclude-user.txt"
        );
    }
}
