//! Service cards: what the original Flowseal files say about a service,
//! kept apart from what the catalog says and from what the running engine
//! uses.
//!
//! * **Catalog** (signed): which domains belong to a service (`Pack.domains`)
//!   and which Flowseal list covers it (`Pack.flowseal_list`).
//! * **Flowseal files** (the active bundle, hash-checked): every original
//!   line, untouched. For each service domain this module finds the exact
//!   line (or the parent-domain line) that covers it, and whether an
//!   exclusion list removes it again.
//! * **Strategy**: which enabled rules of a strategy handle the service —
//!   by its host list, by inline `--hostlist-domains`, or by an L7 protocol
//!   filter — on which protocol and ports. The shared IPSet rules are listed
//!   separately: they never belong to one service.
//!
//! Nothing here decides that a service *works*: that needs a real check.

use crate::lists::{IPSET_ALL, IPSET_ALL_BACKUP, USER_GENERAL};
use crate::ports::{GameFilterPorts, PortSpec};
use crate::strategy::{ArgValue, Strategy};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Biggest list file read for display (Flowseal's IPSet backup is ~0.6 MB).
pub const MAX_LIST_BYTES: usize = 16 * 1024 * 1024;
/// Most lines one page of a list may return.
pub const MAX_PAGE_LINES: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ListKind {
    /// Domains handled by the bypass (`--hostlist`).
    Hostlist,
    /// Domains the bypass must leave alone (`--hostlist-exclude`).
    HostlistExclude,
    /// IP networks handled by the bypass (`--ipset`).
    Ipset,
    /// IP networks the bypass must leave alone (`--ipset-exclude`).
    IpsetExclude,
    /// Flowseal's full IP list, the source of `ipset-all.txt` in the
    /// "Flowseal" IPSet modes.
    IpsetSource,
    Other,
}

pub fn list_kind(name: &str) -> ListKind {
    match name {
        IPSET_ALL_BACKUP => ListKind::IpsetSource,
        IPSET_ALL => ListKind::Ipset,
        n if n.starts_with("ipset-exclude") => ListKind::IpsetExclude,
        n if n.starts_with("list-exclude") => ListKind::HostlistExclude,
        n if n.starts_with("ipset") => ListKind::Ipset,
        n if n.starts_with("list-") => ListKind::Hostlist,
        _ => ListKind::Other,
    }
}

/// The lines of a list file exactly as stored (line endings removed only).
pub fn raw_lines(text: &str) -> Vec<&str> {
    let mut v: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if v.last() == Some(&"") {
        v.pop();
    }
    v
}

/// One entry of a zapret host list. `^domain` matches the domain only,
/// without its subdomains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostEntry {
    /// 1-based line number in the file.
    pub line: usize,
    pub raw: String,
    pub domain: String,
    pub exact: bool,
}

pub fn parse_hostlist(text: &str) -> Vec<HostEntry> {
    raw_lines(text)
        .into_iter()
        .enumerate()
        .filter_map(|(i, raw)| {
            let t = raw.trim().trim_start_matches('\u{feff}');
            if t.is_empty() || t.starts_with('#') {
                return None;
            }
            let (exact, d) = match t.strip_prefix('^') {
                Some(d) => (true, d),
                None => (false, t),
            };
            Some(HostEntry {
                line: i + 1,
                raw: raw.to_string(),
                domain: d.trim_end_matches('.').to_ascii_lowercase(),
                exact,
            })
        })
        .collect()
}

/// Does `entry` (a host-list domain) match `host`?
pub fn covers(entry: &str, exact: bool, host: &str) -> bool {
    let host = host.trim_end_matches('.');
    host.eq_ignore_ascii_case(entry)
        || (!exact
            && host.len() > entry.len()
            && host.as_bytes()[host.len() - entry.len() - 1] == b'.'
            && host[host.len() - entry.len()..].eq_ignore_ascii_case(entry))
}

