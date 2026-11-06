//! Semantic validation of strategies. Runs in the UI process for feedback and
//! again in the privileged helper before anything is started.

use super::options::{self, Kind, OptionSpec, Scope};
use super::{is_valid_id, Arg, ArgValue, Strategy, CURRENT_SCHEMA_VERSION};
use crate::is_safe_resource_name;
use crate::ports::{PortItem, PortSpec};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

pub const MAX_RULES: usize = 64;
pub const MAX_ARGS_TOTAL: usize = 1024;
pub const MAX_TEXT_VALUE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: Severity,
    /// JSON-pointer-like location: `rules[2].args[5]`, `global[0]`, `name`.
    pub path: String,
    pub message: String,
}

/// Resources available for validation. `None` skips existence checks.
#[derive(Debug, Clone, Default)]
pub struct ResourceIndex {
    pub fakes: BTreeSet<String>,
    pub lists: BTreeSet<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Error)
    }
    fn err(&mut self, path: impl Into<String>, msg: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Error,
            path: path.into(),
            message: msg.into(),
        });
    }
    fn warn(&mut self, path: impl Into<String>, msg: impl Into<String>) {
        self.issues.push(Issue {
            severity: Severity::Warning,
            path: path.into(),
            message: msg.into(),
        });
    }
    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|i| i.severity == Severity::Error)
    }
}

pub fn validate(s: &Strategy, resources: Option<&ResourceIndex>) -> Report {
    let mut r = Report::default();
    let specs: HashMap<&'static str, OptionSpec> = options::option_specs()
        .into_iter()
        .map(|o| (o.name, o))
        .collect();

    if s.schema_version != CURRENT_SCHEMA_VERSION {
        r.err(
            "schemaVersion",
            format!("неподдерживаемая версия схемы {}", s.schema_version),
        );
    }
    if !is_valid_id(&s.id) {
        r.err(
            "id",
            "идентификатор должен состоять из a-z, 0-9 и «-» (до 64 символов)",
        );
    }
    let name = s.name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(|c| c.is_control()) {
        r.err(
            "name",
            "название обязательно, до 100 символов, без управляющих символов",
        );
    }
    if s.description.chars().count() > 2000 {
        r.err("description", "описание длиннее 2000 символов");
    }
    if s.rules.is_empty() {
        r.err("rules", "нужно хотя бы одно правило");
    }
    if s.rules.len() > MAX_RULES {
        r.err(
            "rules",
            format!("слишком много правил (максимум {MAX_RULES})"),
        );
    }
    let total: usize = s.global.len() + s.rules.iter().map(|x| x.args.len()).sum::<usize>();
    if total > MAX_ARGS_TOTAL {
        r.err(
            "rules",
            format!("слишком много параметров (максимум {MAX_ARGS_TOTAL})"),
        );
    }
    if !s.rules.iter().any(|x| x.enabled) && !s.rules.is_empty() {
        r.err("rules", "все правила выключены — движку нечего делать");
    }

    // Global options
    let mut seen_global: HashMap<&str, usize> = HashMap::new();
    for (i, a) in s.global.iter().enumerate() {
        let path = format!("global[{i}]");
        match specs.get(a.opt.as_str()) {
            None => r.err(
                &path,
                format!("неизвестный или неподдерживаемый параметр {}", a.opt),
            ),
            Some(spec) if spec.scope != Scope::Global => r.err(
                &path,
                format!(
                    "{} относится к правилу, а не к глобальным параметрам",
                    a.opt
                ),
            ),
            Some(spec) => {
                check_value(&mut r, &path, spec, a, resources);
                if !spec.repeatable {
                    if let Some(prev) = seen_global.insert(spec.name, i) {
                        r.err(&path, format!("{} уже задан в global[{prev}]", a.opt));
                    }
                }
            }
        }
    }
    let wf_tcp = s
        .global
        .iter()
        .find(|a| a.opt == "--wf-tcp")
        .and_then(|a| a.text_value());
    let wf_udp = s
        .global
        .iter()
        .find(|a| a.opt == "--wf-udp")
        .and_then(|a| a.text_value());
    if wf_tcp.is_none() && wf_udp.is_none() {
        r.err("global", "не задан перехват: нужен --wf-tcp и/или --wf-udp");
    }
    let wf_tcp = wf_tcp.and_then(|v| PortSpec::parse(v).ok());
    let wf_udp = wf_udp.and_then(|v| PortSpec::parse(v).ok());

    for (ri, rule) in s.rules.iter().enumerate() {
        let rpath = format!("rules[{ri}]");
        if let Some(l) = &rule.label {
            if l.chars().count() > 100 || l.chars().any(|c| c.is_control()) {
                r.err(
                    format!("{rpath}.label"),
                    "метка правила до 100 символов без управляющих символов",
                );
            }
        }
        if rule.args.is_empty() {
            r.err(&rpath, "пустое правило");
            continue;
        }
        let mut seen: HashMap<&str, usize> = HashMap::new();
        let mut has_filter = false;
        let mut has_desync = false;
        for (ai, a) in rule.args.iter().enumerate() {
            let path = format!("{rpath}.args[{ai}]");
            match specs.get(a.opt.as_str()) {
                None => r.err(
                    &path,
                    format!("неизвестный или неподдерживаемый параметр {}", a.opt),
                ),
                Some(spec) if spec.scope != Scope::Rule => r.err(
                    &path,
                    format!(
                        "{} — глобальный параметр, его нужно указать в разделе «Перехват»",
                        a.opt
                    ),
                ),
                Some(spec) => {
                    check_value(&mut r, &path, spec, a, resources);
                    if !spec.repeatable {
                        if let Some(prev) = seen.insert(spec.name, ai) {
                            r.err(
                                &path,
                                format!("{} уже задан в этом правиле (args[{prev}])", a.opt),
                            );
                        }
                    }
                    match spec.name {
                        "--filter-tcp" => {
                            has_filter = true;
                            if rule.enabled {
                                coverage(&mut r, &path, a, wf_tcp.as_ref(), "--wf-tcp");
                            }
                        }
                        "--filter-udp" => {
                            has_filter = true;
                            if rule.enabled {
                                coverage(&mut r, &path, a, wf_udp.as_ref(), "--wf-udp");
                            }
                        }
                        "--dpi-desync" => has_desync = true,
                        "--dpi-desync-fooling"
                            if a.text_value()
                                .is_some_and(|v| v.split(',').any(|x| x == "ts"))
                                && !s.requirements.tcp_timestamps =>
                        {
                            r.warn(&path, "fooling=ts работает только при включённых TCP timestamps в Windows");
                        }
                        _ => {}
                    }
                }
            }
        }
        if !has_filter {
            r.warn(
                &rpath,
                "нет --filter-tcp/--filter-udp: правило применится ко всему перехваченному трафику",
            );
        }
        if !has_desync {
            r.warn(&rpath, "нет --dpi-desync: правило ничего не меняет");
        }
    }
    r
}

