//! Against the real list files and strategies of Flowseal 1.10.4.
use super::*;
use crate::batparse::{parse_bat, BatOptions};
use crate::ports::GameFilter;
use std::path::PathBuf;

fn fixture(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/flowseal-1.10.4")
        .join(rel);
    String::from_utf8(std::fs::read(p).unwrap()).unwrap()
}
fn strategy(file: &str) -> crate::strategy::Strategy {
    parse_bat(
        &fixture(file),
        &BatOptions {
            name: file.into(),
            ..Default::default()
        },
    )
    .unwrap()
    .strategy
}
fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

#[test]
fn every_original_line_is_kept_and_counted_from_the_file() {
    for (name, lines) in [
        ("list-general.txt", 59),
        ("list-google.txt", 22),
        ("list-exclude.txt", 129),
        ("ipset-exclude.txt", 11),
        ("ipset-all.txt", 1),
    ] {
        let t = fixture(&format!("lists/{name}"));
        let raw = raw_lines(&t);
        assert_eq!(raw.len(), lines, "{name}");
        // Re-joining with the original line ending gives the file back.
        let eol = if t.contains("\r\n") { "\r\n" } else { "\n" };
        let mut back = raw.join(eol);
        if t.ends_with('\n') {
            back.push_str(eol);
        }
        assert_eq!(back, t, "{name} round trip");
    }
    let g = parse_hostlist(&fixture("lists/list-general.txt"));
    assert_eq!(g.len(), 59);
    assert_eq!(
        g[0],
        HostEntry {
            line: 1,
            raw: "^dns.google".into(),
            domain: "dns.google".into(),
            exact: true
        }
    );
    assert_eq!(list_kind("list-exclude.txt"), ListKind::HostlistExclude);
    assert_eq!(list_kind("ipset-all.txt.backup"), ListKind::IpsetSource);
    assert_eq!(list_kind("ipset-exclude.txt"), ListKind::IpsetExclude);
}

#[test]
fn exact_entries_do_not_cover_subdomains() {
    assert!(covers("dns.google", true, "dns.google"));
    assert!(!covers("dns.google", true, "x.dns.google"));
    assert!(covers("googlevideo.com", false, "rr1.googlevideo.com"));
    assert!(!covers("googlevideo.com", false, "notgooglevideo.com"));
    assert!(!covers("video.com", false, "googlevideo.com"));
}

#[test]
fn service_domains_are_traced_to_their_lines_and_shared_domains_are_marked() {
    let general = fixture("lists/list-general.txt");
    let google = fixture("lists/list-google.txt");
    let exclude = fixture("lists/list-exclude.txt");
    let hl = [
        ParsedList {
            name: "list-general.txt",
            entries: parse_hostlist(&general),
        },
        ParsedList {
            name: "list-google.txt",
            entries: parse_hostlist(&google),
        },
    ];
    let ex = [ParsedList {
        name: "list-exclude.txt",
        entries: parse_hostlist(&exclude),
    }];
    let youtube = s(&["youtube.com", "googlevideo.com", "ytimg.com"]);
    let googlec = s(&[
        "googlevideo.com",
        "www.gstatic.com",
        "dns.google",
        "rr5.dns.google",
    ]);
    let discord = s(&["discord.com", "stable.dl2.discordapp.net"]);
    let others = [
        ("youtube", youtube.as_slice()),
        ("discord", discord.as_slice()),
    ];
    let info = analyze_domains(&googlec, &hl, &ex, &others);
    let gv = &info[0];
    assert_eq!(
        gv.sources,
        vec![LineRef {
            list: "list-google.txt".into(),
            line: 4,
            raw: "googlevideo.com".into(),
            parent: false
        }]
    );
    assert_eq!(gv.shared_with, vec!["youtube"]);
    // dns.google is listed with ^: it does not cover its subdomains.
    assert_eq!(info[2].sources[0].list, "list-general.txt");
    assert!(
        info[3].sources.is_empty(),
        "rr5.dns.google is not covered by ^dns.google"
    );
    assert!(info.iter().all(|d| d.excluded_by.is_empty()));
    // Discord's update host is in list-google.txt.
    let d = analyze_domains(&discord, &hl, &ex, &[]);
    assert_eq!(d[0].sources[0].list, "list-general.txt");
    // Discord's update host: covered twice — by its parent in list-general.txt
    // and by its own line in list-google.txt.
    let srcs: Vec<(&str, bool)> = d[1]
        .sources
        .iter()
        .map(|r| (r.list.as_str(), r.parent))
        .collect();
    assert_eq!(
        srcs,
        vec![("list-general.txt", true), ("list-google.txt", false)]
    );
    assert_eq!(domain_group("yt3.ggpht.com"), "ggpht.com");
}

