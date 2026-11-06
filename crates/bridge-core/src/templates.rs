//! Built-in templates derived from Flowseal strategies (originals untouched).

use crate::strategy::{Arg, ArgValue, Rule, Source, Strategy};

/// Domains Discord uses for the client, API, CDN, media and attachments.
pub const DISCORD_DOMAINS: &[&str] = &[
    "discord.com",
    "discord.gg",
    "discord.media",
    "discordapp.com",
    "discordapp.net",
    "discordcdn.com",
    "discord.co",
    "discord.app",
    "discord.dev",
    "discord.gift",
    "discord.new",
    "discordstatus.com",
    "dis.gd",
    "discord-attachments-uploads-prd.storage.googleapis.com",
];

pub const DISCORD_ONLY_DESCRIPTION: &str = "Сокращённая область действия: из «general» убраны правила для Google, \
полного списка IP (ipset-all) и игрового фильтра, а общий список доменов заменён доменами Discord. \
Ограничение — по доменам и портам, а не по процессу: UDP-правило голоса (порты 19294-19344, 50000-50100, \
протоколы discord/stun) применяется к любому приложению с таким трафиком.";

fn uses_game_filter(r: &Rule) -> bool {
    r.args
        .iter()
        .any(|a| a.text_value().is_some_and(|v| v.contains("%GameFilter")))
}

fn has_list(r: &Rule, opt: &str, name: &str) -> bool {
    r.args
        .iter()
        .any(|a| a.opt == opt && a.value == Some(ArgValue::List { list: name.into() }))
}

/// Build «Discord Only» from Flowseal's `general` strategy.
pub fn discord_only_from(general: &Strategy, id: &str) -> Strategy {
    let domains = DISCORD_DOMAINS.join(",");
    let mut rules: Vec<Rule> = general
        .rules
        .iter()
        .filter(|r| !uses_game_filter(r))
        .filter(|r| !has_list(r, "--hostlist", "list-google.txt"))
        .filter(|r| !has_list(r, "--ipset", "ipset-all.txt"))
        .cloned()
        .collect();
    for r in &mut rules {
        let mut out = Vec::with_capacity(r.args.len());
        let mut domains_added = false;
        for a in r.args.drain(..) {
            let is_general_list = a.opt == "--hostlist"
                && matches!(&a.value, Some(ArgValue::List { list }) if list == "list-general.txt" || list == "list-general-user.txt");
            if is_general_list {
                if !domains_added {
                    out.push(Arg::text("--hostlist-domains", domains.clone()));
                    domains_added = true;
                }
                continue;
            }
            out.push(a);
        }
        r.args = out;
    }
    let global = vec![
        Arg::text("--wf-tcp", "80,443,2053,2083,2087,2096,8443"),
        Arg::text("--wf-udp", "443,19294-19344,50000-50100"),
    ];
    let mut s = general.clone();
    s.id = id.into();
    s.name = "Discord Only".into();
    s.description = DISCORD_ONLY_DESCRIPTION.into();
    s.source = Source::Custom;
    s.origin.base_id = Some(general.id.clone());
    s.requirements.game_filter = false;
    s.global = global;
    s.rules = rules;
    s.notes.clear();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batparse::{parse_bat, BatOptions};
    use crate::catalog::resource_index;
    use crate::strategy::validate::{validate, Severity};

    fn general() -> Strategy {
        let p = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/flowseal-1.10.4/general.bat"
        );
        let text = std::fs::read_to_string(p).unwrap();
        parse_bat(
            &text,
            &BatOptions {
                name: "general".into(),
                id: Some("flowseal-general".into()),
                ..Default::default()
            },
        )
        .unwrap()
        .strategy
    }

    #[test]
    fn discord_only_is_narrower_and_valid() {
        let g = general();
        let d = discord_only_from(&g, "discord-only");
        assert!(d.rules.len() < g.rules.len());
        assert!(d.rules.iter().all(|r| !uses_game_filter(r)));
        assert!(!d
            .rules
            .iter()
            .any(|r| has_list(r, "--hostlist", "list-google.txt")
                || has_list(r, "--ipset", "ipset-all.txt")));
        assert!(!d
            .rules
            .iter()
            .any(|r| has_list(r, "--hostlist", "list-general.txt")));
        // voice rule preserved exactly
        let voice = g
            .rules
            .iter()
            .find(|r| r.args.iter().any(|a| a.opt == "--filter-l7"))
            .unwrap();
        assert!(d.rules.contains(voice));
        let idx = resource_index(
            [
                "ACTIVE_DISCORD_UDP.bin",
                "quic_initial_www_google_com.bin",
                "tls_clienthello_www_google_com.bin",
                "tls_clienthello_4pda_to.bin",
            ],
            ["list-exclude.txt", "ipset-exclude.txt"],
        );
        let rep = validate(&d, Some(&idx));
        let errors: Vec<_> = rep
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(g, general(), "original untouched");
    }
}