/// Checks applied when a strategy is *saved*. Drafts with semantic errors
/// (unknown options, wrong values, missing resources) can be saved — they
/// just cannot be started — but data that is unsafe or unbounded cannot be
/// stored at all: oversized documents, control characters, option names
/// that are not plain `--name`, resource names with path components.
pub fn validate_storable(s: &Strategy) -> Result<(), String> {
    if !is_valid_id(&s.id) {
        return Err("недопустимый идентификатор стратегии".into());
    }
    let name = s.name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err("название обязательно: до 100 символов, без управляющих символов".into());
    }
    if s.description.chars().count() > 2000 {
        return Err("описание длиннее 2000 символов".into());
    }
    if s.rules.len() > MAX_RULES {
        return Err(format!("слишком много правил (максимум {MAX_RULES})"));
    }
    let total: usize = s.global.len() + s.rules.iter().map(|r| r.args.len()).sum::<usize>();
    if total > MAX_ARGS_TOTAL {
        return Err(format!(
            "слишком много параметров (максимум {MAX_ARGS_TOTAL})"
        ));
    }
    for r in &s.rules {
        if let Some(l) = &r.label {
            if l.chars().count() > 100 || l.chars().any(char::is_control) {
                return Err("метка правила: до 100 символов без управляющих символов".into());
            }
        }
    }
    for a in s
        .global
        .iter()
        .chain(s.rules.iter().flat_map(|r| r.args.iter()))
    {
        let ok_name = a.opt.len() <= 64
            && a.opt.starts_with("--")
            && a.opt.len() > 2
            && a.opt[2..]
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !ok_name {
            return Err(format!(
                "недопустимое имя параметра «{}»",
                a.opt.chars().take(40).collect::<String>()
            ));
        }
        match &a.value {
            Some(ArgValue::Fake { fake: n }) | Some(ArgValue::List { list: n })
                if !is_safe_resource_name(n) =>
            {
                return Err(format!(
                    "недопустимое имя ресурса «{}»",
                    n.chars().take(40).collect::<String>()
                ));
            }
            Some(ArgValue::Text(t))
                if t.len() > MAX_TEXT_VALUE || t.chars().any(char::is_control) =>
            {
                return Err(format!("недопустимое значение параметра {}", a.opt));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Warn when a rule filters ports that the global capture does not include.
fn coverage(r: &mut Report, path: &str, a: &Arg, wf: Option<&PortSpec>, wf_name: &str) {
    let Some(v) = a.text_value() else { return };
    let Ok(spec) = PortSpec::parse(v) else { return };
    let Some(wf) = wf else {
        r.warn(
            path,
            format!("правило фильтрует порты, но {wf_name} не задан — трафик не будет перехвачен"),
        );
        return;
    };
    let covered = |item: &PortItem| -> bool {
        wf.0.iter().any(|w| match (w, item) {
            (PortItem::Var(a), PortItem::Var(b)) => a == b,
            (PortItem::Single(a), PortItem::Single(b)) => a == b,
            (PortItem::Range(lo, hi), PortItem::Single(b)) => lo <= b && b <= hi,
            (PortItem::Range(lo, hi), PortItem::Range(a, b)) => lo <= a && b <= hi,
            (PortItem::Single(a), PortItem::Range(lo, hi)) => lo == hi && a == lo,
            _ => false,
        })
    };
    let missing: Vec<String> = spec
        .0
        .iter()
        .filter(|i| !covered(i))
        .map(|i| PortSpec(vec![i.clone()]).to_string())
        .collect();
    if !missing.is_empty() {
        r.warn(
            path,
            format!(
                "порты {} не входят в {wf_name} и не будут перехвачены",
                missing.join(",")
            ),
        );
    }
}

fn check_value(
    r: &mut Report,
    path: &str,
    spec: &OptionSpec,
    a: &Arg,
    res: Option<&ResourceIndex>,
) {
    let text = match (&spec.kind, &a.value) {
        (Kind::Flag, None) => return,
        (Kind::Flag, Some(_)) => return r.err(path, format!("{} не принимает значение", a.opt)),
        (_, None) => return r.err(path, format!("{} требует значение", a.opt)),
        (Kind::Fake { .. }, Some(ArgValue::Fake { fake })) => {
            if !is_safe_resource_name(fake) {
                return r.err(path, format!("недопустимое имя файла-фейка «{fake}»"));
            }
            if let Some(idx) = res {
                if !idx.fakes.contains(fake) {
                    r.err(path, format!("файл-фейк «{fake}» не найден среди ресурсов"));
                }
            }
            return;
        }
        (Kind::List, Some(ArgValue::List { list })) => {
            if !is_safe_resource_name(list) {
                return r.err(path, format!("недопустимое имя списка «{list}»"));
            }
            if let Some(idx) = res {
                if !idx.lists.contains(list) {
                    r.err(path, format!("список «{list}» не найден среди ресурсов"));
                }
            }
            return;
        }
        (Kind::List, Some(_)) => {
            return r.err(
                path,
                format!(
                    "{} принимает только ссылку на список ({{\"list\": \"...\"}})",
                    a.opt
                ),
            )
        }
        (_, Some(ArgValue::Fake { .. } | ArgValue::List { .. })) => {
            return r.err(path, format!("{} не принимает ссылку на файл", a.opt))
        }
        (_, Some(ArgValue::Text(t))) => t.as_str(),
    };
    if text.len() > MAX_TEXT_VALUE {
        return r.err(path, "слишком длинное значение");
    }
    // `%` is only meaningful inside port specs (game filter variables), which
    // PortSpec::parse checks strictly.
    let pct_ok = matches!(spec.kind, Kind::Ports);
    if text.chars().any(|c| {
        c.is_control()
            || c == '"'
            || c == '\''
            || (c == '%' && !pct_ok)
            || c == '^'
            || c == '&'
            || c == '|'
            || c == '<'
            || c == '>'
            || c == '`'
            || c == ' '
    }) {
        return r.err(path, format!("недопустимые символы в значении {}", a.opt));
    }
    if let Err(m) = check_text(&spec.kind, text) {
        r.err(path, format!("{}: {m}", a.opt));
    }
}

fn is_hex_literal(v: &str) -> bool {
    v.len() > 2
        && v.len() <= 2 + 2 * 1500
        && (v.starts_with("0x") || v.starts_with("0X"))
        && v[2..].bytes().all(|b| b.is_ascii_hexdigit())
        && (v.len() - 2).is_multiple_of(2)
}

fn is_domain(d: &str) -> bool {
    !d.is_empty()
        && d.len() <= 253
        && d.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

fn is_cidr(v: &str) -> bool {
    let (ip, mask) = match v.split_once('/') {
        Some((a, m)) => (a, Some(m)),
        None => (v, None),
    };
    let max = if ip.parse::<std::net::Ipv4Addr>().is_ok() {
        32
    } else if ip.parse::<std::net::Ipv6Addr>().is_ok() {
        128
    } else {
        return false;
    };
    match mask {
        None => true,
        Some(m) => m.parse::<u8>().map(|m| m <= max).unwrap_or(false),
    }
}

fn is_marker(m: &str) -> bool {
    const NAMED: &[&str] = &[
        "method", "host", "endhost", "sld", "midsld", "endsld", "sniext", "abs",
    ];
    let int = |s: &str| !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit());
    let m2 = m.strip_prefix('-').unwrap_or(m);
    if int(m2) {
        return true;
    }
    let (name, off) = match m.find(['+', '-']) {
        Some(i) => (&m[..i], Some(&m[i + 1..])),
        None => (m, None),
    };
    NAMED.contains(&name) && off.is_none_or(int)
}

pub fn check_text(kind: &Kind, v: &str) -> Result<(), String> {
    match kind {
        Kind::Ports => PortSpec::parse(v).map(|_| ()).map_err(|e| e.to_string()),
        Kind::Flag => Ok(()),
        Kind::Int { min, max } => {
            let n: i64 = v
                .parse()
                .map_err(|_| format!("ожидается целое число, получено «{v}»"))?;
            if n < *min || n > *max {
                Err(format!("значение должно быть от {min} до {max}"))
            } else {
                Ok(())
            }
        }
        Kind::Enum { values, multi } => {
            let parts: Vec<&str> = if *multi {
                v.split(',').collect()
            } else {
                vec![v]
            };
            for p in &parts {
                if !values.contains(p) {
                    return Err(format!(
                        "недопустимое значение «{p}», допустимо: {}",
                        values.join(", ")
                    ));
                }
            }
            Ok(())
        }
        Kind::Fake { allow_bang } => {
            if is_hex_literal(v) || (*allow_bang && v == "!") {
                Ok(())
            } else {
                Err("ожидается файл-фейк, hex-значение 0x... или «!»".into())
            }
        }
        Kind::List => Err("ожидается ссылка на список".into()),
        Kind::Domains => {
            if v.split(',').all(is_domain) {
                Ok(())
            } else {
                Err("ожидаются домены через запятую".into())
            }
        }
        Kind::Cidrs => {
            if v.split(',').all(is_cidr) {
                Ok(())
            } else {
                Err("ожидаются IP-адреса или подсети через запятую".into())
            }
        }
        Kind::Mods { keys } => {
            for item in v.split(',') {
                let (k, val) = match item.split_once('=') {
                    Some((k, val)) => (k, Some(val)),
                    None => (item, None),
                };
                if !keys.contains(&k) {
                    return Err(format!(
                        "неизвестный модификатор «{k}», допустимо: {}",
                        keys.join(", ")
                    ));
                }
                if let Some(val) = val {
                    let ok = !val.is_empty()
                        && val.len() <= 253
                        && val
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
                    if !ok {
                        return Err(format!("некорректное значение модификатора «{item}»"));
                    }
                }
            }
            Ok(())
        }
        Kind::Markers => {
            if v.split(',').all(is_marker) {
                Ok(())
            } else {
                Err("ожидаются позиции: число, midsld, sniext+1, host+2 ...".into())
            }
        }
        Kind::Cutoff => {
            let ok = v.len() >= 2
                && matches!(v.as_bytes()[0], b'n' | b'd' | b's')
                && v.len() <= 10
                && v[1..].bytes().all(|b| b.is_ascii_digit());
            if ok {
                Ok(())
            } else {
                Err("ожидается n<число>, d<число> или s<число>".into())
            }
        }
        Kind::AutoTtl => {
            let ok = !v.is_empty()
                && v.len() <= 16
                && v.bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'-' | b':'));
            if ok {
                Ok(())
            } else {
                Err("ожидается дельта[:мин-макс]".into())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::{Rule, Strategy};

    fn base() -> Strategy {
        Strategy::new_empty("t", "T")
    }

    #[test]
    fn default_strategy_is_valid() {
        let rep = validate(&base(), None);
        assert!(!rep.has_errors(), "{:?}", rep.issues);
    }

    #[test]
    fn rejects_unknown_and_injection() {
        let mut s = base();
        s.rules[0].args.push(Arg::text("--debug", "@C:\\x"));
        s.rules[0]
            .args
            .push(Arg::text("--dpi-desync-repeats", "6 & calc.exe"));
        s.rules[0]
            .args
            .push(Arg::text("--hostlist-domains", "a.com\"--wf-raw"));
        s.rules[0]
            .args
            .push(Arg::fake("--dpi-desync-fake-tls", "..\\..\\Windows\\x"));
        s.rules[0]
            .args
            .push(Arg::text("--hostlist", "C:\\list.txt"));
        let rep = validate(&s, None);
        assert_eq!(rep.errors().count(), 5, "{:?}", rep.issues);
    }

    #[test]
    fn scope_and_duplicates() {
        let mut s = base();
        s.global.push(Arg::text("--filter-tcp", "443"));
        s.rules[0].args.push(Arg::text("--wf-tcp", "443"));
        s.rules[0].args.push(Arg::text("--dpi-desync", "fake"));
        let rep = validate(&s, None);
        assert_eq!(rep.errors().count(), 3, "{:?}", rep.issues);
    }

    #[test]
    fn missing_resource() {
        let s = base();
        let idx = ResourceIndex::default();
        let rep = validate(&s, Some(&idx));
        assert!(rep.errors().any(|i| i.message.contains("list-general.txt")));
    }

    #[test]
    fn coverage_warning() {
        let mut s = base();
        s.rules.push(Rule {
            label: None,
            enabled: true,
            args: vec![
                Arg::text("--filter-udp", "50000-50100"),
                Arg::text("--dpi-desync", "fake"),
            ],
        });
        let rep = validate(&s, None);
        assert!(!rep.has_errors());
        assert!(rep.issues.iter().any(|i| i.message.contains("50000-50100")));
    }

    #[test]
    fn storable_allows_drafts_but_not_unsafe_data() {
        let mut draft = base();
        draft.rules[0].args.push(Arg::text("--unknown-option", "x"));
        draft.rules[0]
            .args
            .push(Arg::text("--dpi-desync-repeats", "abc"));
        assert!(validate(&draft, None).has_errors());
        assert!(validate_storable(&draft).is_ok(), "drafts are storable");

        let mut empty = base();
        empty.rules.clear();
        assert!(
            validate_storable(&empty).is_ok(),
            "an empty draft can be saved"
        );

        let mut bad = base();
        bad.rules[0]
            .args
            .push(Arg::fake("--dpi-desync-fake-tls", "..\\..\\x.bin"));
        assert!(validate_storable(&bad).is_err(), "path in resource name");

        let mut bad = base();
        bad.rules[0].args.push(Arg::text("rm -rf", "x"));
        assert!(validate_storable(&bad).is_err(), "not an option name");

        let mut bad = base();
        bad.rules[0]
            .args
            .push(Arg::text("--hostlist-domains", "a\u{7}b"));
        assert!(validate_storable(&bad).is_err(), "control char in value");

        let mut bad = base();
        bad.name = "x\u{1b}[31m".into();
        assert!(validate_storable(&bad).is_err(), "control char in name");
    }

    #[test]
    fn value_kinds() {
        assert!(check_text(&Kind::Markers, "2,sniext+1").is_ok());
        assert!(check_text(&Kind::Markers, "1,midsld").is_ok());
        assert!(check_text(&Kind::Markers, "evil").is_err());
        assert!(check_text(&Kind::Cutoff, "n3").is_ok());
        assert!(check_text(&Kind::Cutoff, "x3").is_err());
        assert!(check_text(&Kind::Fake { allow_bang: true }, "!").is_ok());
        assert!(check_text(&Kind::Fake { allow_bang: false }, "!").is_err());
        assert!(check_text(&Kind::Fake { allow_bang: false }, "0x00000000").is_ok());
        assert!(check_text(&Kind::Fake { allow_bang: false }, "0x0").is_err());
        assert!(check_text(&Kind::Cidrs, "10.0.0.0/8,::1,fe80::/10").is_ok());
        assert!(check_text(&Kind::Cidrs, "10.0.0.0/33").is_err());
    }
}