#[test]
fn exclusions_are_reported_and_never_count_as_sources() {
    let ex = [ParsedList {
        name: "list-exclude.txt",
        entries: parse_hostlist(&fixture("lists/list-exclude.txt")),
    }];
    let info = analyze_domains(&s(&["store.steampowered.com"]), &[], &ex, &[]);
    assert!(info[0].sources.is_empty());
    assert_eq!(info[0].excluded_by[0].raw, "steampowered.com");
    assert!(info[0].excluded_by[0].parent);
}

#[test]
fn lines_no_card_claims_are_listed() {
    let g = ParsedList {
        name: "list-general.txt",
        entries: parse_hostlist(&fixture("lists/list-general.txt")),
    };
    let discord = s(&["discord.com"]);
    let left = unattributed(&g, &[discord.as_slice()]);
    assert_eq!(left.len(), 58);
    assert!(left.iter().any(|e| e.domain == "cloudfront.net"));
}

#[test]
fn rules_of_the_real_general_strategy() {
    let st = strategy("general.bat");
    let gf = GameFilter::default().ports();
    let f = strategy_facts(&st, &gf);
    assert!(f.wf_udp.as_deref().unwrap().contains("19294-19344"));
    let discord = s(&["discord.com", "discord.media"]);
    let d = card_rules(&f, "discord", &["list-general.txt"], &discord);
    let by: Vec<&RuleMatch> = d.iter().map(|r| &r.matched).collect();
    assert!(by.contains(&&RuleMatch::Hostlist("list-general.txt".into())));
    assert!(by.contains(&&RuleMatch::Protocol(s(&["discord", "stun"]))));
    assert!(by.contains(&&RuleMatch::Domains(s(&["discord.media"]))));
    let voice = f
        .rules
        .iter()
        .find(|r| r.l7.contains(&"discord".to_string()))
        .unwrap();
    assert_eq!(voice.udp.as_deref(), Some("19294-19344,50000-50100"));
    // YouTube: its own list, TCP 443 (and QUIC via list-general rules does not count).
    let y = card_rules(&f, "youtube", &["list-google.txt"], &s(&["youtube.com"]));
    assert!(!y.is_empty());
    assert!(y
        .iter()
        .all(|r| r.matched == RuleMatch::Hostlist("list-google.txt".into())));
    let yr = &f.rules[y[0].index - 1];
    assert_eq!(yr.tcp.as_deref(), Some("443"));
    // The IPSet rules are shared, never attributed to a card.
    let ip = ipset_rules(&f);
    assert!(!ip.is_empty());
    assert!(ip.iter().all(|i| !d.iter().any(|r| r.index == *i)));
    // Exclusions are part of the rule, not targets.
    assert!(f.rules.iter().any(|r| r
        .hostlist_excludes
        .contains(&"list-exclude.txt".to_string())));
}

#[test]
fn a_disabled_rule_does_not_count() {
    let mut st = strategy("general.bat");
    for r in &mut st.rules {
        r.enabled = false;
    }
    let f = strategy_facts(&st, &GameFilter::default().ports());
    assert!(card_rules(&f, "discord", &["list-general.txt"], &s(&["discord.com"])).is_empty());
    assert!(ipset_rules(&f).is_empty());
}

#[test]
fn big_lists_are_paged_and_searched() {
    let mut t = String::new();
    for i in 0..40_000 {
        t.push_str(&format!("10.{}.{}.0/24\r\n", i / 256 % 256, i % 256));
    }
    let p = page("ipset-all.txt.backup", &t, "", 39_990, 50);
    assert_eq!((p.total, p.matched, p.lines.len()), (40_000, 40_000, 10));
    assert_eq!(p.lines[0].0, 39_991);
    let q = page("x", &t, "10.3.7.", 0, 10);
    assert_eq!(q.matched, 1);
    assert_eq!(q.lines[0].1, "10.3.7.0/24");
    assert!(page("x", &t, "", 0, 1_000_000).lines.len() <= MAX_PAGE_LINES);
    assert_eq!(entry_count("# c\r\n\r\na\r\nb\r\n"), 2);
}

// ---------------------------------------------------------------- cards

fn lists_map(names: &[&str]) -> BTreeMap<String, String> {
    names
        .iter()
        .map(|n| (n.to_string(), fixture(&format!("lists/{n}"))))
        .collect()
}

struct World {
    discord: Vec<String>,
    youtube: Vec<String>,
    google: Vec<String>,
    steam: Vec<String>,
    lists: BTreeMap<String, String>,
    st: crate::strategy::Strategy,
    eff: crate::lists::UserLists,
}

