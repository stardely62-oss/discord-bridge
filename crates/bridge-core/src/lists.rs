//! User lists and Flowseal list modes.
//!
//! Flowseal strategies reference `list-general-user.txt`,
//! `list-exclude-user.txt` and `ipset-exclude-user.txt`, created by
//! `service.bat load_user_lists` with placeholder content when missing, and
//! `ipset-all.txt` whose content depends on the "IPSet" switch
//! (`none` / `any` / `loaded`). The helper materialises these files in its
//! protected runtime directory from validated content.

use serde::{Deserialize, Serialize};

pub const USER_GENERAL: &str = "list-general-user.txt";
pub const USER_EXCLUDE: &str = "list-exclude-user.txt";
pub const USER_IPSET_EXCLUDE: &str = "ipset-exclude-user.txt";
pub const IPSET_ALL: &str = "ipset-all.txt";
pub const IPSET_ALL_BACKUP: &str = "ipset-all.txt.backup";

/// Lists that are generated at runtime rather than read from the release.
pub const RUNTIME_LISTS: &[&str] = &[USER_GENERAL, USER_EXCLUDE, USER_IPSET_EXCLUDE, IPSET_ALL];

/// Same placeholder Flowseal writes: winws refuses empty host lists.
pub const PLACEHOLDER_DOMAIN: &str = "domain.example.abc";
/// TEST-NET-3 address Flowseal uses as an inert ipset entry.
pub const PLACEHOLDER_IP: &str = "203.0.113.113/32";

pub const MAX_USER_LIST_LINES: usize = 10_000;

/// Moved to [`crate::ipset`]; re-exported for existing users.
pub use crate::ipset::IpsetMode;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UserLists {
    #[serde(default)]
    pub general: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub ipset_exclude: Vec<String>,
    /// "IP through the bypass": used in the `selected` and
    /// `flowsealPlusSelected` ipset modes (see `crate::ipset`).
    #[serde(default)]
    pub ipset_include: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ListError {
    #[error("строка {line} списка «{list}»: «{value}» не является доменом")]
    BadDomain {
        list: &'static str,
        line: usize,
        value: String,
    },
    #[error("строка {line} списка «{list}»: «{value}» не является IP-адресом или подсетью")]
    BadCidr {
        list: &'static str,
        line: usize,
        value: String,
    },
    #[error("список «{0}» слишком длинный")]
    TooLong(&'static str),
    #[error("набор каталога «{0}» повторяется или не имеет идентификатора")]
    BadPack(String),
    #[error("строка {line} списка «{list}»: «{value}» — {reason}")]
    BadInclude {
        list: &'static str,
        line: usize,
        value: String,
        reason: &'static str,
    },
}

pub(crate) fn domain_ok(d: &str) -> bool {
    let d = d.strip_prefix('^').unwrap_or(d);
    !d.is_empty()
        && d.len() <= 253
        && d.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
}

fn cidr_ok(v: &str) -> bool {
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
    mask.is_none_or(|m| m.parse::<u8>().is_ok_and(|m| m <= max))
}

fn clean(lines: &[String]) -> impl Iterator<Item = (usize, &str)> {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty() && !l.starts_with('#'))
}

impl UserLists {
    pub fn validate(&self) -> Result<(), ListError> {
        for (name, list) in [(USER_GENERAL, &self.general), (USER_EXCLUDE, &self.exclude)] {
            if list.len() > MAX_USER_LIST_LINES {
                return Err(ListError::TooLong(name));
            }
            for (n, l) in clean(list) {
                if !domain_ok(l) {
                    return Err(ListError::BadDomain {
                        list: name,
                        line: n,
                        value: l.chars().take(80).collect(),
                    });
                }
            }
        }
        if self.ipset_exclude.len() > MAX_USER_LIST_LINES {
            return Err(ListError::TooLong(USER_IPSET_EXCLUDE));
        }
        for (n, l) in clean(&self.ipset_exclude) {
            if !cidr_ok(l) {
                return Err(ListError::BadCidr {
                    list: USER_IPSET_EXCLUDE,
                    line: n,
                    value: l.chars().take(80).collect(),
                });
            }
        }
        if self.ipset_include.len() > MAX_USER_LIST_LINES {
            return Err(ListError::TooLong(USER_IPSET_INCLUDE));
        }
        check_include(USER_IPSET_INCLUDE, &self.ipset_include)
    }
}

/// Display name of the "IP through the bypass" list in errors.
pub const USER_IPSET_INCLUDE: &str = "IP через обход";

fn check_include(list: &'static str, lines: &[String]) -> Result<(), ListError> {
    for (n, l) in clean(lines) {
        if let Some(reason) = crate::ipset::include_entry_error(l) {
            return Err(ListError::BadInclude {
                list,
                line: n,
                value: l.chars().take(80).collect(),
                reason,
            });
        }
    }
    Ok(())
}

impl UserLists {
    /// File contents as the helper writes them (never empty).
    pub fn render(&self) -> [(&'static str, String); 3] {
        let join = |list: &Vec<String>, placeholder: &str| {
            let body: Vec<&str> = clean(list).map(|(_, l)| l).collect();
            if body.is_empty() {
                format!("{placeholder}\n")
            } else {
                format!("{}\n", body.join("\n"))
            }
        };
        [
            (USER_GENERAL, join(&self.general, PLACEHOLDER_DOMAIN)),
            (USER_EXCLUDE, join(&self.exclude, PLACEHOLDER_DOMAIN)),
            (
                USER_IPSET_EXCLUDE,
                join(&self.ipset_exclude, PLACEHOLDER_IP),
            ),
        ]
    }
}

/// A catalog pack installed by the user. Its domains are stored apart from
/// the user's own lists (`UserLists`), so removing a pack never touches
/// entries the user typed, and two packs may share a domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPack {
    pub id: String,
    /// Catalog revision the domains were taken from (for future update checks).
    #[serde(default)]
    pub version: Option<String>,
    /// Snapshot of the pack's domains at install time.
    #[serde(default)]
    pub domains: Vec<String>,
    /// Snapshot of the pack's IP addresses / subnets (IPv4 and IPv6). They
    /// take effect only in the `selected` / `flowsealPlusSelected` modes.
    #[serde(default)]
    pub ips: Vec<String>,
    /// Display name at install time, so a pack later removed from the
    /// catalog stays recognisable (RU / EN).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<crate::packs::Localized>,
    /// The verified snapshot this one replaced (for rollback). Never used
    /// for the engine configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<PackSnapshot>,
}

/// A previous verified version of an installed pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PackSnapshot {
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub ips: Vec<String>,
}