/// Two domains of different cards concern the same traffic if they are
/// equal or one is a subdomain of the other.
fn related(a: &str, b: &str) -> bool {
    covers(a, false, b) || covers(b, false, a)
}

/// Grouping key: the last two labels (`yt3.ggpht.com` → `ggpht.com`).
pub fn domain_group(d: &str) -> String {
    let labels: Vec<&str> = d.trim_end_matches('.').rsplitn(3, '.').collect();
    match labels.as_slice() {
        [tld, sld, ..] => format!("{sld}.{tld}").to_ascii_lowercase(),
        _ => d.to_ascii_lowercase(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRef {
    pub list: String,
    pub line: usize,
    pub raw: String,
    /// The line names this domain itself (`exact`) or a parent domain.
    pub parent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainInfo {
    pub domain: String,
    pub group: String,
    /// Host lists of the active Flowseal version that contain it.
    pub sources: Vec<LineRef>,
    /// Exclusion lines that remove it again (then it is not bypassed).
    pub excluded_by: Vec<LineRef>,
    /// Other cards / packs with the same or a related domain.
    pub shared_with: Vec<String>,
}

/// A host list of the active bundle, parsed.
pub struct ParsedList<'a> {
    pub name: &'a str,
    pub entries: Vec<HostEntry>,
}

fn refs(lists: &[ParsedList], domain: &str) -> Vec<LineRef> {
    let mut out = Vec::new();
    for l in lists {
        for e in &l.entries {
            if covers(&e.domain, e.exact, domain) {
                out.push(LineRef {
                    list: l.name.to_string(),
                    line: e.line,
                    raw: e.raw.clone(),
                    parent: !e.domain.eq_ignore_ascii_case(domain.trim_end_matches('.')),
                });
            }
        }
    }
    out
}

/// Per-domain facts for one card. `hostlists` / `excludes`: the parsed files
/// of the active bundle; `others`: (id, domains) of every other card or pack.
pub fn analyze_domains(
    domains: &[String],
    hostlists: &[ParsedList],
    excludes: &[ParsedList],
    others: &[(&str, &[String])],
) -> Vec<DomainInfo> {
    domains
        .iter()
        .map(|d| DomainInfo {
            domain: d.clone(),
            group: domain_group(d),
            sources: refs(hostlists, d),
            excluded_by: refs(excludes, d),
            shared_with: others
                .iter()
                .filter(|(_, ds)| ds.iter().any(|o| related(o, d)))
                .map(|(id, _)| id.to_string())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
        })
        .collect()
}

/// Lines of a Flowseal host list that no card claims (shown honestly as
/// "not attributed").
pub fn unattributed<'a>(list: &'a ParsedList, cards: &[&[String]]) -> Vec<&'a HostEntry> {
    list.entries
        .iter()
        .filter(|e| {
            !cards
                .iter()
                .any(|ds| ds.iter().any(|d| d.eq_ignore_ascii_case(&e.domain)))
        })
        .collect()
}

// ---------------------------------------------------------------- strategy

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleInfo {
    /// 1-based position among the strategy's rules (`--new` blocks).
    pub index: usize,
    pub label: Option<String>,
    pub enabled: bool,
    /// Ports with the game filter substituted (`443`, `19294-19344,50000-50100`).
    pub tcp: Option<String>,
    pub udp: Option<String>,
    pub hostlists: Vec<String>,
    pub hostlist_domains: Vec<String>,
    pub hostlist_excludes: Vec<String>,
    pub ipsets: Vec<String>,
    pub ipset_excludes: Vec<String>,
    pub l7: Vec<String>,
    /// The method (`fake`, `multisplit`, …).
    pub desync: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyFacts {
    pub id: String,
    pub name: String,
    /// Ports WinDivert captures at all (`--wf-tcp`, `--wf-udp`).
    pub wf_tcp: Option<String>,
    pub wf_udp: Option<String>,
    pub rules: Vec<RuleInfo>,
}

fn text(v: &Option<ArgValue>) -> Option<String> {
    match v {
        Some(ArgValue::Text(t)) => Some(t.clone()),
        Some(ArgValue::List { list }) => Some(list.clone()),
        Some(ArgValue::Fake { fake }) => Some(fake.clone()),
        None => None,
    }
}

fn ports(v: &Option<ArgValue>, gf: &GameFilterPorts) -> Option<String> {
    let t = text(v)?;
    Some(PortSpec::parse(&t).map(|p| p.render(gf)).unwrap_or(t))
}

pub fn strategy_facts(s: &Strategy, gf: &GameFilterPorts) -> StrategyFacts {
    let mut f = StrategyFacts {
        id: s.id.clone(),
        name: s.name.clone(),
        ..Default::default()
    };
    for a in &s.global {
        match a.opt.as_str() {
            "--wf-tcp" => f.wf_tcp = ports(&a.value, gf),
            "--wf-udp" => f.wf_udp = ports(&a.value, gf),
            _ => {}
        }
    }
    for (i, r) in s.rules.iter().enumerate() {
        let mut ri = RuleInfo {
            index: i + 1,
            label: r.label.clone(),
            enabled: r.enabled,
            ..Default::default()
        };
        for a in &r.args {
            let v = text(&a.value);
            match a.opt.as_str() {
                "--filter-tcp" => ri.tcp = ports(&a.value, gf),
                "--filter-udp" => ri.udp = ports(&a.value, gf),
                "--hostlist" => ri.hostlists.extend(v),
                "--hostlist-exclude" => ri.hostlist_excludes.extend(v),
                "--ipset" => ri.ipsets.extend(v),
                "--ipset-exclude" => ri.ipset_excludes.extend(v),
                "--hostlist-domains" => ri.hostlist_domains.extend(
                    v.iter()
                        .flat_map(|t| t.split(','))
                        .map(|d| d.trim().to_ascii_lowercase())
                        .filter(|d| !d.is_empty()),
                ),
                "--filter-l7" => ri.l7.extend(
                    v.iter()
                        .flat_map(|t| t.split(','))
                        .map(|p| p.trim().to_ascii_lowercase())
                        .filter(|p| !p.is_empty()),
                ),
                "--dpi-desync" => ri.desync = v,
                _ => {}
            }
        }
        f.rules.push(ri);
    }
    f
}

/// Why a rule concerns a card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "by", content = "value")]
pub enum RuleMatch {
    /// The rule uses the card's host list.
    Hostlist(String),
    /// Inline `--hostlist-domains` names one of the card's domains.
    Domains(Vec<String>),
    /// An L7 protocol filter of this service (e.g. Discord voice: `discord`, `stun`).
    Protocol(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardRule {
    pub index: usize,
    pub matched: RuleMatch,
    /// Addresses of the card this rule actually handles: covered by its host
    /// lists / inline domains and not removed by *its own* exclusion lists.
    /// Protocol rules handle traffic, not addresses: 0.
    #[serde(default)]
    pub covers: usize,
}

/// L7 protocols that identify a service's own traffic in winws.
pub fn service_protocols(card_id: &str) -> &'static [&'static str] {
    match card_id {
        // Discord voice (RTP over UDP with Discord's IP discovery) and the
        // STUN it uses; Flowseal targets both with one rule.
        "discord" => &["discord", "stun"],
        _ => &[],
    }
}

/// Enabled rules that handle the card's traffic. `lists`: the Flowseal list
/// of the card (`flowseal_list`), or `list-general-user.txt` for installed
/// packs (their domains are written there).
pub fn card_rules(
    facts: &StrategyFacts,
    card_id: &str,
    lists: &[&str],
    domains: &[String],
) -> Vec<CardRule> {
    let protos = service_protocols(card_id);
    let mut out = Vec::new();
    for r in facts.rules.iter().filter(|r| r.enabled) {
        let m = if let Some(l) = r.hostlists.iter().find(|h| lists.contains(&h.as_str())) {
            Some(RuleMatch::Hostlist(l.clone()))
        } else {
            let ds: Vec<String> = r
                .hostlist_domains
                .iter()
                .filter(|h| domains.iter().any(|d| related(h, d)))
                .cloned()
                .collect();
            if !ds.is_empty() {
                Some(RuleMatch::Domains(ds))
            } else {
                let ps: Vec<String> =
                    r.l7.iter()
                        .filter(|p| protos.contains(&p.as_str()))
                        .cloned()
                        .collect();
                (!ps.is_empty()).then_some(RuleMatch::Protocol(ps))
            }
        };
        if let Some(matched) = m {
            out.push(CardRule {
                index: r.index,
                matched,
                covers: 0,
            });
        }
    }
    out
}

/// Enabled rules that use the shared IPSet (`ipset-all.txt`): they apply to
/// IP addresses on their ports, independently of any one service.
pub fn ipset_rules(facts: &StrategyFacts) -> Vec<usize> {
    facts
        .rules
        .iter()
        .filter(|r| r.enabled && r.ipsets.iter().any(|i| i == IPSET_ALL))
        .map(|r| r.index)
        .collect()
}

// ---------------------------------------------------------------- ports

/// Ports of one protocol that a pack needs (`Pack.ports`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NeededPorts {
    /// `tcp` or `udp`.
    pub protocol: String,
    pub ports: String,
}

/// Whether the active strategy processes a pack's own ports for IP addresses.
/// This is about configuration only: it never says that the service works.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum PortCoverage {
    /// Every needed port is captured and handled by these enabled rules that
    /// apply to addresses of the IP set (whether the pack's IPs are in that
    /// set is the separate IP-mode status).
    Covered { rules: Vec<usize> },
    /// Not handled now; handled once the game filter is set to `mode` (which
    /// keeps the protocols that are on already).
    NeedsGameFilter {
        mode: crate::ports::GameFilterMode,
        missing: Vec<NeededPorts>,
    },
    /// Not handled by any rule of the strategy, even with the game filter
    /// (its ranges exclude these ports, or the strategy has no such rule).
    NoRule { missing: Vec<NeededPorts> },
}

/// A rule that would apply to the pack's IP traffic on its ports: it uses
/// the shared IP set, or has no positive address filter at all, and no L7
/// filter that game traffic (`unknown`) would fail.
fn applies_to_ips(r: &RuleInfo) -> bool {
    let l7_ok = r.l7.is_empty() || r.l7.iter().any(|p| p == "unknown");
    let ipset = r.ipsets.iter().any(|i| i == IPSET_ALL);
    let unrestricted =
        r.hostlists.is_empty() && r.hostlist_domains.is_empty() && r.ipsets.is_empty();
    r.enabled && l7_ok && (ipset || unrestricted)
}

fn ranges_of(spec: &Option<String>) -> Vec<(u16, u16)> {
    spec.as_deref()
        .and_then(|s| PortSpec::parse(s).ok())
        .and_then(|p| p.ranges())
        .unwrap_or_default()
}

/// Rules handling every port of `need`, and the needs nothing handles.
fn evaluate(
    s: &Strategy,
    gf: &GameFilterPorts,
    needs: &[crate::packs::PortNeed],
) -> (Vec<usize>, Vec<NeededPorts>) {
    let facts = strategy_facts(s, gf);
    let mut rules = BTreeSet::new();
    let mut missing = Vec::new();
    for n in needs {
        let tcp = n.protocol == "tcp";
        let wf = if tcp { &facts.wf_tcp } else { &facts.wf_udp };
        let captured = crate::ports::covers_all(&ranges_of(wf), &n.ranges);
        let found: Vec<usize> = facts
            .rules
            .iter()
            .filter(|r| {
                let filter = if tcp { &r.tcp } else { &r.udp };
                applies_to_ips(r) && crate::ports::covers_all(&ranges_of(filter), &n.ranges)
            })
            .map(|r| r.index)
            .collect();
        if captured && !found.is_empty() {
            rules.extend(found);
        } else {
            missing.push(NeededPorts {
                protocol: n.protocol.to_string(),
                ports: n.text.to_string(),
            });
        }
    }
    (rules.into_iter().collect(), missing)
}

/// Whether `s`, with the user's game filter `gf`, handles the ports of a
/// pack. When it does not, the game filter mode that would fix it (if any).
pub fn port_coverage(
    s: &Strategy,
    gf: &crate::ports::GameFilter,
    ports: &crate::packs::PackPorts,
) -> PortCoverage {
    use crate::ports::GameFilterMode as M;
    let needs = ports.needs();
    let (rules, missing) = evaluate(s, &gf.ports(), &needs);
    if missing.is_empty() {
        return PortCoverage::Covered { rules };
    }
    let on = |m: M| match m {
        M::Disabled => (false, false),
        M::All => (true, true),
        M::Tcp => (true, false),
        M::Udp => (false, true),
    };
    let (mut tcp, mut udp) = on(gf.mode);
    for n in &missing {
        if n.protocol == "tcp" {
            tcp = true;
        } else {
            udp = true;
        }
    }
    let target = match (tcp, udp) {
        (true, true) => M::All,
        (true, false) => M::Tcp,
        (false, true) => M::Udp,
        (false, false) => M::Disabled,
    };
    if target != gf.mode {
        let with = crate::ports::GameFilter {
            mode: target,
            ..gf.clone()
        };
        let (_, still) = evaluate(s, &with.ports(), &needs);
        if still.is_empty() {
            return PortCoverage::NeedsGameFilter {
                mode: target,
                missing,
            };
        }
        return PortCoverage::NoRule { missing: still };
    }
    PortCoverage::NoRule { missing }
}

/// Port status of one catalog pack, or `None` when there is nothing to say:
/// no ports and no game-filter requirement, or no active strategy. A pack of
/// an older catalog without `ports` but with `requires.gameFilter` still
/// gets the game-filter hint when the filter is off.
pub fn pack_port_coverage(
    pack: &crate::packs::Pack,
    s: Option<&Strategy>,
    gf: &crate::ports::GameFilter,
) -> Option<PortCoverage> {
    if let Some(p) = &pack.ports {
        return s.map(|s| port_coverage(s, gf, p));
    }
    if pack.requires.game_filter && gf.mode == crate::ports::GameFilterMode::Disabled {
        return Some(PortCoverage::NeedsGameFilter {
            mode: crate::ports::GameFilterMode::All,
            missing: Vec::new(),
        });
    }
    None
}

/// Lists a pack's domains end up in when installed.
pub fn installed_pack_lists() -> &'static [&'static str] {
    &[USER_GENERAL]
}