fn world() -> World {
    let mut lists = lists_map(&[
        "list-general.txt",
        "list-google.txt",
        "list-exclude.txt",
        "ipset-all.txt",
        "ipset-exclude.txt",
    ]);
    lists.insert(
        IPSET_ALL_BACKUP.into(),
        "1.0.0.0/24\r\n1.1.1.0/24\r\n".into(),
    );
    World {
        discord: s(&["discord.com", "discord.media", "stable.dl2.discordapp.net"]),
        youtube: s(&["youtube.com", "googlevideo.com"]),
        google: s(&["googlevideo.com", "www.gstatic.com", "dns.google"]),
        steam: s(&["steampowered.com", "game.example"]),
        lists,
        st: strategy("general.bat"),
        eff: crate::lists::UserLists::default(),
    }
}

fn ctx<'a>(w: &'a World, running: Option<Running>) -> Context<'a> {
    Context {
        bundle_version: Some("1.10.4"),
        lists: &w.lists,
        selected: Some(&w.st),
        game_filter: GameFilter::default().ports(),
        ipset_mode: crate::ipset::IpsetMode::None,
        effective: &w.eff,
        running,
    }
}

fn cards(w: &World) -> Vec<CardInput<'_>> {
    vec![
        CardInput {
            id: "discord",
            kind: CardKind::Flowseal,
            flowseal_list: Some("list-general.txt"),
            domains: &w.discord,
            ips: &[],
        },
        CardInput {
            id: "youtube",
            kind: CardKind::Flowseal,
            flowseal_list: Some("list-google.txt"),
            domains: &w.youtube,
            ips: &[],
        },
        CardInput {
            id: "google",
            kind: CardKind::Flowseal,
            flowseal_list: Some("list-google.txt"),
            domains: &w.google,
            ips: &[],
        },
        CardInput {
            id: "steam",
            kind: CardKind::Available,
            flowseal_list: None,
            domains: &w.steam,
            ips: &[],
        },
    ]
}

fn running(id: &str, pending: bool) -> Option<Running> {
    Some(Running {
        strategy_id: Some(id.into()),
        bundle_version: Some("1.10.4".into()),
        pending_apply: pending,
    })
}

#[test]
fn apply_state_follows_the_real_engine_and_never_claims_more() {
    let w = world();
    let all = cards(&w);
    let id = w.st.id.clone();
    // Not running: the selected strategy has rules → prepared.
    assert_eq!(
        details(&all[0], &all, &ctx(&w, None)).summary.state,
        ApplyState::Prepared
    );
    // Running the same strategy and version, no pending changes → applied.
    let d = details(&all[0], &all, &ctx(&w, running(&id, false)));
    assert_eq!(d.summary.state, ApplyState::Applied);
    assert_eq!(d.summary.domains, 3);
    assert_eq!(d.summary.in_flowseal, 3);
    assert!(
        d.summary.extra_rules >= 2,
        "voice protocol + discord.media rule"
    );
    // Unapplied changes, another strategy or another Flowseal version → only prepared.
    assert_eq!(
        details(&all[0], &all, &ctx(&w, running(&id, true)))
            .summary
            .state,
        ApplyState::Prepared
    );
    assert_eq!(
        details(&all[0], &all, &ctx(&w, running("other", false)))
            .summary
            .state,
        ApplyState::Prepared
    );
    let mut other_version = running(&id, false).unwrap();
    other_version.bundle_version = Some("1.10.5".into());
    assert_eq!(
        details(&all[0], &all, &ctx(&w, Some(other_version)))
            .summary
            .state,
        ApplyState::Prepared
    );
    // A pack that is not installed is only known.
    assert_eq!(
        details(&all[3], &all, &ctx(&w, running(&id, false)))
            .summary
            .state,
        ApplyState::Known
    );
}

#[test]
fn a_strategy_without_rules_for_the_list_leaves_the_card_known() {
    let mut w = world();
    for r in &mut w.st.rules {
        r.args.retain(
            |a| !matches!(&a.value, Some(ArgValue::List { list }) if list == "list-google.txt"),
        );
    }
    let all = cards(&w);
    let id = w.st.id.clone();
    assert_eq!(
        details(&all[1], &all, &ctx(&w, running(&id, false)))
            .summary
            .state,
        ApplyState::Known
    );
}

