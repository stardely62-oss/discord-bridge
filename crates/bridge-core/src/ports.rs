//! Port specifications for `--wf-tcp`, `--wf-udp`, `--filter-tcp`, `--filter-udp`.
//!
//! Flowseal strategies reference the game filter through the batch variables
//! `%GameFilterTCP%` / `%GameFilterUDP%`. They are kept symbolic in the schema
//! and substituted at render time from the user's game filter setting.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortVar {
    GameFilterTcp,
    GameFilterUdp,
}

impl PortVar {
    pub fn token(self) -> &'static str {
        match self {
            PortVar::GameFilterTcp => "%GameFilterTCP%",
            PortVar::GameFilterUdp => "%GameFilterUDP%",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PortItem {
    Single(u16),
    Range(u16, u16),
    Var(PortVar),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct PortSpec(pub Vec<PortItem>);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PortError {
    #[error("пустой список портов")]
    Empty,
    #[error("некорректный порт или диапазон: «{0}»")]
    Invalid(String),
    #[error("начало диапазона больше конца: «{0}»")]
    Reversed(String),
    #[error("слишком много элементов в списке портов")]
    TooMany,
}

fn parse_port(s: &str) -> Option<u16> {
    if s.is_empty() || s.len() > 5 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u32>()
        .ok()
        .filter(|v| *v <= 65535)
        .map(|v| v as u16)
}

impl PortSpec {
    pub fn parse(s: &str) -> Result<Self, PortError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(PortError::Empty);
        }
        let mut items = Vec::new();
        for raw in s.split(',') {
            let part = raw.trim();
            if items.len() >= 256 {
                return Err(PortError::TooMany);
            }
            if part.eq_ignore_ascii_case("%GameFilterTCP%") {
                items.push(PortItem::Var(PortVar::GameFilterTcp));
            } else if part.eq_ignore_ascii_case("%GameFilterUDP%") {
                items.push(PortItem::Var(PortVar::GameFilterUdp));
            } else if let Some((a, b)) = part.split_once('-') {
                let (a, b) = (
                    parse_port(a).ok_or_else(|| PortError::Invalid(part.into()))?,
                    parse_port(b).ok_or_else(|| PortError::Invalid(part.into()))?,
                );
                if a > b {
                    return Err(PortError::Reversed(part.into()));
                }
                items.push(PortItem::Range(a, b));
            } else {
                items.push(PortItem::Single(
                    parse_port(part).ok_or_else(|| PortError::Invalid(part.into()))?,
                ));
            }
        }
        Ok(PortSpec(items))
    }

    /// Concrete port ranges, or `None` while game-filter variables remain.
    pub fn ranges(&self) -> Option<Vec<(u16, u16)>> {
        self.0
            .iter()
            .map(|i| match i {
                PortItem::Single(p) => Some((*p, *p)),
                PortItem::Range(a, b) => Some((*a, *b)),
                PortItem::Var(_) => None,
            })
            .collect()
    }

    pub fn vars(&self) -> impl Iterator<Item = PortVar> + '_ {
        self.0.iter().filter_map(|i| match i {
            PortItem::Var(v) => Some(*v),
            _ => None,
        })
    }

