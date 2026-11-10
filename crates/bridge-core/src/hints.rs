//! Provider hints: strategies that users of an internet provider reported
//! as working, used only to choose the *order* in which Auto tries its
//! candidates.
//!
//! * A hint never adds a candidate, never skips one and never marks anything
//!   as working: Auto still verifies every attempt itself.
//! * The provider is chosen by the user in the settings. Bridge does not
//!   detect it and sends nothing anywhere to learn it.
//! * Nobody decides for the users what works for them. The table
//!   (`resources/hints/providers.json`, shipped with the app) is **generated**
//!   from the users' own reports: the app opens the "works at my provider"
//!   issue form already filled in ([`report_url`]), the user submits it, and
//!   `scripts/hints/aggregate.mjs` counts the reports — a strategy is listed
//!   for a provider once enough different people reported it. A provider
//!   without enough reports has an empty list, and Auto keeps its usual order.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::OnceLock;

pub const HINTS_SCHEMA: u32 = 1;
pub const MAX_PROVIDERS: usize = 500;
pub const MAX_HINTED: usize = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderName {
    pub ru: String,
    pub en: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provider {
    pub id: String,
    pub name: ProviderName,
    /// Strategy ids, the most often reported first.
    #[serde(default)]
    pub strategies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderHints {
    pub schema_version: u32,
    /// Date of the last change of the table, `YYYY-MM-DD`.
    pub updated: String,
    pub providers: Vec<Provider>,
}

/// `a-z`, digits and `-`, 1 to 40 characters: the form of a provider id and
/// of a strategy id.
pub fn valid_id(id: &str) -> bool {
    (1..=40).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

fn valid_name(s: &str) -> bool {
    let n = s.chars().count();
    (1..=60).contains(&n) && !s.chars().any(char::is_control) && s.trim() == s
}

impl ProviderHints {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let h: ProviderHints =
            serde_json::from_slice(bytes).map_err(|e| format!("подсказки провайдеров: {e}"))?;
        h.validate()?;
        Ok(h)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != HINTS_SCHEMA {
            return Err(format!(
                "подсказки провайдеров: неизвестная версия схемы {}",
                self.schema_version
            ));
        }
        if self.providers.len() > MAX_PROVIDERS {
            return Err("подсказки провайдеров: слишком много записей".into());
        }
        let mut seen = HashSet::new();
        for p in &self.providers {
            if !valid_id(&p.id) {
                return Err(format!("подсказки провайдеров: недопустимый id «{}»", p.id));
            }
            if !seen.insert(p.id.as_str()) {
                return Err(format!("подсказки провайдеров: id «{}» повторяется", p.id));
            }
            if !valid_name(&p.name.ru) || !valid_name(&p.name.en) {
                return Err(format!(
                    "подсказки провайдеров: недопустимое название у «{}»",
                    p.id
                ));
            }
            if p.strategies.len() > MAX_HINTED {
                return Err(format!(
                    "подсказки провайдеров: слишком много стратегий у «{}»",
                    p.id
                ));
            }
            let mut s = HashSet::new();
            for id in &p.strategies {
                if !valid_id(id) || !s.insert(id.as_str()) {
                    return Err(format!(
                        "подсказки провайдеров: недопустимая или повторная стратегия «{id}» у «{}»",
                        p.id
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }
}

/// The table shipped with this build. It is checked by a test, so a table
/// that does not parse cannot be released; at run time a bad table would
/// simply mean "no hints".
pub fn builtin() -> &'static ProviderHints {
    static HINTS: OnceLock<ProviderHints> = OnceLock::new();
    HINTS.get_or_init(|| {
        ProviderHints::parse(include_bytes!("../../../resources/hints/providers.json")).unwrap_or(
            ProviderHints {
                schema_version: HINTS_SCHEMA,
                updated: String::new(),
                providers: Vec::new(),
            },
        )
    })
}

/// The issue form of `.github/ISSUE_TEMPLATE/strategy.yml`, filled in with
/// what the app knows: its version, the provider the user chose and the
/// built-in strategy that is running. Nothing is sent by the app: the page
/// opens in the browser and the user reviews and submits it.
pub fn report_url(
    app_version: &str,
    provider: Option<&Provider>,
    strategy: Option<&str>,
) -> String {
    fn enc(s: &str) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    }
    let mut url = format!(
        "https://github.com/{}/issues/new?template=strategy.yml&version={}",
        crate::updates::APP_REPO,
        enc(app_version)
    );
    if let Some(p) = provider {
        url.push_str(&format!("&provider={}", enc(&p.name.ru)));
    }
    if let Some(s) = strategy {
        url.push_str(&format!("&strategy={}", enc(s)));
    }
    url
}

/// Hinted candidates first, in the order of the hints; all the others after
/// them in their original order. The result is a permutation of
/// `candidates`: hints that are not candidates are ignored.
pub fn order_candidates(candidates: Vec<String>, hinted: &[String]) -> Vec<String> {
    let mut first: Vec<String> = Vec::new();
    for h in hinted {
        if candidates.contains(h) && !first.contains(h) {
            first.push(h.clone());
        }
    }
    let rest: Vec<String> = candidates
        .into_iter()
        .filter(|c| !first.contains(c))
        .collect();
    first.extend(rest);
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn hinted_candidates_go_first_and_nothing_is_added_or_lost() {
        let c = ids(&["a", "b", "c", "d"]);
        assert_eq!(
            order_candidates(c.clone(), &ids(&["c", "x", "a", "c"])),
            ids(&["c", "a", "b", "d"])
        );
        assert_eq!(order_candidates(c.clone(), &[]), c);
        assert_eq!(order_candidates(Vec::new(), &ids(&["a"])), ids(&[]));
    }

    #[test]
    fn builtin_table_is_valid_and_not_a_fallback() {
        let raw = include_bytes!("../../../resources/hints/providers.json");
        let parsed = ProviderHints::parse(raw).expect("resources/hints/providers.json");
        assert!(!parsed.providers.is_empty());
        assert_eq!(builtin(), &parsed);
    }

    #[test]
    fn every_hinted_strategy_is_a_built_in_flowseal_strategy() {
        let dir = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/flowseal-1.10.4"
        );
        let files: Vec<(String, String)> = std::fs::read_dir(dir)
            .expect("fixtures")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "bat"))
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    String::from_utf8_lossy(&std::fs::read(e.path()).expect("bat")).into_owned(),
                )
            })
            .collect();
        let known: Vec<String> = crate::catalog::build_catalog("1.10.4", &files)
            .strategies
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(known.len(), 22);
        for p in &builtin().providers {
            for s in &p.strategies {
                assert!(known.contains(s), "{}: unknown strategy {s}", p.id);
            }
        }
    }

    #[test]
    fn bad_tables_are_rejected() {
        let ok = r#"{"schemaVersion":1,"updated":"2026-11-10","providers":[{"id":"a","name":{"ru":"А","en":"A"},"strategies":["flowseal-general"]}]}"#;
        assert!(ProviderHints::parse(ok.as_bytes()).is_ok());
        for bad in [
            ok.replace("\"schemaVersion\":1", "\"schemaVersion\":2"),
            ok.replace("\"id\":\"a\"", "\"id\":\"A b\""),
            ok.replace("\"ru\":\"А\"", "\"ru\":\"\""),
            ok.replace("flowseal-general", "../x"),
            ok.replace("\"strategies\"", "\"url\":\"http://x\",\"strategies\""),
            ok.replace(
                "]}",
                r#",{"id":"a","name":{"ru":"Б","en":"B"},"strategies":[]}]}"#,
            ),
        ] {
            assert!(ProviderHints::parse(bad.as_bytes()).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_report_form_is_filled_in_and_nothing_else_is_reachable() {
        let p = builtin().provider("mts-home").unwrap();
        assert_eq!(
            report_url("0.2.0", Some(p), Some("general (ALT2)")),
            "https://github.com/stardely62-oss/discord-bridge/issues/new?template=strategy.yml&version=0.2.0&provider=%D0%9C%D0%A2%D0%A1%20%28%D0%B4%D0%BE%D0%BC%D0%B0%D1%88%D0%BD%D0%B8%D0%B9%29&strategy=general%20%28ALT2%29"
        );
        assert_eq!(
            report_url("0.2.0", None, None),
            "https://github.com/stardely62-oss/discord-bridge/issues/new?template=strategy.yml&version=0.2.0"
        );
        // Values cannot add parameters or leave the form.
        let u = report_url("1&body=x#y", None, Some("a&title=b/../c"));
        assert_eq!(u.matches('&').count(), 2);
        assert!(!u.contains('#'));
    }

    #[test]
    fn ids_are_strict() {
        assert!(valid_id("mts-home"));
        for bad in ["", "-a", "a-", "A", "a b", "a/b", &"a".repeat(41)] {
            assert!(!valid_id(bad), "{bad}");
        }
    }
}