// ---------------------------------------------------------------- paging

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPage {
    pub name: String,
    /// All lines of the file.
    pub total: usize,
    /// Lines matching the query (= total without a query).
    pub matched: usize,
    pub offset: usize,
    /// (1-based line number, original line).
    pub lines: Vec<(usize, String)>,
}

/// A page of a list, optionally filtered by a case-insensitive substring.
pub fn page(name: &str, text: &str, query: &str, offset: usize, limit: usize) -> ListPage {
    let all = raw_lines(text);
    let q = query.trim().to_ascii_lowercase();
    let limit = limit.min(MAX_PAGE_LINES);
    let mut matched = 0;
    let mut lines = Vec::new();
    for (i, l) in all.iter().enumerate() {
        if q.is_empty() || l.to_ascii_lowercase().contains(&q) {
            if matched >= offset && lines.len() < limit {
                lines.push((i + 1, l.to_string()));
            }
            matched += 1;
        }
    }
    ListPage {
        name: name.to_string(),
        total: all.len(),
        matched,
        offset,
        lines,
    }
}

/// Count of meaningful entries (no blanks, no comments).
pub fn entry_count(text: &str) -> usize {
    raw_lines(text)
        .iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .count()
}

/// Per-group domain counts, for the compact card.
pub fn group_counts(domains: &[DomainInfo]) -> BTreeMap<String, usize> {
    let mut m = BTreeMap::new();
    for d in domains {
        *m.entry(d.group.clone()).or_insert(0) += 1;
    }
    m
}

