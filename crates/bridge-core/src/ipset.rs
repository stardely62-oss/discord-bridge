//! IP sets: parsing, normalisation and the final `ipset-all.txt`.
//!
//! Flowseal strategies match IP rules against `ipset-all.txt` and skip
//! `ipset-exclude*.txt`. What goes into `ipset-all.txt` is chosen by
//! [`IpsetMode`] and built by [`build_ipset_all`] from:
//! 1. the user's "IP through the bypass" entries (`UserLists::ipset_include`),
//! 2. IPs of installed catalog packs (merged into the same list by
//!    `lists::effective_lists`),
//! 3. Flowseal's full list (`ipset-all.txt.backup`),
//!
//! minus the user's exclusions (`UserLists::ipset_exclude`), which always win.
//! The result is never an empty file unless the mode is `any`: winws treats
//! an empty ipset as "every address", so an empty selection is written as an
//! inert placeholder instead.

use crate::lists::PLACEHOLDER_IP;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::IpAddr;

/// What `ipset-all.txt` contains. Modes never change by themselves: installing
/// a pack with IPs does not widen the mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum IpsetMode {
    /// IP rules match nothing (Flowseal default).
    #[default]
    None,
    /// Only the user's IPs and the IPs of installed packs.
    Selected,
    /// Flowseal's full list (`ipset-all.txt.backup`). Former name `loaded`.
    #[serde(alias = "loaded")]
    Flowseal,
    /// Flowseal's full list plus the selected IPs.
    FlowsealPlusSelected,
    /// Every address (empty file). Advanced and risky: affects all traffic
    /// on the captured ports.
    Any,
}

impl IpsetMode {
    /// Whether the user's and the packs' IPs end up in `ipset-all.txt`.
    pub fn uses_selected(self) -> bool {
        matches!(self, IpsetMode::Selected | IpsetMode::FlowsealPlusSelected)
    }
    pub fn uses_flowseal(self) -> bool {
        matches!(self, IpsetMode::Flowseal | IpsetMode::FlowsealPlusSelected)
    }
}

/// A parsed network: address masked to its prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Net {
    v6: bool,
    bits: u128,
    prefix: u8,
}

impl Net {
    /// `1.2.3.4`, `1.2.3.0/24`, `2001:db8::/32`. A host part beyond the prefix
    /// is accepted and masked off.
    pub fn parse(s: &str) -> Option<Net> {
        let (ip, mask) = match s.trim().split_once('/') {
            Some((a, m)) => (a, Some(m)),
            None => (s.trim(), None),
        };
        let (v6, bits, max) = match ip.parse::<IpAddr>().ok()? {
            IpAddr::V4(a) => (false, u32::from(a) as u128, 32u8),
            IpAddr::V6(a) => (true, u128::from(a), 128u8),
        };
        let prefix = match mask {
            None => max,
            Some(m) if !m.is_empty() && m.bytes().all(|b| b.is_ascii_digit()) => {
                m.parse::<u8>().ok().filter(|p| *p <= max)?
            }
            Some(_) => return None,
        };
        Some(Net {
            v6,
            bits: bits & Self::mask(v6, prefix),
            prefix,
        })
    }

    fn mask(v6: bool, prefix: u8) -> u128 {
        let width = if v6 { 128 } else { 32 };
        if prefix == 0 {
            0
        } else {
            (u128::MAX << (width - prefix as u32)) & if v6 { u128::MAX } else { u32::MAX as u128 }
        }
    }

    /// `self` lies entirely inside `other`.
    pub fn within(&self, other: &Net) -> bool {
        self.v6 == other.v6
            && self.prefix >= other.prefix
            && self.bits & Self::mask(self.v6, other.prefix) == other.bits
    }

    pub fn prefix(&self) -> u8 {
        self.prefix
    }

    fn overlaps(&self, other: &Net) -> bool {
        self.within(other) || other.within(self)
    }

    /// The two halves of this network (prefix + 1).
    fn halves(&self) -> Option<(Net, Net)> {
        let width: u32 = if self.v6 { 128 } else { 32 };
        if self.prefix as u32 >= width {
            return None;
        }
        let p = self.prefix + 1;
        let bit = 1u128 << (width - p as u32);
        let lo = Net {
            v6: self.v6,
            bits: self.bits,
            prefix: p,
        };
        let hi = Net {
            v6: self.v6,
            bits: self.bits | bit,
            prefix: p,
        };
        Some((lo, hi))
    }