#[test]
fn a_missing_flowseal_file_is_reported_not_guessed() {
    let mut w = world();
    w.lists.remove("list-google.txt");
    let all = cards(&w);
    let d = details(&all[1], &all, &ctx(&w, None));
    assert_eq!(d.summary.state, ApplyState::Unknown);
    assert_eq!(d.summary.in_flowseal, 0);
    let g = d
        .sources
        .iter()
        .find(|f| f.name == "list-google.txt")
        .unwrap();
    assert!(!g.present);
}

#[test]
fn shared_domains_and_the_shared_ipset_are_kept_apart_from_the_service() {
    let w = world();
    let all = cards(&w);
    let d = details(&all[2], &all, &ctx(&w, None));
    let gv = d
        .domains
        .iter()
        .find(|x| x.domain == "googlevideo.com")
        .unwrap();
    assert_eq!(gv.shared_with, vec!["youtube"]);
    assert_eq!(d.summary.shared, 1);
    // No IPs of the service itself; the IPSet is described separately.
    assert_eq!(d.summary.ips, 0);
    assert!(d.service_ips.is_empty());
    assert_eq!(d.ipset.source_entries, Some(2));
    assert!(
        d.ipset.runtime_placeholder,
        "mode none: ipset-all.txt is Flowseal's placeholder"
    );
    assert!(!d.ipset.rules.is_empty());
    assert!(d
        .ipset
        .rules
        .iter()
        .all(|i| !d.rules.iter().any(|r| r.index == *i)));
    let mut ctx2 = ctx(&w, None);
    ctx2.ipset_mode = crate::ipset::IpsetMode::Flowseal;
    let d2 = details(&all[2], &all, &ctx2);
    assert_eq!(d2.ipset.runtime_entries, Some(2));
    assert!(!d2.ipset.runtime_placeholder);
}

#[test]
fn installed_packs_use_the_user_list_rules() {
    let w = world();
    let mut all = cards(&w);
    all[3].kind = CardKind::Installed;
    let id = w.st.id.clone();
    let d = details(&all[3], &all, &ctx(&w, running(&id, false)));
    assert!(d
        .rules
        .iter()
        .all(|r| r.matched == RuleMatch::Hostlist(USER_GENERAL.into())));
    assert_eq!(d.summary.state, ApplyState::Applied);
    // steampowered.com is excluded by Flowseal's list-exclude.txt — reported.
    assert_eq!(d.summary.excluded, 1);
    // If every domain is excluded, nothing is bypassed: only "known".
    let only = s(&["steampowered.com"]);
    all[3].domains = &only;
    assert_eq!(
        details(&all[3], &all, &ctx(&w, running(&id, false)))
            .summary
            .state,
        ApplyState::Known
    );
}

#[test]
fn shared_domains_are_written_once_at_runtime() {
    use crate::lists::{effective_lists, InstalledPack, UserLists};
    let p = |id: &str, d: &[&str]| InstalledPack {
        id: id.into(),
        domains: s(d),
        ..Default::default()
    };
    let manual = UserLists {
        general: s(&["my.example", "shared.example"]),
        ..Default::default()
    };
    let e = effective_lists(
        &manual,
        &[
            p("a", &["shared.example", "a.example"]),
            p("b", &["SHARED.example", "a.example", "b.example"]),
        ],
    );
    assert_eq!(
        e.general,
        s(&["my.example", "shared.example", "a.example", "b.example"])
    );
}

#[test]
fn every_list_file_is_listed_with_counts_from_the_file() {
    let w = world();
    let f = source_files(&w.lists);
    let get = |n: &str| f.iter().find(|x| x.name == n).unwrap();
    assert_eq!(
        (
            get("list-general.txt").lines,
            get("list-general.txt").entries
        ),
        (59, 59)
    );
    assert_eq!(get("list-exclude.txt").kind, ListKind::HostlistExclude);
    assert_eq!(get(IPSET_ALL_BACKUP).entries, 2);
}

// ------------------------------------------- regression: real coverage

fn rule_strategy(args: Vec<crate::strategy::Arg>) -> crate::strategy::Strategy {
    let mut st = strategy("general.bat");
    st.id = "only-rule".into();
    st.rules = vec![crate::strategy::Rule {
        label: None,
        enabled: true,
        args,
    }];
    st
}
fn list_arg(opt: &str, list: &str) -> crate::strategy::Arg {
    crate::strategy::Arg {
        opt: opt.into(),
        value: Some(ArgValue::List { list: list.into() }),
    }
}