// ---------------------------------------------------------------- cards

/// How far the bypass has got for a card. Never says the service *works*:
/// that is a separate, real check (diagnostics, voice confirmation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplyState {
    /// No verified source lists the card's addresses (e.g. the Flowseal file
    /// is missing, or every domain is excluded).
    Unknown,
    /// Addresses are in a verified source (signed catalog / Flowseal file),
    /// but the selected strategy has no rule for them (or a pack is not installed).
    Known,
    /// The selected strategy has rules for them: starting it would apply them.
    Prepared,
    /// The running engine uses exactly this configuration (same strategy,
    /// same Flowseal version, no unapplied changes).
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CardKind {
    /// Covered by a Flowseal list: information only, never installed.
    Flowseal,
    /// A catalog pack the user installed (domains go to list-general-user.txt).
    Installed,
    /// A catalog pack that is not installed.
    Available,
}

pub struct CardInput<'a> {
    pub id: &'a str,
    pub kind: CardKind,
    pub flowseal_list: Option<&'a str>,
    pub domains: &'a [String],
    pub ips: &'a [String],
}

/// What the engine runs right now (from the helper status).
#[derive(Debug, Clone, Default)]
pub struct Running {
    pub strategy_id: Option<String>,
    pub bundle_version: Option<String>,
    /// The running configuration differs from the saved one.
    pub pending_apply: bool,
}