    /// Number of addresses (saturating for huge IPv6 networks).
    pub fn size(&self) -> u128 {
        let width = if self.v6 { 128 } else { 32 };
        1u128
            .checked_shl(width - self.prefix as u32)
            .unwrap_or(u128::MAX)
    }
    pub fn is_v6(&self) -> bool {
        self.v6
    }
}

impl std::fmt::Display for Net {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.v6 {
            write!(f, "{}/{}", std::net::Ipv6Addr::from(self.bits), self.prefix)
        } else {
            write!(
                f,
                "{}/{}",
                std::net::Ipv4Addr::from(self.bits as u32),
                self.prefix
            )
        }
    }
}

/// Upper bound for the lines of the final `ipset-all.txt` (Flowseal's full
/// list has ~33 000). Exceeding it is an error, never a silent cut.
pub const MAX_IPSET_ENTRIES: usize = 200_000;
/// Upper bound for the subtraction work (network comparisons and splits).
pub const MAX_IPSET_WORK: u64 = 50_000_000;

/// Work and size accounting for one ipset build.
#[derive(Debug, Default)]
pub struct Budget {
    work: u64,
}

impl Budget {
    fn spend(&mut self, n: usize) -> Result<(), IpsetError> {
        self.work = self.work.saturating_add(n as u64);
        if self.work > MAX_IPSET_WORK {
            Err(IpsetError::TooComplex)
        } else {
            Ok(())
        }
    }
}

/// `net` minus every network in `exclude`, as the smallest list of CIDR
/// blocks (in address order). A partial overlap splits the network, e.g.
/// 10.0.0.0/8 − 10.1.0.0/16 = eight blocks from /9 to /16. Exclusions are thus
/// enforced by the list itself, whatever `--ipset-exclude` a strategy uses.
/// The work is charged to `budget`; the caller bounds the result size.
pub fn subtract(net: Net, exclude: &[Net], budget: &mut Budget) -> Result<Vec<Net>, IpsetError> {
    budget.spend(exclude.len())?;
    let relevant: Vec<Net> = exclude
        .iter()
        .copied()
        .filter(|e| e.overlaps(&net))
        .collect();
    let mut out = Vec::new();
    fn go(n: Net, ex: &[Net], out: &mut Vec<Net>, budget: &mut Budget) -> Result<(), IpsetError> {
        budget.spend(ex.len() + 1)?;
        if ex.iter().any(|e| n.within(e)) {
            return Ok(());
        }
        let inner: Vec<Net> = ex.iter().copied().filter(|e| e.within(&n)).collect();
        if inner.is_empty() {
            if out.len() >= MAX_IPSET_ENTRIES {
                return Err(IpsetError::TooManyEntries);
            }
            out.push(n);
            return Ok(());
        }
        if let Some((lo, hi)) = n.halves() {
            go(lo, &inner, out, budget)?;
            go(hi, &inner, out, budget)?;
        }
        Ok(())
    }
    go(net, &relevant, &mut out, budget)?;
    Ok(out)
}

/// Narrowest network allowed in an "include" list: anything wider would make
/// the bypass cover a large part of the internet without choosing `any`.
pub const MIN_INCLUDE_PREFIX_V4: u8 = 8;
pub const MIN_INCLUDE_PREFIX_V6: u8 = 16;

/// Validation of an "include" entry (user list or catalog pack).
pub fn include_entry_error(s: &str) -> Option<&'static str> {
    let Some(n) = Net::parse(s) else {
        return Some("не является IP-адресом или подсетью");
    };
    let min = if n.is_v6() {
        MIN_INCLUDE_PREFIX_V6
    } else {
        MIN_INCLUDE_PREFIX_V4
    };
    (n.prefix() < min)
        .then_some("слишком широкая подсеть — для всех адресов есть режим «Все адреса»")
}

