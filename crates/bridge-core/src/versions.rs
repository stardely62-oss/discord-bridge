//! Version comparison.
//!
//! * **Discord Bridge** versions are Semantic Versioning 2.0 (`0.1.0-alpha.3`),
//!   compared with the `semver` crate: `0.1.0-alpha.3 < 0.1.0-alpha.10 < 0.1.0`.
//! * **Flowseal** tags are not guaranteed to be SemVer (`1.10.4`, possibly
//!   `1.10`, `1.10.4b`). They are compared by the documented scheme in
//!   [`flowseal_cmp`]: dot-separated numeric components, missing components
//!   count as 0, a trailing non-numeric suffix sorts *after* the plain number
//!   (`1.10.4 < 1.10.4b < 1.10.5`). Anything else is rejected.

use std::cmp::Ordering;

pub fn app_version(v: &str) -> Result<semver::Version, String> {
    semver::Version::parse(v.trim().trim_start_matches('v'))
        .map_err(|e| format!("некорректная версия «{v}»: {e}"))
}

/// `a` compared to `b` as SemVer.
pub fn app_cmp(a: &str, b: &str) -> Result<Ordering, String> {
    Ok(app_version(a)?.cmp(&app_version(b)?))
}

/// True if the running app (`current`) satisfies `min_required`.
pub fn app_satisfies(current: &str, min_required: &str) -> Result<bool, String> {
    Ok(app_cmp(current, min_required)? != Ordering::Less)
}

fn flowseal_key(v: &str) -> Option<Vec<(u64, String)>> {
    let v = v.trim().trim_start_matches('v');
    if v.is_empty() || v.len() > 32 {
        return None;
    }
    let mut out = Vec::new();
    for part in v.split('.') {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || digits.len() > 9 {
            return None;
        }
        let suffix = &part[digits.len()..];
        if !suffix.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        out.push((digits.parse().ok()?, suffix.to_ascii_lowercase()));
    }
    Some(out)
}

/// Compare Flowseal release tags (see module docs).
pub fn flowseal_cmp(a: &str, b: &str) -> Result<Ordering, String> {
    let (ka, kb) = (
        flowseal_key(a).ok_or_else(|| format!("некорректная версия Flowseal «{a}»"))?,
        flowseal_key(b).ok_or_else(|| format!("некорректная версия Flowseal «{b}»"))?,
    );
    let n = ka.len().max(kb.len());
    for i in 0..n {
        let x = ka.get(i).cloned().unwrap_or((0, String::new()));
        let y = kb.get(i).cloned().unwrap_or((0, String::new()));
        match x.0.cmp(&y.0).then_with(|| x.1.cmp(&y.1)) {
            Ordering::Equal => continue,
            o => return Ok(o),
        }
    }
    Ok(Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Ordering::*;

    #[test]
    fn semver_order_of_app_versions() {
        let ordered = [
            "0.1.0-alpha.1",
            "0.1.0-alpha.3",
            "0.1.0-alpha.10",
            "0.1.0",
            "0.1.1",
            "0.2.0",
        ];
        for w in ordered.windows(2) {
            assert_eq!(app_cmp(w[0], w[1]).unwrap(), Less, "{} < {}", w[0], w[1]);
        }
        assert_eq!(app_cmp("v0.1.0", "0.1.0").unwrap(), Equal);
        assert!(app_satisfies("0.1.0-alpha.4", "0.1.0-alpha.1").unwrap());
        assert!(!app_satisfies("0.1.0-alpha.4", "0.1.0").unwrap());
        assert!(app_cmp("garbage", "0.1.0").is_err());
    }

    #[test]
    fn flowseal_scheme() {
        assert_eq!(flowseal_cmp("1.10.4", "1.10.10").unwrap(), Less);
        assert_eq!(flowseal_cmp("1.10", "1.10.0").unwrap(), Equal);
        assert_eq!(flowseal_cmp("1.10.4", "1.10.4b").unwrap(), Less);
        assert_eq!(flowseal_cmp("1.10.4b", "1.10.5").unwrap(), Less);
        assert_eq!(flowseal_cmp("2.0", "1.99.99").unwrap(), Greater);
        assert!(flowseal_cmp("1.x", "1.0").is_err());
        assert!(flowseal_cmp("", "1.0").is_err());
    }
}