pub struct Context<'a> {
    pub bundle_version: Option<&'a str>,
    /// Text of each list file of the active Flowseal version that could be
    /// read and verified (name → text).
    pub lists: &'a BTreeMap<String, String>,
    /// The strategy the bypass starts with (selected in the settings).
    pub selected: Option<&'a Strategy>,
    pub game_filter: GameFilterPorts,
    pub ipset_mode: crate::ipset::IpsetMode,
    /// Manual lists merged with installed packs (`effective_lists`).
    pub effective: &'a crate::lists::UserLists,
    /// None: the engine does not run.
    pub running: Option<Running>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSummary {
    pub id: String,
    pub kind: CardKind,
    /// Domains of the service itself (catalog).
    pub domains: usize,
    /// Of those, found in a Flowseal host list of the active version.
    pub in_flowseal: usize,
    /// Of those, removed again by an exclusion list.
    pub excluded: usize,
    /// Domains another card or pack also lists.
    pub shared: usize,
    /// IP networks of the service itself (catalog); the shared IPSet is not counted.
    pub ips: usize,
    /// Rules of the selected strategy for this card, and how many of them
    /// match by protocol or inline domains (besides the host list).
    pub rules: usize,
    pub extra_rules: usize,
    pub state: ApplyState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub name: String,
    pub kind: ListKind,
    /// Read and verified for the active version.
    pub present: bool,
    pub lines: usize,
    pub entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpsetFacts {
    pub mode: crate::ipset::IpsetMode,
    /// Networks in `ipset-all.txt` as the engine gets it with the current
    /// settings (None: could not be built, e.g. the Flowseal list is missing).
    pub runtime_entries: Option<usize>,
    /// The runtime file is Flowseal's inert placeholder (matches nothing).
    pub runtime_placeholder: bool,
    /// Every address: an empty `ipset-all.txt`.
    pub runtime_all: bool,
    /// Networks in Flowseal's full list (`ipset-all.txt.backup`).
    pub source_entries: Option<usize>,
    /// Rules of the selected strategy that use the shared IPSet.
    pub rules: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardDetails {
    pub summary: CardSummary,
    pub domains: Vec<DomainInfo>,
    pub service_ips: Vec<String>,
    pub rules: Vec<CardRule>,
    pub strategy: Option<StrategyFacts>,
    pub ipset: IpsetFacts,
    pub sources: Vec<SourceFile>,
    pub bundle_version: Option<String>,
    pub running_strategy: Option<String>,
    pub pending_apply: bool,
}

const SOURCE_NAMES: &[&str] = &[
    "list-general.txt",
    "list-google.txt",
    "list-exclude.txt",
    "ipset-all.txt",
    "ipset-exclude.txt",
    IPSET_ALL_BACKUP,
];

pub fn source_files(lists: &BTreeMap<String, String>) -> Vec<SourceFile> {
    let mut names: Vec<String> = SOURCE_NAMES.iter().map(|s| s.to_string()).collect();
    for n in lists.keys() {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    names
        .into_iter()
        .map(|n| {
            let t = lists.get(&n);
            SourceFile {
                kind: list_kind(&n),
                present: t.is_some(),
                lines: t.map(|t| raw_lines(t).len()).unwrap_or(0),
                entries: t.map(|t| entry_count(t)).unwrap_or(0),
                name: n,
            }
        })
        .collect()
}

fn parsed<'a>(ctx: &'a Context, kind: ListKind) -> Vec<ParsedList<'a>> {
    ctx.lists
        .iter()
        .filter(|(n, _)| list_kind(n) == kind)
        .map(|(n, t)| ParsedList {
            name: n.as_str(),
            entries: parse_hostlist(t),
        })
        .collect()
}

fn card_lists<'a>(c: &CardInput<'a>) -> Vec<&'a str> {
    match c.kind {
        CardKind::Flowseal => c.flowseal_list.into_iter().collect(),
        CardKind::Installed => installed_pack_lists().to_vec(),
        CardKind::Available => vec![],
    }
}