fn entries(lines: &[String]) -> impl Iterator<Item = &str> {
    lines
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

/// Appends `line` minus the exclusions: the original spelling when nothing
/// is cut out, otherwise the remaining blocks. Deduplicated through `seen`.
/// Fails instead of producing more than `MAX_IPSET_ENTRIES` lines.
fn push_minus(
    line: &str,
    net: Net,
    ex: &[Net],
    seen: &mut HashSet<Net>,
    out: &mut Vec<String>,
    budget: &mut Budget,
) -> Result<(), IpsetError> {
    let parts = subtract(net, ex, budget)?;
    let push = |s: String, out: &mut Vec<String>| {
        if out.len() >= MAX_IPSET_ENTRIES {
            return Err(IpsetError::TooManyEntries);
        }
        out.push(s);
        Ok(())
    };
    if parts.len() == 1 && parts[0] == net {
        if seen.insert(net) {
            push(line.to_string(), out)?;
        }
        return Ok(());
    }
    for p in parts {
        if seen.insert(p) {
            push(p.to_string(), out)?;
        }
    }
    Ok(())
}

/// Include entries deduplicated by network, minus the exclusions (partial
/// overlaps are split). Keeps the first spelling and the input order.
pub fn selected_ips(include: &[String], exclude: &[String]) -> Result<Vec<String>, IpsetError> {
    let ex: Vec<Net> = entries(exclude).filter_map(Net::parse).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut budget = Budget::default();
    for l in entries(include) {
        if let Some(n) = Net::parse(l) {
            push_minus(l, n, &ex, &mut seen, &mut out, &mut budget)?;
        }
    }
    Ok(out)
}

/// How a strategy uses IP lists: rules (enabled) that match against
/// `ipset-all.txt`, and how many of them also skip the user's exclusions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StrategyIpUse {
    pub ipset_rules: usize,
    pub ipset_rules_with_user_exclude: usize,
}

pub fn strategy_ip_use(s: &crate::strategy::Strategy) -> StrategyIpUse {
    use crate::strategy::ArgValue;
    let uses = |r: &crate::strategy::Rule, opt: &str, list: &str| {
        r.args.iter().any(|a| {
            a.opt == opt && matches!(&a.value, Some(ArgValue::List { list: l }) if l == list)
        })
    };
    let mut u = StrategyIpUse::default();
    for r in s.rules.iter().filter(|r| r.enabled) {
        if uses(r, "--ipset", crate::lists::IPSET_ALL) {
            u.ipset_rules += 1;
            if uses(r, "--ipset-exclude", crate::lists::USER_IPSET_EXCLUDE) {
                u.ipset_rules_with_user_exclude += 1;
            }
        }
    }
    u
}

/// Selected IPs of one pack that survive the exclusions (not entirely excluded).
pub fn effective_ip_count(ips: &[String], exclude: &[String]) -> Result<usize, IpsetError> {
    let ex: Vec<Net> = entries(exclude).filter_map(Net::parse).collect();
    let mut budget = Budget::default();
    let mut n = 0;
    for net in entries(ips).filter_map(Net::parse) {
        // Fully excluded = nothing left; no need to materialise the blocks.
        budget.spend(ex.len())?;
        if !ex.iter().any(|e| net.within(e)) {
            n += 1;
        }
    }
    Ok(n)
}

/// IP state of one installed pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackIpStatus {
    pub id: String,
    pub ips: usize,
    /// Entries not entirely covered by the user's exclusions.
    pub effective: usize,
}

/// What can honestly be said about the selected IPs. Each step is separate:
/// installed (packs) → in the prepared configuration (`prepared`) → the
/// strategy matches against them (`strategy.ipset_rules`) → the running
/// engine uses exactly this configuration (`applied`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IpSetStatus {
    pub mode: IpsetMode,
    pub mode_uses_selected: bool,
    /// Selected IP entries (manual + packs, minus exclusions) written to
    /// `ipset-all.txt` with the current settings; 0 if the mode ignores them.
    pub prepared: usize,
    pub strategy_id: Option<String>,
    pub strategy: StrategyIpUse,
    /// The engine runs this strategy.
    pub engine_runs_strategy: bool,
    /// The running engine uses exactly the current configuration.
    pub engine_up_to_date: bool,
    /// All of the above: the selected IPs are matched by the running engine.
    pub applied: bool,
    pub packs: Vec<PackIpStatus>,
    /// The IP list cannot be built (limit exceeded): starts fail with it.
    pub error: Option<String>,
}