    /// Render with game-filter variables substituted.
    pub fn render(&self, gf: &GameFilterPorts) -> String {
        self.0
            .iter()
            .map(|i| match i {
                PortItem::Single(p) => p.to_string(),
                PortItem::Range(a, b) => format!("{a}-{b}"),
                PortItem::Var(PortVar::GameFilterTcp) => gf.tcp.clone(),
                PortItem::Var(PortVar::GameFilterUdp) => gf.udp.clone(),
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl fmt::Display for PortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self
            .0
            .iter()
            .map(|i| match i {
                PortItem::Single(p) => p.to_string(),
                PortItem::Range(a, b) => format!("{a}-{b}"),
                PortItem::Var(v) => v.token().to_string(),
            })
            .collect::<Vec<_>>()
            .join(",");
        f.write_str(&s)
    }
}

/// Whether every port of `need` lies in `have` (ranges may overlap or touch).
pub fn covers_all(have: &[(u16, u16)], need: &[(u16, u16)]) -> bool {
    let mut merged: Vec<(u32, u32)> = have.iter().map(|&(a, b)| (a as u32, b as u32)).collect();
    merged.sort_unstable();
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(merged.len());
    for (a, b) in merged {
        match out.last_mut() {
            Some(last) if a <= last.1 + 1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    need.iter()
        .all(|&(a, b)| out.iter().any(|&(x, y)| x <= a as u32 && b as u32 <= y))
}

/// Game filter mode, mirroring Flowseal's `utils\game_filter.enabled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GameFilterMode {
    #[default]
    Disabled,
    All,
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFilter {
    pub mode: GameFilterMode,
    pub tcp_range: String,
    pub udp_range: String,
}

impl Default for GameFilter {
    fn default() -> Self {
        Self {
            mode: GameFilterMode::Disabled,
            tcp_range: "1024-65535".into(),
            udp_range: "1024-65535".into(),
        }
    }
}

/// Concrete strings substituted for the game filter variables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFilterPorts {
    pub tcp: String,
    pub udp: String,
}

/// Flowseal uses port 12 as an inert placeholder when the filter is disabled.
pub const GAME_FILTER_PLACEHOLDER: &str = "12";

impl GameFilter {
    /// Validates ranges with the same rules as Flowseal's `validate_game_filter_range`.
    pub fn validate(&self) -> Result<(), PortError> {
        for r in [&self.tcp_range, &self.udp_range] {
            let spec = PortSpec::parse(r)?;
            if spec.vars().next().is_some() {
                return Err(PortError::Invalid(r.clone()));
            }
            for item in &spec.0 {
                match item {
                    PortItem::Single(0) | PortItem::Range(0, _) => {
                        return Err(PortError::Invalid(r.clone()))
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub fn ports(&self) -> GameFilterPorts {
        let ph = GAME_FILTER_PLACEHOLDER.to_string();
        match self.mode {
            GameFilterMode::Disabled => GameFilterPorts {
                tcp: ph.clone(),
                udp: ph,
            },
            GameFilterMode::All => GameFilterPorts {
                tcp: self.tcp_range.clone(),
                udp: self.udp_range.clone(),
            },
            GameFilterMode::Tcp => GameFilterPorts {
                tcp: self.tcp_range.clone(),
                udp: ph,
            },
            GameFilterMode::Udp => GameFilterPorts {
                tcp: ph,
                udp: self.udp_range.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_render() {
        let p = PortSpec::parse("80,443,2053-2096,%GameFilterTCP%").unwrap();
        assert_eq!(p.to_string(), "80,443,2053-2096,%GameFilterTCP%");
        let gf = GameFilter::default();
        assert_eq!(p.render(&gf.ports()), "80,443,2053-2096,12");
        let gf = GameFilter {
            mode: GameFilterMode::All,
            ..Default::default()
        };
        assert_eq!(p.render(&gf.ports()), "80,443,2053-2096,1024-65535");
    }

    #[test]
    fn rejects_garbage() {
        assert!(PortSpec::parse("").is_err());
        assert!(PortSpec::parse("70000").is_err());
        assert!(PortSpec::parse("10-5").is_err());
        assert!(PortSpec::parse("80;calc").is_err());
        assert!(PortSpec::parse("%PATH%").is_err());
        assert!(PortSpec::parse("1,,2").is_err());
    }

    #[test]
    fn game_filter_validation() {
        let mut gf = GameFilter::default();
        assert!(gf.validate().is_ok());
        gf.tcp_range = "1024-1934,1936-65535".into();
        assert!(gf.validate().is_ok());
        gf.tcp_range = "%GameFilterTCP%".into();
        assert!(gf.validate().is_err());
        gf.tcp_range = "0-10".into();
        assert!(gf.validate().is_err());
    }

    #[test]
    fn ranges_and_coverage() {
        let r = |s: &str| PortSpec::parse(s).unwrap().ranges().unwrap();
        assert_eq!(r("443,50000-50100"), vec![(443, 443), (50000, 50100)]);
        assert!(PortSpec::parse("443,%GameFilterUDP%")
            .unwrap()
            .ranges()
            .is_none());
        // Touching and overlapping ranges merge.
        assert!(covers_all(&r("1024-5000,5001-65535"), &r("5055-5058")));
        assert!(covers_all(&r("5055,5056-5057,5058"), &r("5055-5058")));
        assert!(!covers_all(&r("5055-5057"), &r("5055-5058")));
        assert!(!covers_all(&r("12"), &r("5055-5058")));
        assert!(covers_all(&r("65535"), &r("65535")));
        assert!(covers_all(&r("1-65535"), &r("443,5055-5058")));
    }
}
