//! Strategy comparison: a structured per-section diff plus an LCS-based
//! argument diff that keeps rule order visible.

use crate::ports::GameFilterPorts;
use crate::strategy::{summarize_rule, Arg, ArgValue, Strategy};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Same,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub op: Op,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChange {
    pub field: String,
    pub left: String,
    pub right: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleDiff {
    /// 1-based index in the left / right strategy (None when added/removed).
    pub left_index: Option<usize>,
    pub right_index: Option<usize>,
    pub changes: Vec<FieldChange>,
    pub args: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyDiff {
    pub identical: bool,
    pub meta: Vec<FieldChange>,
    pub global: Vec<DiffLine>,
    pub rules: Vec<RuleDiff>,
    /// Rules appear in a different relative order.
    pub order_changed: bool,
}

fn arg_text(a: &Arg) -> String {
    match &a.value {
        None => a.opt.clone(),
        Some(ArgValue::Text(t)) => format!("{}={t}", a.opt),
        Some(ArgValue::Fake { fake }) => format!("{}=bin/{fake}", a.opt),
        Some(ArgValue::List { list }) => format!("{}=lists/{list}", a.opt),
    }
}

/// Classic LCS diff over strings (inputs are small: dozens of items).
pub fn lcs_diff(a: &[String], b: &[String]) -> Vec<DiffLine> {
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if a[i] == b[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(DiffLine {
                op: Op::Same,
                text: a[i].clone(),
            });
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push(DiffLine {
                op: Op::Removed,
                text: a[i].clone(),
            });
            i += 1;
        } else {
            out.push(DiffLine {
                op: Op::Added,
                text: b[j].clone(),
            });
            j += 1;
        }
    }
    out.extend(a[i..].iter().map(|t| DiffLine {
        op: Op::Removed,
        text: t.clone(),
    }));
    out.extend(b[j..].iter().map(|t| DiffLine {
        op: Op::Added,
        text: t.clone(),
    }));
    out
}

fn opt_s(v: &Option<String>) -> String {
    v.clone().unwrap_or_else(|| "—".into())
}

fn rule_key(s: &Strategy, i: usize) -> String {
    let r = &s.rules[i];
    let sum = summarize_rule(r);
    format!(
        "{}|{}|{}|{}",
        sum.protocol,
        opt_s(&sum.tcp_ports),
        opt_s(&sum.udp_ports),
        sum.lists.join(",")
    )
}

pub fn diff(left: &Strategy, right: &Strategy, _gf: &GameFilterPorts) -> StrategyDiff {
    let mut meta = Vec::new();
    let mut field = |f: &str, l: String, r: String| {
        if l != r {
            meta.push(FieldChange {
                field: f.into(),
                left: l,
                right: r,
            });
        }
    };
    field("name", left.name.clone(), right.name.clone());
    field(
        "description",
        left.description.clone(),
        right.description.clone(),
    );
    field(
        "tcpTimestamps",
        left.requirements.tcp_timestamps.to_string(),
        right.requirements.tcp_timestamps.to_string(),
    );

    let g = lcs_diff(
        &left.global.iter().map(arg_text).collect::<Vec<_>>(),
        &right.global.iter().map(arg_text).collect::<Vec<_>>(),
    );

    // Align rules by a coarse key (protocol, ports, lists) using LCS, then
    // pair leftover removed/added rules positionally as "modified".
    let lk: Vec<String> = (0..left.rules.len()).map(|i| rule_key(left, i)).collect();
    let rk: Vec<String> = (0..right.rules.len()).map(|i| rule_key(right, i)).collect();
    let mut pairs: Vec<(Option<usize>, Option<usize>)> = Vec::new();
    {
        let (n, m) = (lk.len(), rk.len());
        let mut dp = vec![vec![0u32; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i][j] = if lk[i] == rk[j] {
                    dp[i + 1][j + 1] + 1
                } else {
                    dp[i + 1][j].max(dp[i][j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        let mut pend_l: Vec<usize> = Vec::new();
        let mut pend_r: Vec<usize> = Vec::new();
        let flush = |pairs: &mut Vec<(Option<usize>, Option<usize>)>,
                     pl: &mut Vec<usize>,
                     pr: &mut Vec<usize>| {
            let k = pl.len().max(pr.len());
            for x in 0..k {
                pairs.push((pl.get(x).copied(), pr.get(x).copied()));
            }
            pl.clear();
            pr.clear();
        };
        while i < n && j < m {
            if lk[i] == rk[j] {
                flush(&mut pairs, &mut pend_l, &mut pend_r);
                pairs.push((Some(i), Some(j)));
                i += 1;
                j += 1;
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                pend_l.push(i);
                i += 1;
            } else {
                pend_r.push(j);
                j += 1;
            }
        }
        pend_l.extend(i..n);
        pend_r.extend(j..m);
        flush(&mut pairs, &mut pend_l, &mut pend_r);
    }

    let mut rules = Vec::new();
    for (li, ri) in &pairs {
        let la: Vec<String> = li
            .map(|i| left.rules[i].args.iter().map(arg_text).collect())
            .unwrap_or_default();
        let ra: Vec<String> = ri
            .map(|i| right.rules[i].args.iter().map(arg_text).collect())
            .unwrap_or_default();
        let args = lcs_diff(&la, &ra);
        let mut changes = Vec::new();
        let ls = li
            .map(|i| summarize_rule(&left.rules[i]))
            .unwrap_or_default();
        let rs = ri
            .map(|i| summarize_rule(&right.rules[i]))
            .unwrap_or_default();
        let mut ch = |f: &str, l: String, r: String| {
            if l != r {
                changes.push(FieldChange {
                    field: f.into(),
                    left: l,
                    right: r,
                });
            }
        };
        ch("tcp", opt_s(&ls.tcp_ports), opt_s(&rs.tcp_ports));
        ch("udp", opt_s(&ls.udp_ports), opt_s(&rs.udp_ports));
        ch("l7", opt_s(&ls.l7), opt_s(&rs.l7));
        ch("lists", ls.lists.join(", "), rs.lists.join(", "));
        ch("domains", ls.domains.join(", "), rs.domains.join(", "));
        ch("fakes", ls.fakes.join(", "), rs.fakes.join(", "));
        ch("desync", opt_s(&ls.desync), opt_s(&rs.desync));
        ch("params", ls.other.join(" "), rs.other.join(" "));
        let le = li.map(|i| left.rules[i].enabled);
        let re = ri.map(|i| right.rules[i].enabled);
        if let (Some(a), Some(b)) = (le, re) {
            ch("enabled", a.to_string(), b.to_string());
        }
        // Identical rules are kept (with no changes) so the UI can show order.
        rules.push(RuleDiff {
            left_index: li.map(|i| i + 1),
            right_index: ri.map(|i| i + 1),
            changes,
            args,
        });
    }
    let matched: Vec<(usize, usize)> = pairs.iter().filter_map(|p| Some((p.0?, p.1?))).collect();
    let order_changed = matched.windows(2).any(|w| w[1].1 < w[0].1)
        || (left.rules.len() == right.rules.len() && lk != rk && {
            let mut a = lk.clone();
            let mut b = rk.clone();
            a.sort();
            b.sort();
            a == b
        });
    let identical = meta.is_empty()
        && g.iter().all(|d| d.op == Op::Same)
        && rules.iter().all(|r| {
            r.left_index.is_some()
                && r.right_index.is_some()
                && r.changes.is_empty()
                && r.args.iter().all(|d| d.op == Op::Same)
        });
    StrategyDiff {
        identical,
        meta,
        global: g,
        rules,
        order_changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::GameFilter;
    use crate::strategy::{Arg, Rule};

    #[test]
    fn lcs_basic() {
        let a: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let b: Vec<String> = ["a", "c", "d"].iter().map(|s| s.to_string()).collect();
        let d = lcs_diff(&a, &b);
        let ops: Vec<Op> = d.iter().map(|x| x.op).collect();
        assert_eq!(ops, vec![Op::Same, Op::Removed, Op::Same, Op::Added]);
    }

    #[test]
    fn identical_and_changed() {
        let gf = GameFilter::default().ports();
        let a = Strategy::new_empty("a", "A");
        assert!(diff(&a, &a, &gf).identical);
        let mut b = a.clone();
        b.rules[0].args[2] = Arg::text("--dpi-desync", "fake");
        let d = diff(&a, &b, &gf);
        assert!(!d.identical);
        assert_eq!(d.rules.len(), 1);
        assert!(d.rules[0].changes.iter().any(|c| c.field == "desync"));
    }

    #[test]
    fn detects_reorder() {
        let gf = GameFilter::default().ports();
        let mut a = Strategy::new_empty("a", "A");
        a.rules.push(Rule {
            label: None,
            enabled: true,
            args: vec![
                Arg::text("--filter-udp", "443"),
                Arg::text("--dpi-desync", "fake"),
            ],
        });
        let mut b = a.clone();
        b.rules.swap(0, 1);
        let d = diff(&a, &b, &gf);
        assert!(d.order_changed);
        assert!(!d.identical);
    }
}