/// How many of the card's domains `rule` really handles (see `CardRule::covers`).
fn rule_covers(
    card: &CardInput,
    domains: &[DomainInfo],
    m: &RuleMatch,
    rule: &RuleInfo,
    ctx: &Context,
) -> usize {
    let user_exclude = ctx.effective.exclude.join("\n");
    let excl: Vec<HostEntry> = rule
        .hostlist_excludes
        .iter()
        .flat_map(|name| {
            if name == crate::lists::USER_EXCLUDE {
                parse_hostlist(&user_exclude)
            } else {
                ctx.lists
                    .get(name)
                    .map(|t| parse_hostlist(t))
                    .unwrap_or_default()
            }
        })
        .collect();
    let excluded = |d: &str| excl.iter().any(|e| covers(&e.domain, e.exact, d));
    domains
        .iter()
        .filter(|d| match m {
            RuleMatch::Hostlist(l) if l == USER_GENERAL => card.kind == CardKind::Installed,
            RuleMatch::Hostlist(l) => d.sources.iter().any(|s| &s.list == l),
            RuleMatch::Domains(ds) => ds.iter().any(|x| related(x, &d.domain)),
            RuleMatch::Protocol(_) => false,
        })
        .filter(|d| !excluded(&d.domain))
        .count()
}

