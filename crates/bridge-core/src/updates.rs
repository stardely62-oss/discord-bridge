//! Update checks — the decisions, without any network access.
//!
//! The app asks GitHub two independent questions:
//!
//! * **Bridge.** Which app release (tag `v<SemVer>`) is the newest?
//!   Only tags of the exact form `v<SemVer>` count; drafts and other tags
//!   (`flowseal-*`, `catalog-*`) are ignored. The newest is chosen by SemVer,
//!   never by date, and an older release is never offered (no downgrade). A
//!   stable installation is offered only stable releases; a pre-release
//!   installation is offered newer pre-releases and stable releases.
//! * **Flowseal.** Is the latest official Flowseal release newer than the
//!   active one, and can Bridge install it? Installable means: a
//!   signed manifest for exactly that version is published by this project,
//!   its Ed25519 signature verifies with a pinned key and it allows this app
//!   version. An official release alone is *not* installable.
//!
//! The result of each check is an [`UpdateInfo`] with an explicit
//! [`UpdateStatus`]; a failed check is `Error`, never "up to date".

use crate::manifest::{self, SignedManifest};
use crate::versions;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

pub const APP_REPO: &str = "stardely62-oss/discord-bridge";
pub const FLOWSEAL_REPO: &str = "Flowseal/zapret-discord-youtube";
/// Where this project publishes signed Flowseal manifests.
pub const MANIFEST_BASE: &str =
    "https://github.com/stardely62-oss/discord-bridge/releases/download";
/// Signed manifests are a few kilobytes; anything larger is refused.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
/// Release listings are small JSON documents; refuse anything huge.
pub const MAX_RELEASES_BYTES: usize = 4 * 1024 * 1024;

/// A GitHub release as returned by the REST API (only the fields used).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum UpdateStatus {
    /// Checked: nothing newer.
    UpToDate,
    /// A newer version exists (for Flowseal: and it is installable).
    Available,
    /// A newer Flowseal release exists but cannot be installed (yet).
    Unavailable,
    /// The check itself failed — nothing is known.
    #[default]
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub status: UpdateStatus,
    pub current: Option<String>,
    pub latest: Option<String>,
    pub newer: bool,
    pub changelog: Option<String>,
    /// Page of exactly the `latest` release, built from the validated tag.
    pub url: Option<String>,
    pub error: Option<String>,
    /// Flowseal only: a verified signed manifest allows installation.
    pub installable: bool,
    /// Flowseal only: why a newer release cannot be installed.
    pub unavailable_reason: Option<String>,
    /// Unix seconds of the check.
    pub checked_at: u64,
}

impl UpdateInfo {
    pub fn failed(current: Option<String>, error: impl Into<String>, now: u64) -> Self {
        UpdateInfo {
            status: UpdateStatus::Error,
            current,
            error: Some(error.into()),
            checked_at: now,
            ..Default::default()
        }
    }
}

fn changelog(body: &Option<String>) -> Option<String> {
    body.as_ref()
        .map(|b| b.chars().take(8000).collect::<String>())
        .filter(|b| !b.trim().is_empty())
}

/// The app version of a release tag: exactly `v` + canonical SemVer.
pub fn app_tag_version(tag: &str) -> Option<semver::Version> {
    let v = semver::Version::parse(tag.strip_prefix('v')?).ok()?;
    (format!("v{v}") == tag).then_some(v)
}

/// Page of an app release. Only a validated version can be turned into a URL.
pub fn app_release_url(v: &semver::Version) -> String {
    format!("https://github.com/{APP_REPO}/releases/tag/v{v}")
}

/// A Flowseal tag that is safe to use in URLs and comparable by the
/// Flowseal scheme (digits, letters and dots only).
pub fn flowseal_tag(tag: &str) -> Option<String> {
    let t = tag.trim();
    let ok = !t.is_empty()
        && t.len() <= 32
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
        && versions::flowseal_cmp(t.trim_start_matches('v'), "0").is_ok();
    ok.then(|| t.to_string())
}

pub fn flowseal_release_url(tag: &str) -> Option<String> {
    flowseal_tag(tag).map(|t| format!("https://github.com/{FLOWSEAL_REPO}/releases/tag/{t}"))
}

pub fn manifest_url(base: &str, version: &str) -> String {
    format!("{base}/flowseal-{version}/flowseal-{version}.signed.json")
}

/// Decide about an app update from the release list.
pub fn evaluate_app(current: &str, releases: &[Release], now: u64) -> UpdateInfo {
    let cur = match versions::app_version(current) {
        Ok(v) => v,
        Err(e) => return UpdateInfo::failed(Some(current.into()), e, now),
    };
    let stable_only = cur.pre.is_empty();
    let best = releases
        .iter()
        .filter(|r| !r.draft)
        .filter_map(|r| app_tag_version(&r.tag_name).map(|v| (v, r)))
        .filter(|(v, r)| !stable_only || (v.pre.is_empty() && !r.prerelease))
        .max_by(|a, b| a.0.cmp(&b.0));
    let Some((v, r)) = best else {
        return UpdateInfo::failed(
            Some(current.into()),
            "на GitHub не найдено ни одного релиза Bridge",
            now,
        );
    };
    let newer = v.cmp(&cur) == Ordering::Greater;
    UpdateInfo {
        status: if newer {
            UpdateStatus::Available
        } else {
            UpdateStatus::UpToDate
        },
        current: Some(cur.to_string()),
        latest: Some(v.to_string()),
        newer,
        changelog: if newer { changelog(&r.body) } else { None },
        url: newer.then(|| app_release_url(&v)),
        checked_at: now,
        ..Default::default()
    }
}

