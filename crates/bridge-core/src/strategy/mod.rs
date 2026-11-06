//! Versioned, typed strategy schema.
//!
//! A strategy is an ordered list of global winws options plus an ordered list
//! of rules (winws "profiles", separated by `--new` on the command line).
//! Every argument keeps its original position so that converting a Flowseal
//! BAT file into a strategy and rendering it back yields the same argument
//! order. Values that reference files never contain paths: they name a
//! resource (`{"fake": "..."}` / `{"list": "..."}`) that is resolved by the
//! privileged helper inside its own protected directories.

pub mod migrate;
pub mod options;
pub mod render;
pub mod validate;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Original Flowseal strategy, read-only.
    Flowseal,
    /// Created or edited by the user.
    Custom,
    /// Imported from a BAT/JSON/TOML file.
    Imported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    #[default]
    Winws,
}

/// Where a strategy came from (for copies of Flowseal strategies this enables
/// "compare with original" and "restore original").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flowseal_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ArgValue {
    Text(String),
    Fake { fake: String },
    List { list: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arg {
    /// Full option name including the leading `--`.
    pub opt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ArgValue>,
}

impl Arg {
    pub fn text(opt: &str, v: impl Into<String>) -> Self {
        Arg {
            opt: opt.into(),
            value: Some(ArgValue::Text(v.into())),
        }
    }
    pub fn fake(opt: &str, v: impl Into<String>) -> Self {
        Arg {
            opt: opt.into(),
            value: Some(ArgValue::Fake { fake: v.into() }),
        }
    }
    pub fn list(opt: &str, v: impl Into<String>) -> Self {
        Arg {
            opt: opt.into(),
            value: Some(ArgValue::List { list: v.into() }),
        }
    }
    pub fn flag(opt: &str) -> Self {
        Arg {
            opt: opt.into(),
            value: None,
        }
    }
    pub fn text_value(&self) -> Option<&str> {
        match &self.value {
            Some(ArgValue::Text(t)) => Some(t),
            _ => None,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub args: Vec<Arg>,
}

/// Side effects Flowseal BAT files request through `call service.bat ...`.
/// They are recorded as requirements and never executed blindly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Requirements {
    /// `status_zapret` enables TCP timestamps globally (needed for `--dpi-desync-fooling=ts`).
    #[serde(default)]
    pub tcp_timestamps: bool,
    /// `load_game_filter` defines `%GameFilterTCP%`/`%GameFilterUDP%`.
    #[serde(default)]
    pub game_filter: bool,
    /// `load_user_lists` creates `*-user.txt` lists.
    #[serde(default)]
    pub user_lists: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Strategy {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub source: Source,
    #[serde(default)]
    pub engine: Engine,
    #[serde(default)]
    pub origin: Origin,
    #[serde(default)]
    pub requirements: Requirements,
    /// Options that apply to the whole engine (`--wf-tcp`, `--wf-udp`, ...).
    #[serde(default)]
    pub global: Vec<Arg>,
    pub rules: Vec<Rule>,
    /// Free-form notes, e.g. Flowseal's `:: NOT RECOMMENDED` marker.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl Strategy {
    pub fn new_empty(id: &str, name: &str) -> Self {
        Strategy {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: id.into(),
            name: name.into(),
            description: String::new(),
            source: Source::Custom,
            engine: Engine::Winws,
            origin: Origin::default(),
            requirements: Requirements::default(),
            global: vec![Arg::text("--wf-tcp", "443"), Arg::text("--wf-udp", "443")],
            rules: vec![Rule {
                label: Some("HTTPS".into()),
                enabled: true,
                args: vec![
                    Arg::text("--filter-tcp", "443"),
                    Arg::list("--hostlist", "list-general.txt"),
                    Arg::text("--dpi-desync", "multisplit"),
                    Arg::text("--dpi-desync-split-pos", "1"),
                ],
            }],
            notes: vec![],
            metadata: BTreeMap::new(),
        }
    }

    pub fn is_builtin(&self) -> bool {
        self.source == Source::Flowseal
    }

    /// All resources referenced by the strategy, in order of first appearance.
    pub fn resources(&self) -> Vec<ResourceRef> {
        let mut out: Vec<ResourceRef> = Vec::new();
        let all = self
            .global
            .iter()
            .chain(self.rules.iter().flat_map(|r| r.args.iter()));
        for a in all {
            let r = match &a.value {
                Some(ArgValue::Fake { fake }) => ResourceRef::Fake(fake.clone()),
                Some(ArgValue::List { list }) => ResourceRef::List(list.clone()),
                _ => continue,
            };
            if !out.contains(&r) {
                out.push(r);
            }
        }
        out
    }

    /// Serialize to pretty JSON (export format).
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("strategy is always serializable")
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "lowercase")]
pub enum ResourceRef {
    Fake(String),
    List(String),
}

/// Human-oriented summary of one rule, used by the UI and by the diff view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RuleSummary {
    pub protocol: String,
    pub tcp_ports: Option<String>,
    pub udp_ports: Option<String>,
    pub l7: Option<String>,
    pub lists: Vec<String>,
    pub domains: Vec<String>,
    pub fakes: Vec<String>,
    pub desync: Option<String>,
    pub other: Vec<String>,
}

pub fn summarize_rule(rule: &Rule) -> RuleSummary {
    let mut s = RuleSummary::default();
    for a in &rule.args {
        let val = match &a.value {
            Some(ArgValue::Text(t)) => t.clone(),
            Some(ArgValue::Fake { fake }) => fake.clone(),
            Some(ArgValue::List { list }) => list.clone(),
            None => String::new(),
        };
        match a.opt.as_str() {
            "--filter-tcp" => s.tcp_ports = Some(val),
            "--filter-udp" => s.udp_ports = Some(val),
            "--filter-l7" => s.l7 = Some(val),
            "--hostlist" | "--ipset" => s.lists.push(val),
            "--hostlist-exclude" | "--ipset-exclude" => s.lists.push(format!("−{val}")),
            "--hostlist-domains" => s.domains.push(val),
            "--hostlist-exclude-domains" => s.domains.push(format!("−{val}")),
            "--dpi-desync" => s.desync = Some(val),
            _ => {
                if matches!(a.value, Some(ArgValue::Fake { .. })) {
                    if !s.fakes.contains(&val) {
                        s.fakes.push(val);
                    }
                } else {
                    s.other.push(if val.is_empty() {
                        a.opt.clone()
                    } else {
                        format!("{}={}", a.opt, val)
                    });
                }
            }
        }
    }
    s.protocol = match (s.tcp_ports.is_some(), s.udp_ports.is_some()) {
        (true, true) => "tcp+udp",
        (true, false) => "tcp",
        (false, true) => "udp",
        (false, false) => "any",
    }
    .into();
    s
}

/// Stable hash of everything that determines what winws actually does:
/// the rendered arguments (with resource placeholders, independent of where
/// files are materialised), the Flowseal version and the runtime options
/// (game filter, ipset mode, user lists). Used for rollback snapshots and to
/// invalidate voice confirmations when the effective configuration changes.
pub fn config_hash(
    s: &Strategy,
    runtime: &crate::ipc::RuntimeOptions,
    flowseal_version: &str,
) -> String {
    use sha2::{Digest, Sha256};
    let args = render::render_args(s, &runtime.game_filter.ports(), &render::PreviewResolver)
        .unwrap_or_default();
    let doc = serde_json::json!({
        "args": args,
        "flowseal": flowseal_version,
        "ipset": runtime.ipset_mode,
        "lists": runtime.user_lists,
    });
    hex::encode(Sha256::digest(doc.to_string().as_bytes()))
}

/// Make a URL/file-safe id from a display name.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars().flat_map(|c| c.to_lowercase()) {
        let mapped = translit(c);
        for m in mapped.chars() {
            if m.is_ascii_alphanumeric() {
                out.push(m);
                dash = false;
            } else if !dash && !out.is_empty() {
                out.push('-');
                dash = true;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.truncate(48);
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "strategy".into()
    } else {
        out
    }
}

fn translit(c: char) -> String {
    let s = match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' | 'ё' => "e",
        'ж' => "zh",
        'з' => "z",
        'и' | 'й' => "i",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "h",
        'ц' => "c",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "sch",
        'ы' => "y",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        'ъ' | 'ь' => "",
        _ => return c.to_string(),
    };
    s.to_string()
}

pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !id.starts_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug() {
        assert_eq!(slugify("general (ALT10)"), "general-alt10");
        assert_eq!(slugify("Моя стратегия"), "moya-strategiya");
        assert_eq!(slugify("!!!"), "strategy");
        assert!(is_valid_id(&slugify("general (FAKE TLS AUTO ALT2)")));
    }

    #[test]
    fn json_roundtrip_keeps_value_kinds() {
        let s = Strategy::new_empty("x", "X");
        let j = s.to_json_pretty();
        assert!(j.contains("\"list\": \"list-general.txt\""));
        let back: Strategy = serde_json::from_str(&j).unwrap();
        assert_eq!(back, s);
        let t = s.to_toml().unwrap();
        let back: Strategy = toml::from_str(&t).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn config_hash_tracks_effective_changes_only() {
        let rt = crate::ipc::RuntimeOptions::default();
        let a = Strategy::new_empty("a", "A");
        let mut renamed = a.clone();
        renamed.name = "Другое имя".into();
        renamed.description = "x".into();
        assert_eq!(
            config_hash(&a, &rt, "1.10.4"),
            config_hash(&renamed, &rt, "1.10.4")
        );
        let mut edited = a.clone();
        edited.rules[0].args[2] = Arg::text("--dpi-desync", "fake");
        assert_ne!(
            config_hash(&a, &rt, "1.10.4"),
            config_hash(&edited, &rt, "1.10.4")
        );
        assert_ne!(
            config_hash(&a, &rt, "1.10.4"),
            config_hash(&a, &rt, "1.10.5")
        );
        let mut rt2 = rt.clone();
        rt2.user_lists.general.push("example.org".into());
        assert_ne!(
            config_hash(&a, &rt, "1.10.4"),
            config_hash(&a, &rt2, "1.10.4")
        );
        let mut rt3 = rt.clone();
        rt3.ignore_conflicts = true;
        assert_eq!(
            config_hash(&a, &rt, "1.10.4"),
            config_hash(&a, &rt3, "1.10.4")
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        let bad = r#"{"schemaVersion":1,"id":"a","name":"a","source":"custom","rules":[],"exe":"C:\\evil.exe"}"#;
        assert!(serde_json::from_str::<Strategy>(bad).is_err());
    }
}