pub const MAX_INSTALLED_PACKS: usize = 200;

/// The lists the engine actually gets: the user's own entries first, then the
/// domains of every installed pack that are not already present
/// (case-insensitive). Exclusions are the user's only.
pub fn effective_lists(manual: &UserLists, packs: &[InstalledPack]) -> UserLists {
    let mut seen: std::collections::HashSet<String> = clean(&manual.general)
        .map(|(_, l)| l.to_ascii_lowercase())
        .collect();
    let mut general = manual.general.clone();
    for d in packs.iter().flat_map(|p| p.domains.iter()) {
        let t = d.trim();
        if !t.is_empty() && !t.starts_with('#') && seen.insert(t.to_ascii_lowercase()) {
            general.push(t.to_string());
        }
    }
    // IPs: the user's entries first, then pack IPs not already present
    // (compared as networks, so 1.2.3.4 and 1.2.3.4/32 are one entry).
    let mut nets: std::collections::HashSet<crate::ipset::Net> = clean(&manual.ipset_include)
        .filter_map(|(_, l)| crate::ipset::Net::parse(l))
        .collect();
    let mut ipset_include = manual.ipset_include.clone();
    for ip in packs.iter().flat_map(|p| p.ips.iter()) {
        let t = ip.trim();
        if let Some(n) = crate::ipset::Net::parse(t) {
            if nets.insert(n) {
                ipset_include.push(t.to_string());
            }
        }
    }
    UserLists {
        general,
        exclude: manual.exclude.clone(),
        ipset_exclude: manual.ipset_exclude.clone(),
        ipset_include,
    }
}