/// First stage of the Flowseal check: is the latest official release newer?
/// `Available` here only means "newer, manifest still to be checked".
pub fn evaluate_flowseal_release(current: Option<&str>, latest: &Release, now: u64) -> UpdateInfo {
    let cur = current.map(str::to_string);
    if latest.draft || latest.prerelease {
        return UpdateInfo::failed(
            cur,
            "GitHub вернул черновик или предварительный релиз Flowseal",
            now,
        );
    }
    let Some(tag) = flowseal_tag(&latest.tag_name) else {
        return UpdateInfo::failed(
            cur,
            format!(
                "некорректный тег релиза Flowseal «{}»",
                latest.tag_name.chars().take(40).collect::<String>()
            ),
            now,
        );
    };
    let version = tag.trim_start_matches('v').to_string();
    let newer = match current {
        None => true,
        Some(c) => match versions::flowseal_cmp(&version, c) {
            Ok(o) => o == Ordering::Greater,
            Err(e) => return UpdateInfo::failed(cur, e, now),
        },
    };
    UpdateInfo {
        status: if newer {
            UpdateStatus::Available
        } else {
            UpdateStatus::UpToDate
        },
        current: cur,
        latest: Some(version),
        newer,
        changelog: if newer { changelog(&latest.body) } else { None },
        url: flowseal_release_url(&tag),
        checked_at: now,
        ..Default::default()
    }
}

/// Result of downloading the signed manifest for the latest version.
#[derive(Debug)]
pub enum ManifestFetch {
    /// 404: not published by this project (yet).
    NotFound,
    Bytes(Vec<u8>),
    /// Network or HTTP failure: nothing is known about installability.
    Failed(String),
}

/// Second stage: installable only with a verified signed manifest for exactly
/// this version that allows `app_version`.
pub fn apply_manifest(
    info: &mut UpdateInfo,
    fetch: ManifestFetch,
    app_version: &str,
    keys: &[&str],
) {
    if info.status != UpdateStatus::Available {
        return;
    }
    let version = info.latest.clone().unwrap_or_default();
    let unavailable = |info: &mut UpdateInfo, why: String| {
        info.status = UpdateStatus::Unavailable;
        info.installable = false;
        info.unavailable_reason = Some(why);
    };
    let bytes = match fetch {
        ManifestFetch::NotFound => {
            return unavailable(
                info,
                format!("Вышел Flowseal {version}, но его пакет ещё не проверен для Bridge: нужен подписанный манифест, опубликованный проектом."),
            )
        }
        ManifestFetch::Failed(e) => {
            info.status = UpdateStatus::Error;
            info.installable = false;
            info.error = Some(format!(
                "вышел Flowseal {version}, но проверить подписанный манифест не удалось: {e}"
            ));
            return;
        }
        ManifestFetch::Bytes(b) => b,
    };
    if bytes.len() > MAX_MANIFEST_BYTES {
        return unavailable(
            info,
            "подписанный манифест слишком большой — он не принят".into(),
        );
    }
    let signed: SignedManifest = match serde_json::from_slice(&bytes) {
        Ok(s) => s,
        Err(_) => {
            return unavailable(
                info,
                "подписанный манифест повреждён — установка недоступна".into(),
            )
        }
    };
    let m =
        match manifest::verify(&signed, keys) {
            Ok(m) => m,
            Err(_) => return unavailable(
                info,
                "подпись манифеста не прошла проверку закреплённым ключом — установка недоступна"
                    .into(),
            ),
        };
    if m.flowseal_version != version {
        return unavailable(
            info,
            "подписанный манифест относится к другой версии Flowseal".into(),
        );
    }
    match versions::app_satisfies(app_version, &m.min_app_version) {
        Ok(true) => {
            info.installable = true;
            info.unavailable_reason = None;
        }
        Ok(false) => unavailable(
            info,
            format!(
                "Flowseal {version} требует Bridge {} или новее — сначала обновите приложение",
                m.min_app_version
            ),
        ),
        Err(e) => unavailable(info, e),
    }
}

/// User-facing text for a failed GitHub request.
pub fn http_error(
    status: u16,
    ratelimit_remaining: Option<&str>,
    ratelimit_reset: Option<u64>,
    now: u64,
) -> String {
    let limited = status == 429 || (status == 403 && ratelimit_remaining == Some("0"));
    if limited {
        return match ratelimit_reset.filter(|r| *r > now) {
            Some(r) => format!(
                "лимит запросов к GitHub API исчерпан — повторите примерно через {} мин",
                (r - now).div_ceil(60)
            ),
            None => "лимит запросов к GitHub API исчерпан — повторите позже".into(),
        };
    }
    match status {
        404 => "релизы не найдены (GitHub ответил 404)".into(),
        500..=599 => format!("GitHub временно недоступен (HTTP {status})"),
        _ => format!("GitHub ответил HTTP {status}"),
    }
}

#[cfg(test)]
mod tests;