/// Details of one card. `all`: every card (for shared domains).
pub fn details(card: &CardInput, all: &[CardInput], ctx: &Context) -> CardDetails {
    let hostlists = parsed(ctx, ListKind::Hostlist);
    let excludes = parsed(ctx, ListKind::HostlistExclude);
    let others: Vec<(&str, &[String])> = all
        .iter()
        .filter(|o| o.id != card.id)
        .map(|o| (o.id, o.domains))
        .collect();
    let domains = analyze_domains(card.domains, &hostlists, &excludes, &others);
    let facts = ctx.selected.map(|s| strategy_facts(s, &ctx.game_filter));
    // The card's own list plus every Flowseal list that really contains one
    // of its domains (e.g. dns.google of the Google card is in list-general.txt).
    let mut lists = card_lists(card);
    if card.kind == CardKind::Flowseal {
        for src in domains.iter().flat_map(|d| d.sources.iter()) {
            if !lists.contains(&src.list.as_str()) {
                lists.push(src.list.as_str());
            }
        }
    }
    let mut rules = match (&facts, card.kind) {
        (Some(f), CardKind::Flowseal | CardKind::Installed) => {
            card_rules(f, card.id, &lists, card.domains)
        }
        _ => vec![],
    };
    if let Some(f) = &facts {
        for r in &mut rules {
            if let Some(info) = f.rules.iter().find(|x| x.index == r.index) {
                r.covers = rule_covers(card, &domains, &r.matched, info, ctx);
            }
        }
    }
    let in_flowseal = domains.iter().filter(|d| !d.sources.is_empty()).count();
    let excluded = domains.iter().filter(|d| !d.excluded_by.is_empty()).count();
    let handled = domains
        .iter()
        .filter(|d| {
            d.excluded_by.is_empty() && (card.kind == CardKind::Installed || !d.sources.is_empty())
        })
        .count()
        + if card.kind == CardKind::Installed {
            card.ips.len()
        } else {
            0
        };
    let known = match card.kind {
        CardKind::Flowseal => handled > 0,
        CardKind::Installed | CardKind::Available => {
            !card.domains.is_empty() || !card.ips.is_empty()
        }
    };
    let selected_id = ctx.selected.map(|s| s.id.as_str());
    let applied_config = ctx.running.as_ref().is_some_and(|r| {
        !r.pending_apply
            && r.strategy_id.as_deref().is_some()
            && r.strategy_id.as_deref() == selected_id
            && r.bundle_version.as_deref() == ctx.bundle_version
    });
    let state = if !known {
        ApplyState::Unknown
    } else if handled == 0 || !rules.iter().any(|r| r.covers > 0) {
        // A rule that matches the card but handles none of its addresses
        // (all excluded for that rule, or another list) prepares nothing.
        ApplyState::Known
    } else if applied_config {
        ApplyState::Applied
    } else {
        ApplyState::Prepared
    };
    let summary = CardSummary {
        id: card.id.to_string(),
        kind: card.kind,
        domains: card.domains.len(),
        in_flowseal,
        excluded,
        shared: domains.iter().filter(|d| !d.shared_with.is_empty()).count(),
        ips: card.ips.len(),
        rules: rules.len(),
        extra_rules: rules
            .iter()
            .filter(|r| !matches!(r.matched, RuleMatch::Hostlist(_)))
            .count(),
        state,
    };
    let backup = ctx.lists.get(IPSET_ALL_BACKUP).map(String::as_str);
    let runtime = crate::lists::runtime_list_files(ctx.effective, ctx.ipset_mode, backup)
        .ok()
        .and_then(|f| f.into_iter().find(|(n, _)| *n == IPSET_ALL).map(|(_, t)| t));
    let ipset = IpsetFacts {
        mode: ctx.ipset_mode,
        runtime_entries: runtime.as_deref().map(entry_count),
        runtime_placeholder: runtime
            .as_deref()
            .is_some_and(|t| t.trim() == crate::lists::PLACEHOLDER_IP),
        runtime_all: runtime.as_deref().is_some_and(|t| t.trim().is_empty()),
        source_entries: backup.map(entry_count),
        rules: facts.as_ref().map(ipset_rules).unwrap_or_default(),
    };
    CardDetails {
        summary,
        domains,
        service_ips: card.ips.to_vec(),
        rules,
        strategy: facts,
        ipset,
        sources: source_files(ctx.lists),
        bundle_version: ctx.bundle_version.map(str::to_string),
        running_strategy: ctx.running.as_ref().and_then(|r| r.strategy_id.clone()),
        pending_apply: ctx.running.as_ref().is_some_and(|r| r.pending_apply),
    }
}

#[cfg(test)]
mod tests;