pub fn ip_set_status(
    mode: IpsetMode,
    manual: &crate::lists::UserLists,
    packs: &[crate::lists::InstalledPack],
    strategy: Option<&crate::strategy::Strategy>,
    engine_runs_strategy: bool,
    pending_apply: bool,
) -> IpSetStatus {
    let eff = crate::lists::effective_lists(manual, packs);
    let mut error = None;
    let prepared = if mode.uses_selected() {
        match selected_ips(&eff.ipset_include, &eff.ipset_exclude) {
            Ok(v) => v.len(),
            Err(e) => {
                error = Some(e.to_string());
                0
            }
        }
    } else {
        0
    };
    let use_ = strategy.map(strategy_ip_use).unwrap_or_default();
    let up_to_date = engine_runs_strategy && !pending_apply;
    IpSetStatus {
        mode,
        mode_uses_selected: mode.uses_selected(),
        prepared,
        strategy_id: strategy.map(|s| s.id.clone()),
        strategy: use_,
        engine_runs_strategy,
        engine_up_to_date: up_to_date,
        applied: up_to_date && mode.uses_selected() && prepared > 0 && use_.ipset_rules > 0,
        packs: packs
            .iter()
            .filter(|p| !p.ips.is_empty())
            .map(|p| PackIpStatus {
                id: p.id.clone(),
                ips: p.ips.len(),
                // On a limit error report the pack as not excluded; `error` explains.
                effective: effective_ip_count(&p.ips, &manual.ipset_exclude).unwrap_or(p.ips.len()),
            })
            .collect(),
        error,
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IpsetError {
    #[error("в наборе Flowseal нет полного списка IP (ipset-all.txt.backup)")]
    NoBackup,
    #[error("список IP получился бы длиннее 200 000 записей — сократите исключения «IP — не трогать» или выберите другой режим IP")]
    TooManyEntries,
    #[error("слишком сложное вычитание исключений IP (более 50 млн шагов) — сократите список «IP — не трогать»")]
    TooComplex,
}

/// Content of `ipset-all.txt`. `include` already contains the packs' IPs
/// (see `lists::effective_lists`); `exclude` always wins.
pub fn build_ipset_all(
    mode: IpsetMode,
    include: &[String],
    exclude: &[String],
    backup: Option<&str>,
) -> Result<String, IpsetError> {
    let ex: Vec<Net> = entries(exclude).filter_map(Net::parse).collect();
    if mode == IpsetMode::Any {
        if ex.is_empty() {
            // Unchanged behaviour: an empty file means every address.
            return Ok(String::new());
        }
        // Every address minus the exclusions, so they hold even for a
        // strategy without `--ipset-exclude`.
        let mut seen = HashSet::new();
        let mut lines = Vec::new();
        let mut budget = Budget::default();
        for all in ["0.0.0.0/0", "::/0"] {
            push_minus(
                all,
                Net::parse(all).expect("valid"),
                &ex,
                &mut seen,
                &mut lines,
                &mut budget,
            )?;
        }
        return Ok(if lines.is_empty() {
            format!("{PLACEHOLDER_IP}\n")
        } else {
            format!("{}\n", lines.join("\n"))
        });
    }
    let mut lines: Vec<String> = Vec::new();
    let mut seen: HashSet<Net> = HashSet::new();
    let mut budget = Budget::default();
    if mode.uses_flowseal() {
        let backup = backup.ok_or(IpsetError::NoBackup)?;
        if ex.is_empty() && !mode.uses_selected() {
            // Unchanged Flowseal behaviour: the backup file as shipped.
            return Ok(if backup.trim().is_empty() {
                format!("{PLACEHOLDER_IP}\n")
            } else {
                backup.to_string()
            });
        }
        for l in backup.lines().map(str::trim).filter(|l| !l.is_empty()) {
            match Net::parse(l) {
                Some(n) => push_minus(l, n, &ex, &mut seen, &mut lines, &mut budget)?,
                // Keep anything Flowseal ships that we do not understand.
                None => {
                    if lines.len() >= MAX_IPSET_ENTRIES {
                        return Err(IpsetError::TooManyEntries);
                    }
                    lines.push(l.to_string())
                }
            }
        }
    }
    if mode.uses_selected() {
        for l in entries(include) {
            if let Some(n) = Net::parse(l) {
                push_minus(l, n, &ex, &mut seen, &mut lines, &mut budget)?;
            }
        }
    }
    if lines.is_empty() {
        return Ok(format!("{PLACEHOLDER_IP}\n"));
    }
    Ok(format!("{}\n", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }
    const BACKUP: &str = "1.0.0.0/24\n8.8.8.0/24\n2001:db8::/32\n";

    #[test]
    fn parses_and_normalises() {
        assert_eq!(Net::parse("10.1.2.3/8"), Net::parse("10.0.0.0/8"));
        assert_eq!(Net::parse("1.2.3.4"), Net::parse("1.2.3.4/32"));
        assert!(Net::parse("2001:db8::1/64").is_some());
        for bad in [
            "",
            "1.2.3",
            "1.2.3.4/33",
            "1.2.3.4/",
            "1.2.3.4/x",
            "::/129",
            "example.com",
            "1.2.3.4/-1",
        ] {
            assert!(Net::parse(bad).is_none(), "{bad}");
        }
        assert!(Net::parse("10.1.0.0/16")
            .unwrap()
            .within(&Net::parse("10.0.0.0/8").unwrap()));
        assert!(!Net::parse("10.0.0.0/8")
            .unwrap()
            .within(&Net::parse("10.1.0.0/16").unwrap()));
        assert!(!Net::parse("::a00:0/104")
            .unwrap()
            .within(&Net::parse("10.0.0.0/8").unwrap()));
    }

    #[test]
    fn include_validation_rejects_garbage_and_too_wide() {
        assert!(include_entry_error("149.154.160.0/20").is_none());
        assert!(include_entry_error("2a0a:f280::/32").is_none());
        assert!(include_entry_error("nonsense").is_some());
        assert!(include_entry_error("0.0.0.0/0").is_some());
        assert!(include_entry_error("10.0.0.0/7").is_some());
        assert!(include_entry_error("::/8").is_some());
    }

    #[test]
    fn manual_include_is_used_in_selected_mode() {
        let out = build_ipset_all(IpsetMode::Selected, &v(&["5.5.5.0/24"]), &[], None).unwrap();
        assert_eq!(out, "5.5.5.0/24\n");
    }

    #[test]
    fn exclusion_wins_and_duplicates_are_removed() {
        let inc = v(&["5.5.5.0/24", "5.5.5.9/24", "6.6.6.0/24", "10.1.0.0/16"]);
        let out = build_ipset_all(
            IpsetMode::Selected,
            &inc,
            &v(&["10.0.0.0/8", "6.6.6.0/24"]),
            None,
        )
        .unwrap();
        assert_eq!(out, "5.5.5.0/24\n");
    }

    #[test]
    fn mode_none_ignores_selected_ips() {
        let out = build_ipset_all(IpsetMode::None, &v(&["5.5.5.0/24"]), &[], Some(BACKUP)).unwrap();
        assert_eq!(out, format!("{PLACEHOLDER_IP}\n"));
    }

    #[test]
    fn empty_selection_is_never_an_empty_file() {
        assert_eq!(
            build_ipset_all(IpsetMode::Selected, &[], &[], None).unwrap(),
            format!("{PLACEHOLDER_IP}\n")
        );
        let all_excluded = build_ipset_all(
            IpsetMode::Selected,
            &v(&["5.5.5.0/24"]),
            &v(&["5.0.0.0/8"]),
            None,
        )
        .unwrap();
        assert_eq!(all_excluded, format!("{PLACEHOLDER_IP}\n"));
    }

    #[test]
    fn flowseal_mode_uses_the_backup_as_shipped() {
        assert_eq!(
            build_ipset_all(IpsetMode::Flowseal, &v(&["5.5.5.0/24"]), &[], Some(BACKUP)).unwrap(),
            BACKUP
        );
        assert_eq!(
            build_ipset_all(IpsetMode::Flowseal, &[], &[], None),
            Err(IpsetError::NoBackup)
        );
        // Exclusions still win over the backup.
        let out =
            build_ipset_all(IpsetMode::Flowseal, &[], &v(&["8.0.0.0/8"]), Some(BACKUP)).unwrap();
        assert_eq!(out, "1.0.0.0/24\n2001:db8::/32\n");
    }

    #[test]
    fn flowseal_plus_selected_merges_both_sources() {
        let out = build_ipset_all(
            IpsetMode::FlowsealPlusSelected,
            &v(&["5.5.5.0/24", "8.8.8.0/24"]),
            &[],
            Some(BACKUP),
        )
        .unwrap();
        assert_eq!(out, "1.0.0.0/24\n8.8.8.0/24\n2001:db8::/32\n5.5.5.0/24\n");
    }

    fn total(lines: &str) -> u128 {
        lines.lines().filter_map(Net::parse).map(|n| n.size()).sum()
    }

    #[test]
    fn partial_overlap_is_cut_out_of_the_include() {
        let out = build_ipset_all(
            IpsetMode::Selected,
            &v(&["10.0.0.0/8"]),
            &v(&["10.1.0.0/16"]),
            None,
        )
        .unwrap();
        let ex = Net::parse("10.1.0.0/16").unwrap();
        let nets: Vec<Net> = out.lines().map(|l| Net::parse(l).unwrap()).collect();
        assert_eq!(nets.len(), 8, "{out}");
        assert!(nets.iter().all(|n| !n.overlaps(&ex)), "{out}");
        assert!(nets
            .iter()
            .all(|n| n.within(&Net::parse("10.0.0.0/8").unwrap())));
        assert_eq!(total(&out), (1u128 << 24) - (1u128 << 16));
        assert!(out.contains("10.0.0.0/16") && out.contains("10.128.0.0/9"));
    }

    #[test]
    fn partial_overlap_applies_to_the_flowseal_list_and_ipv6() {
        let out = build_ipset_all(
            IpsetMode::Flowseal,
            &[],
            &v(&["8.8.8.8", "2001:db8:1::/48"]),
            Some(BACKUP),
        )
        .unwrap();
        assert!(!out.contains("8.8.8.0/24"));
        assert_eq!(total(&out), 256 + 255 + (1u128 << 96) - (1u128 << 80));
        for l in out.lines() {
            let n = Net::parse(l).unwrap();
            assert!(!n.overlaps(&Net::parse("8.8.8.8").unwrap()));
            assert!(!n.overlaps(&Net::parse("2001:db8:1::/48").unwrap()));
        }
    }

    #[test]
    fn no_output_block_ever_touches_an_exclusion() {
        // Deterministic pseudo-random networks and exclusions.
        let mut x: u64 = 0x9E3779B97F4A7C15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..200 {
            let inc: Vec<String> = (0..3)
                .map(|_| format!("10.{}.0.0/{}", next() % 4, 8 + next() % 12))
                .collect();
            let exc: Vec<String> = (0..3)
                .map(|_| format!("10.{}.{}.0/{}", next() % 4, next() % 256, 12 + next() % 17))
                .collect();
            let out = build_ipset_all(IpsetMode::Selected, &inc, &exc, None).unwrap();
            let ex: Vec<Net> = exc.iter().filter_map(|e| Net::parse(e)).collect();
            for l in out.lines().filter(|l| *l != PLACEHOLDER_IP) {
                let n = Net::parse(l).unwrap();
                assert!(ex.iter().all(|e| !n.overlaps(e)), "{l} vs {exc:?}");
            }
        }
    }

    #[test]
    fn any_mode_stays_an_empty_file_without_exclusions() {
        let out = build_ipset_all(IpsetMode::Any, &v(&["5.5.5.0/24"]), &[], None).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn any_mode_with_exclusions_is_everything_but_them() {
        let out =
            build_ipset_all(IpsetMode::Any, &[], &v(&["10.0.0.0/8", "fd00::/8"]), None).unwrap();
        let v4: Vec<&str> = out.lines().filter(|l| !l.contains(':')).collect();
        assert_eq!(total(&v4.join("\n")), (1u128 << 32) - (1u128 << 24));
        for l in out.lines() {
            let n = Net::parse(l).unwrap();
            assert!(!n.overlaps(&Net::parse("10.0.0.0/8").unwrap()), "{l}");
            assert!(!n.overlaps(&Net::parse("fd00::/8").unwrap()), "{l}");
        }
    }

    #[test]
    fn strategy_ip_use_counts_enabled_ipset_rules() {
        use crate::strategy::{Arg, ArgValue, Rule};
        let list = |opt: &str, l: &str| Arg {
            opt: opt.into(),
            value: Some(ArgValue::List { list: l.into() }),
        };
        let mut s: crate::strategy::Strategy = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1, "id": "x", "name": "x", "source": "custom", "engine": "winws",
            "origin": {}, "global": [], "rules": []
        }))
        .unwrap();
        assert_eq!(strategy_ip_use(&s).ipset_rules, 0);
        s.rules.push(Rule {
            label: None,
            enabled: true,
            args: vec![
                list("--ipset", "ipset-all.txt"),
                list("--ipset-exclude", "ipset-exclude-user.txt"),
            ],
        });
        s.rules.push(Rule {
            label: None,
            enabled: true,
            args: vec![list("--ipset", "ipset-all.txt")],
        });
        s.rules.push(Rule {
            label: None,
            enabled: false,
            args: vec![list("--ipset", "ipset-all.txt")],
        });
        s.rules.push(Rule {
            label: None,
            enabled: true,
            args: vec![list("--hostlist", "list-general.txt")],
        });
        let u = strategy_ip_use(&s);
        assert_eq!(u.ipset_rules, 2);
        assert_eq!(u.ipset_rules_with_user_exclude, 1);
    }

    fn strategy_with_ipset(rules: usize) -> crate::strategy::Strategy {
        use crate::strategy::{Arg, ArgValue, Rule};
        let mut s: crate::strategy::Strategy = serde_json::from_value(serde_json::json!({
            "schemaVersion": 1, "id": "s", "name": "s", "source": "custom", "engine": "winws",
            "origin": {}, "global": [], "rules": []
        }))
        .unwrap();
        for _ in 0..rules {
            s.rules.push(Rule {
                label: None,
                enabled: true,
                args: vec![Arg {
                    opt: "--ipset".into(),
                    value: Some(ArgValue::List {
                        list: "ipset-all.txt".into(),
                    }),
                }],
            });
        }
        s
    }

    #[test]
    fn ip_status_distinguishes_every_step() {
        use crate::lists::{InstalledPack, UserLists};
        let tg = InstalledPack {
            id: "telegram".into(),
            version: None,
            domains: vec![],
            ips: v(&["149.154.160.0/20", "91.108.4.0/22"]),
            title: None,
            previous: None,
        };
        let manual = UserLists::default();
        let with = strategy_with_ipset(1);
        let without = strategy_with_ipset(0);
        // Installed, but the mode ignores selected IPs.
        let st = ip_set_status(
            IpsetMode::None,
            &manual,
            std::slice::from_ref(&tg),
            Some(&with),
            true,
            false,
        );
        assert_eq!((st.prepared, st.applied), (0, false));
        // Prepared, strategy uses them, but the engine is not running it.
        let st = ip_set_status(
            IpsetMode::Selected,
            &manual,
            std::slice::from_ref(&tg),
            Some(&with),
            false,
            false,
        );
        assert_eq!(
            (st.prepared, st.strategy.ipset_rules, st.applied),
            (2, 1, false)
        );
        // Running, but with an older configuration.
        let st = ip_set_status(
            IpsetMode::Selected,
            &manual,
            std::slice::from_ref(&tg),
            Some(&with),
            true,
            true,
        );
        assert!(!st.engine_up_to_date && !st.applied);
        // Strategy without IP rules: never "applied".
        let st = ip_set_status(
            IpsetMode::Selected,
            &manual,
            std::slice::from_ref(&tg),
            Some(&without),
            true,
            false,
        );
        assert_eq!((st.strategy.ipset_rules, st.applied), (0, false));
        // Everything lines up.
        let st = ip_set_status(
            IpsetMode::FlowsealPlusSelected,
            &manual,
            std::slice::from_ref(&tg),
            Some(&with),
            true,
            false,
        );
        assert!(st.applied);
        // All pack IPs excluded: nothing prepared, not applied.
        let ex = UserLists {
            ipset_exclude: v(&["149.154.0.0/16", "91.108.0.0/16"]),
            ..Default::default()
        };
        let st = ip_set_status(
            IpsetMode::Selected,
            &ex,
            std::slice::from_ref(&tg),
            Some(&with),
            true,
            false,
        );
        assert_eq!(
            (st.prepared, st.packs[0].effective, st.applied),
            (0, 0, false)
        );
    }

    #[test]
    fn effective_ip_count_ignores_fully_excluded_entries() {
        assert_eq!(
            effective_ip_count(
                &v(&["1.1.1.0/24", "10.1.0.0/16", "10.0.0.0/8"]),
                &v(&["10.0.0.0/12"])
            ),
            Ok(2)
        );
    }

    /// `n` distinct /32 exclusions spread over IPv4.
    fn many_hosts(n: u32) -> Vec<String> {
        (0..n)
            .map(|i| {
                let a = 1 + (i % 200);
                let b = (i / 200) % 250;
                let c = i / 50_000;
                format!("{a}.{b}.{c}.1")
            })
            .collect()
    }

    /// `n` distinct IPv6 host exclusions: each /128 hole costs up to ~128 blocks.
    fn many_v6_hosts(n: u32) -> Vec<String> {
        (0..n).map(|i| format!("2001:db8:{:x}::1", i)).collect()
    }

    #[test]
    fn any_mode_with_thousands_of_small_exclusions_fails_instead_of_truncating() {
        // 5 000 IPv6 /128 holes would need ~550 000 blocks.
        let r = build_ipset_all(IpsetMode::Any, &[], &many_v6_hosts(5_000), None);
        assert!(
            matches!(
                r,
                Err(IpsetError::TooManyEntries) | Err(IpsetError::TooComplex)
            ),
            "{:?}",
            r.as_ref().map(|s| s.lines().count())
        );
        // A modest number of holes still works and is exact.
        let ok = build_ipset_all(IpsetMode::Any, &[], &many_hosts(100), None).unwrap();
        assert!(ok.lines().count() < MAX_IPSET_ENTRIES);
        let ex: Vec<Net> = many_hosts(100)
            .iter()
            .filter_map(|e| Net::parse(e))
            .collect();
        for l in ok.lines() {
            let n = Net::parse(l).unwrap();
            assert!(ex.iter().all(|e| !n.overlaps(e)));
        }
    }

    #[test]
    fn subtraction_work_is_bounded() {
        let ex: Vec<Net> = many_v6_hosts(5_000)
            .iter()
            .filter_map(|e| Net::parse(e))
            .collect();
        let mut b = Budget::default();
        let r = subtract(Net::parse("::/0").unwrap(), &ex, &mut b);
        assert!(matches!(
            r,
            Err(IpsetError::TooManyEntries) | Err(IpsetError::TooComplex)
        ));
        // The limits are errors with an actionable message, not a silent cut.
        assert!(IpsetError::TooManyEntries.to_string().contains("200 000"));
    }

    #[test]
    fn the_real_flowseal_list_fits_the_limits() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/flowseal/1.10.4/lists/ipset-all.txt.backup"
        );
        let Ok(backup) = std::fs::read_to_string(path) else {
            return; // the pinned release is fetched in CI; skip when absent
        };
        let excl = v(&["10.0.0.0/8", "192.168.0.0/16", "172.16.0.0/12"]);
        let out = build_ipset_all(
            IpsetMode::FlowsealPlusSelected,
            &v(&["149.154.160.0/20"]),
            &excl,
            Some(&backup),
        )
        .unwrap();
        assert!(out.lines().count() >= 1000);
    }

    #[test]
    fn status_reports_a_limit_error() {
        use crate::lists::UserLists;
        let manual = UserLists {
            ipset_include: v(&["2001:db8::/32"]),
            ipset_exclude: many_v6_hosts(5_000),
            ..Default::default()
        };
        let st = ip_set_status(
            IpsetMode::Selected,
            &manual,
            &[],
            Some(&strategy_with_ipset(1)),
            true,
            false,
        );
        assert!(st.error.is_some());
        assert_eq!(st.prepared, 0);
        assert!(!st.applied);
    }

    #[test]
    fn old_mode_names_still_load() {
        let m: IpsetMode = serde_json::from_str("\"loaded\"").unwrap();
        assert_eq!(m, IpsetMode::Flowseal);
        assert_eq!(
            serde_json::to_string(&IpsetMode::FlowsealPlusSelected).unwrap(),
            "\"flowsealPlusSelected\""
        );
        for (s, m) in [
            ("\"none\"", IpsetMode::None),
            ("\"any\"", IpsetMode::Any),
            ("\"selected\"", IpsetMode::Selected),
        ] {
            assert_eq!(serde_json::from_str::<IpsetMode>(s).unwrap(), m);
        }
    }
}