/// "Rule applied" must not appear when the selected strategy's rule matches
/// the card but handles none of its non-excluded addresses — even though the
/// card has other non-excluded addresses and the engine runs exactly this
/// configuration.
#[test]
fn applied_needs_a_rule_that_really_covers_a_non_excluded_address() {
    let mut w = world();
    // The only rule: list-google.txt on TCP 443, with an exclusion list that
    // removes the card's only list-google.txt domain.
    w.lists.insert(
        "list-exclude-extra.txt".into(),
        "play.google.com\r\n".into(),
    );
    w.st = rule_strategy(vec![
        crate::strategy::Arg::text("--filter-tcp", "443"),
        list_arg("--hostlist", "list-google.txt"),
        list_arg("--hostlist-exclude", "list-exclude-extra.txt"),
        crate::strategy::Arg::text("--dpi-desync", "multisplit"),
    ]);
    // play.google.com is in list-google.txt (excluded by the rule);
    // dns.google is in list-general.txt (no rule uses that list).
    let card_domains = s(&["play.google.com", "dns.google"]);
    let card = CardInput {
        id: "google",
        kind: CardKind::Flowseal,
        flowseal_list: Some("list-google.txt"),
        domains: &card_domains,
        ips: &[],
    };
    let all = [CardInput { ..card }];
    let d = details(&all[0], &all, &ctx(&w, running("only-rule", false)));
    assert_eq!(d.rules.len(), 1, "the rule matches the card's list");
    assert_eq!(d.rules[0].covers, 0, "but handles none of its addresses");
    assert_eq!(d.summary.state, ApplyState::Known);
    assert_ne!(d.summary.state, ApplyState::Applied);
    // Without the exclusion the same rule covers play.google.com → applied.
    w.st.rules[0].args.retain(|a| a.opt != "--hostlist-exclude");
    let d = details(&all[0], &all, &ctx(&w, running("only-rule", false)));
    assert_eq!(d.rules[0].covers, 1);
    assert_eq!(d.summary.state, ApplyState::Applied);
}

#[test]
fn a_rule_for_another_service_s_inline_domain_does_not_apply_to_this_card() {
    let mut w = world();
    w.st = rule_strategy(vec![
        crate::strategy::Arg::text("--filter-tcp", "2053"),
        crate::strategy::Arg::text("--hostlist-domains", "discord.media"),
    ]);
    let all = cards(&w);
    let d = details(&all[1], &all, &ctx(&w, running("only-rule", false)));
    assert!(d.rules.is_empty());
    assert_eq!(d.summary.state, ApplyState::Known);
}

#[test]
fn a_protocol_rule_alone_is_not_an_address_rule() {
    let mut w = world();
    w.st = rule_strategy(vec![
        crate::strategy::Arg::text("--filter-udp", "19294-19344"),
        crate::strategy::Arg::text("--filter-l7", "discord,stun"),
    ]);
    let all = cards(&w);
    let d = details(&all[0], &all, &ctx(&w, running("only-rule", false)));
    assert_eq!(d.rules.len(), 1);
    assert_eq!(d.rules[0].covers, 0);
    assert_eq!(d.summary.state, ApplyState::Known);
}

#[test]
fn exclusions_count_only_for_rules_that_use_them() {
    // general.bat: the list-google.txt rule (TCP 443) has no exclusion lists,
    // so a list-exclude.txt entry does not remove youtube.com from it.
    let mut w = world();
    let mut ex = w.lists["list-exclude.txt"].clone();
    ex.push_str("youtube.com\r\n");
    w.lists.insert("list-exclude.txt".into(), ex);
    let all = cards(&w);
    let id = w.st.id.clone();
    let d = details(&all[1], &all, &ctx(&w, running(&id, false)));
    let r = d
        .rules
        .iter()
        .find(|r| r.matched == RuleMatch::Hostlist("list-google.txt".into()))
        .unwrap();
    assert_eq!(r.covers, 2, "youtube.com and googlevideo.com");
    assert_eq!(d.summary.state, ApplyState::Applied);
}

#[test]
fn rules_of_every_list_that_contains_a_card_domain_are_counted() {
    let w = world();
    let all = cards(&w);
    let id = w.st.id.clone();
    // Google: dns.google is in list-general.txt, the rest in list-google.txt.
    let d = details(&all[2], &all, &ctx(&w, running(&id, false)));
    let lists: Vec<&str> = d
        .rules
        .iter()
        .filter_map(|r| match &r.matched {
            RuleMatch::Hostlist(l) => Some(l.as_str()),
            _ => None,
        })
        .collect();
    assert!(lists.contains(&"list-google.txt") && lists.contains(&"list-general.txt"));
    let general = d
        .rules
        .iter()
        .find(|r| r.matched == RuleMatch::Hostlist("list-general.txt".into()))
        .unwrap();
    assert_eq!(general.covers, 1, "dns.google");
}