/// Installed packs must have distinct ids and valid domains; the merged list
/// must also fit the engine limits.
pub fn validate_packs(manual: &UserLists, packs: &[InstalledPack]) -> Result<(), ListError> {
    if packs.len() > MAX_INSTALLED_PACKS {
        return Err(ListError::TooLong("наборы каталога"));
    }
    let mut ids = std::collections::HashSet::new();
    for p in packs {
        if p.id.is_empty() || !ids.insert(p.id.as_str()) {
            return Err(ListError::BadPack(p.id.chars().take(80).collect()));
        }
        for (n, l) in clean(&p.domains) {
            if !domain_ok(l) {
                return Err(ListError::BadDomain {
                    list: "набор каталога",
                    line: n,
                    value: l.chars().take(80).collect(),
                });
            }
        }
        if p.ips.len() > MAX_USER_LIST_LINES {
            return Err(ListError::TooLong("IP набора каталога"));
        }
        check_include("IP набора каталога", &p.ips)?;
        if let Some(prev) = &p.previous {
            if prev.domains.len() > MAX_USER_LIST_LINES || prev.ips.len() > MAX_USER_LIST_LINES {
                return Err(ListError::TooLong("предыдущая версия набора"));
            }
            for (n, l) in clean(&prev.domains) {
                if !domain_ok(l) {
                    return Err(ListError::BadDomain {
                        list: "предыдущая версия набора",
                        line: n,
                        value: l.chars().take(80).collect(),
                    });
                }
            }
            check_include("IP предыдущей версии набора", &prev.ips)?;
        }
    }
    effective_lists(manual, packs).validate()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(id: &str, d: &[&str]) -> InstalledPack {
        InstalledPack {
            id: id.into(),
            version: Some("1".into()),
            domains: d.iter().map(|s| s.to_string()).collect(),
            ips: vec![],
            title: None,
            previous: None,
        }
    }

    #[test]
    fn packs_merge_without_touching_manual_entries() {
        let manual = UserLists {
            general: vec!["Steampowered.com".into(), "example.org".into()],
            ..Default::default()
        };
        let steam = pack("steam", &["steampowered.com", "steamcommunity.com"]);
        let eff = effective_lists(&manual, std::slice::from_ref(&steam));
        assert_eq!(
            eff.general,
            vec!["Steampowered.com", "example.org", "steamcommunity.com"]
        );
        // Removing the pack = dropping it from the installed list: manual entry stays.
        assert_eq!(effective_lists(&manual, &[]).general, manual.general);
    }

    #[test]
    fn shared_domain_survives_removal_of_one_pack() {
        let riot = pack("riot", &["riotgames.com", "pvp.net"]);
        let valorant = pack("valorant", &["playvalorant.com", "riotgames.com"]);
        let both = effective_lists(&UserLists::default(), &[riot, valorant.clone()]);
        assert_eq!(
            both.general
                .iter()
                .filter(|d| *d == "riotgames.com")
                .count(),
            1
        );
        let only_valorant = effective_lists(&UserLists::default(), &[valorant]);
        assert!(only_valorant.general.contains(&"riotgames.com".to_string()));
        assert!(!only_valorant.general.contains(&"pvp.net".to_string()));
    }

    #[test]
    fn packs_are_validated() {
        let m = UserLists::default();
        assert!(validate_packs(&m, &[pack("a", &["ok.com"])]).is_ok());
        assert!(validate_packs(&m, &[pack("a", &["bad domain"])]).is_err());
        assert!(validate_packs(&m, &[pack("a", &["x.com"]), pack("a", &["y.com"])]).is_err());
        assert!(validate_packs(&m, &[pack("", &["x.com"])]).is_err());
    }

    #[test]
    fn validates_and_renders() {
        let l = UserLists {
            general: vec!["# c".into(), "example.com".into(), "^dns.google".into()],
            exclude: vec![],
            ipset_exclude: vec!["10.0.0.0/8".into()],
            ipset_include: vec![],
        };
        l.validate().unwrap();
        let r = l.render();
        assert_eq!(r[0].1, "example.com\n^dns.google\n");
        assert_eq!(r[1].1, "domain.example.abc\n");
        assert_eq!(r[2].1, "10.0.0.0/8\n");
    }

    #[test]
    fn rejects_bad_entries() {
        let l = UserLists {
            general: vec!["evil\"--x".into()],
            ..Default::default()
        };
        assert!(l.validate().is_err());
        let l = UserLists {
            ipset_exclude: vec!["1.2.3.4/40".into()],
            ..Default::default()
        };
        assert!(l.validate().is_err());
    }

    fn ip_pack(id: &str, ips: &[&str]) -> InstalledPack {
        InstalledPack {
            id: id.into(),
            version: None,
            domains: vec![],
            ips: ips.iter().map(|s| s.to_string()).collect(),
            title: None,
            previous: None,
        }
    }

    #[test]
    fn manual_and_pack_ips_merge_into_include() {
        let manual = UserLists {
            ipset_include: vec!["5.5.5.0/24".into()],
            ..Default::default()
        };
        let tg = ip_pack("telegram", &["149.154.160.0/20", "5.5.5.9/24"]);
        let eff = effective_lists(&manual, std::slice::from_ref(&tg));
        assert_eq!(eff.ipset_include, vec!["5.5.5.0/24", "149.154.160.0/20"]);
        // Removing the pack keeps the manual IP.
        assert_eq!(
            effective_lists(&manual, &[]).ipset_include,
            vec!["5.5.5.0/24"]
        );
    }

    #[test]
    fn shared_ip_survives_removal_of_one_pack() {
        let a = ip_pack("a", &["91.108.4.0/22", "1.1.1.0/24"]);
        let b = ip_pack("b", &["91.108.4.0/22"]);
        let both = effective_lists(&UserLists::default(), &[a, b.clone()]);
        assert_eq!(both.ipset_include, vec!["91.108.4.0/22", "1.1.1.0/24"]);
        let only_b = effective_lists(&UserLists::default(), &[b]);
        assert_eq!(only_b.ipset_include, vec!["91.108.4.0/22"]);
    }

    #[test]
    fn include_lists_reject_bad_or_too_wide_networks() {
        for bad in ["nonsense", "1.2.3.4/33", "0.0.0.0/0", "::/0"] {
            let l = UserLists {
                ipset_include: vec![bad.into()],
                ..Default::default()
            };
            assert!(l.validate().is_err(), "{bad}");
            assert!(
                validate_packs(&UserLists::default(), &[ip_pack("p", &[bad])]).is_err(),
                "{bad}"
            );
        }
        let ok = UserLists {
            ipset_include: vec!["# telegram".into(), "2001:b28:f23d::/48".into()],
            ..Default::default()
        };
        ok.validate().unwrap();
    }

    #[test]
    fn old_settings_without_ip_fields_load() {
        let l: UserLists =
            serde_json::from_str(r#"{"general":["a.com"],"ipsetExclude":["10.0.0.0/8"]}"#).unwrap();
        assert!(l.ipset_include.is_empty());
        let p: InstalledPack =
            serde_json::from_str(r#"{"id":"steam","domains":["a.com"]}"#).unwrap();
        assert!(p.ips.is_empty());
    }
}

/// Every runtime list file the helper writes into a generation directory:
/// the three user lists and `ipset-all.txt`. Pure function so the result is
/// tested on every platform; the helper only writes it.
pub fn runtime_list_files(
    lists: &UserLists,
    mode: IpsetMode,
    backup_ipset: Option<&str>,
) -> Result<Vec<(&'static str, String)>, String> {
    lists.validate().map_err(|e| e.to_string())?;
    let mut files: Vec<(&'static str, String)> = lists.render().into_iter().collect();
    let ipset = crate::ipset::build_ipset_all(
        mode,
        &lists.ipset_include,
        &lists.ipset_exclude,
        backup_ipset,
    )
    .map_err(|e| e.to_string())?;
    files.push((IPSET_ALL, ipset));
    Ok(files)
}

#[cfg(test)]
mod runtime_tests {
    use super::*;

    fn get<'a>(f: &'a [(&'static str, String)], name: &str) -> &'a str {
        &f.iter().find(|(n, _)| *n == name).unwrap().1
    }

    #[test]
    fn default_runtime_is_unchanged() {
        // Same files as before IP sets existed: placeholders everywhere.
        let f = runtime_list_files(&UserLists::default(), IpsetMode::None, None).unwrap();
        assert_eq!(f.len(), 4);
        assert_eq!(get(&f, USER_GENERAL), "domain.example.abc\n");
        assert_eq!(get(&f, USER_EXCLUDE), "domain.example.abc\n");
        assert_eq!(get(&f, USER_IPSET_EXCLUDE), "203.0.113.113/32\n");
        assert_eq!(get(&f, IPSET_ALL), "203.0.113.113/32\n");
        let any = runtime_list_files(&UserLists::default(), IpsetMode::Any, None).unwrap();
        assert_eq!(get(&any, IPSET_ALL), "");
        let fs = runtime_list_files(
            &UserLists::default(),
            IpsetMode::Flowseal,
            Some("1.0.0.0/24\n"),
        )
        .unwrap();
        assert_eq!(get(&fs, IPSET_ALL), "1.0.0.0/24\n");
        assert!(runtime_list_files(&UserLists::default(), IpsetMode::Flowseal, None).is_err());
    }

    #[test]
    fn pack_ips_reach_ipset_all_only_in_selected_modes() {
        let pack = InstalledPack {
            id: "telegram".into(),
            version: None,
            domains: vec!["telegram.org".into()],
            ips: vec!["149.154.160.0/20".into()],
            title: None,
            previous: None,
        };
        let eff = effective_lists(&UserLists::default(), &[pack]);
        let sel = runtime_list_files(&eff, IpsetMode::Selected, None).unwrap();
        assert_eq!(get(&sel, IPSET_ALL), "149.154.160.0/20\n");
        assert_eq!(get(&sel, USER_GENERAL), "telegram.org\n");
        let none = runtime_list_files(&eff, IpsetMode::None, None).unwrap();
        assert_eq!(get(&none, IPSET_ALL), "203.0.113.113/32\n");
    }

    #[test]
    fn invalid_lists_are_not_materialised() {
        let bad = UserLists {
            ipset_include: vec!["0.0.0.0/0".into()],
            ..Default::default()
        };
        assert!(runtime_list_files(&bad, IpsetMode::Selected, None).is_err());
    }
}
