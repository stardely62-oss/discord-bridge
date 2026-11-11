//! Remote catalog of address packs (apps, games, services).
//!
//! The catalog is data only: domains and IP networks with metadata. It is
//! published by a separate repository as two files of a GitHub release:
//!
//! - `catalog.signed.json` — an Ed25519 envelope ([`SignedManifest`]) over a
//!   [`CatalogManifest`] (`kind = "discord-bridge-catalog"`), signed with a key
//!   from [`CATALOG_TRUSTED_KEYS`] (never the Flowseal key);
//! - `catalog-data.json` — the [`CatalogData`] snapshot; its size and SHA-256
//!   are in the manifest, each icon's SHA-256 too.
//!
//! [`verify_catalog`] checks everything (signature, kind, format, client
//! version, sizes, hashes, every pack and icon) before anything is used.
//! Installed packs keep living in `InstalledPack` snapshots; this module only
//! produces and compares those snapshots.

use crate::ipset::Net;
use crate::lists::{InstalledPack, PackSnapshot};
use crate::manifest::{sha256_hex, verify_signed_bytes, ManifestError, SignedManifest};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

pub const CATALOG_KIND: &str = "discord-bridge-catalog";
/// Manifest / data format this client understands.
pub const CATALOG_FORMAT: u32 = 1;
/// Highest pack schema this client understands.
pub const PACK_SCHEMA: u32 = 1;

/// Public keys trusted for the catalog (32-byte Ed25519, base64). Separate
/// from the Flowseal manifest keys: a key signs one kind of document only.
pub const CATALOG_TRUSTED_KEYS: &[&str] = &["sI+E10LKuJ0Gg+kzX11dszTtu2staYBkGT3GoJvpzVI="];

pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;
pub const MAX_DATA_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PACKS: usize = 500;
pub const MAX_DOMAINS_PER_PACK: usize = 2_000;
pub const MAX_IPS_PER_PACK: usize = 5_000;
pub const MAX_ICONS: usize = 600;
pub const MAX_ICON_BYTES: usize = 64 * 1024;
pub const MAX_ICON_SIDE: u32 = 512;
pub const MAX_TEXT: usize = 600;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Localized {
    pub ru: String,
    pub en: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackCategory {
    Messenger,
    Voice,
    Game,
    Launcher,
    Video,
    Streaming,
    Service,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackSource {
    /// Maintained by the Bridge project.
    Official,
    /// Contributed by the community, reviewed before publication.
    Community,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProvenanceKind {
    Domains,
    Ips,
    Icon,
}

/// Where a part of the pack comes from (shown to the user).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provenance {
    pub kind: ProvenanceKind,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Requirements known for certain (otherwise false).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Requirements {
    /// The pack's IPs only help in the "selected" ipset modes.
    #[serde(default)]
    pub selected_ips: bool,
    /// Game traffic needs the game filter (UDP/TCP game ports).
    #[serde(default)]
    pub game_filter: bool,
}

/// Ports the service's own traffic uses (`"5055-5058"`, `"443,4530"`), in the
/// syntax of `--filter-tcp` / `--filter-udp`. Descriptive data: Bridge only
/// checks whether the active strategy handles these ports; nothing from here
/// ever reaches winws. A top-level field of the pack, so versions that do not
/// know it ignore it (`requires` is strict and could not carry it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackPorts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tcp: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub udp: Option<String>,
}

/// At most this many ports/ranges per protocol in a pack.
pub const MAX_PORT_ITEMS: usize = 32;

/// One declared list of [`PackPorts`] with its concrete ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortNeed<'a> {
    /// `tcp` or `udp`.
    pub protocol: &'static str,
    pub text: &'a str,
    pub ranges: Vec<(u16, u16)>,
}

impl PackPorts {
    /// Each declared list with its ranges (validated packs only).
    pub fn needs(&self) -> Vec<PortNeed<'_>> {
        [("tcp", &self.tcp), ("udp", &self.udp)]
            .into_iter()
            .filter_map(|(protocol, v)| {
                let text = v.as_deref()?;
                let ranges = crate::ports::PortSpec::parse(text).ok()?.ranges()?;
                Some(PortNeed {
                    protocol,
                    text,
                    ranges,
                })
            })
            .collect()
    }

    fn validate(&self) -> Result<(), String> {
        if self.tcp.is_none() && self.udp.is_none() {
            return Err("ports: не указан ни tcp, ни udp".into());
        }
        for (proto, v) in [("tcp", &self.tcp), ("udp", &self.udp)] {
            let Some(v) = v else { continue };
            let spec =
                crate::ports::PortSpec::parse(v).map_err(|e| format!("ports.{proto}: {e}"))?;
            let ranges = spec
                .ranges()
                .ok_or_else(|| format!("ports.{proto}: переменные игрового фильтра недопустимы"))?;
            if ranges.len() > MAX_PORT_ITEMS {
                return Err(format!("ports.{proto}: больше {MAX_PORT_ITEMS} элементов"));
            }
            if ranges.iter().any(|&(a, _)| a == 0) {
                return Err(format!("ports.{proto}: порт 0 недопустим"));
            }
            if v != v.trim() || v.contains(' ') {
                return Err(format!("ports.{proto}: пробелы недопустимы"));
            }
        }
        Ok(())
    }
}

/// Unknown fields are ignored (a newer catalog may add descriptive data;
/// the catalog's own CI rejects typos). A pack of a newer `schema` is not
/// used at all: see [`CatalogData::usable_packs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub schema: u32,
    /// Stable id (`[a-z0-9-]`, 1–40).
    pub id: String,
    /// Monotonic revision of this pack.
    pub version: u32,
    /// Date of the last change (`YYYY-MM-DD`).
    pub updated: String,
    pub name: Localized,
    pub description: Localized,
    pub category: PackCategory,
    pub source: PackSource,
    /// File name of the icon in the catalog (`*.png` / `*.webp`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub ips: Vec<String>,
    #[serde(default)]
    pub provenance: Vec<Provenance>,
    /// Limitations the card must show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Localized>,
    /// Domains help only partly (the service also talks to IPs directly).
    #[serde(default)]
    pub partial_coverage: bool,
    #[serde(default)]
    pub requires: Requirements,
    /// Ports of the service's own traffic (see [`PackPorts`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ports: Option<PackPorts>,
    /// Already covered by this Flowseal list: shown as "part of Flowseal",
    /// never as "installed" and never as "working".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flowseal_list: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_app_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IconFile {
    pub name: String,
    pub sha256: String,
    /// Base64 of the PNG / WebP bytes.
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogData {
    pub format: u32,
    pub serial: u64,
    pub packs: Vec<Pack>,
    #[serde(default)]
    pub icons: Vec<IconFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileRef {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogManifest {
    pub kind: String,
    pub format: u32,
    /// Monotonic publication number (commit time); never goes back.
    pub serial: u64,
    /// RFC 3339 publication time.
    pub published: String,
    pub pack_schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_app_version: Option<String>,
    pub data: FileRef,
    pub packs: usize,
    pub icons: usize,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("подпись каталога: {0}")]
    Signature(String),
    #[error("каталог повреждён: {0}")]
    Malformed(String),
    #[error("неподдерживаемый формат каталога {0} (обновите Bridge)")]
    UnsupportedFormat(u32),
    #[error("каталог требует Bridge {0} или новее")]
    AppTooOld(String),
    #[error("каталог больше допустимого: {0}")]
    TooLarge(String),
    #[error("данные каталога не совпадают с подписанным манифестом: {0}")]
    HashMismatch(String),
    #[error("набор «{id}»: {reason}")]
    BadPack { id: String, reason: String },
    #[error("иконка {name}: {reason}")]
    BadIcon { name: String, reason: String },
    #[error("сервер отдал более старый каталог ({got} < {have}) — он не принят")]
    Rollback { got: u64, have: u64 },
    /// The confirmed snapshot is no longer the one in the catalog or in the
    /// settings: nothing was changed, the user has to review again.
    #[error("набор «{id}» изменился после просмотра ({what}) — ничего не изменено; обновите информацию и посмотрите изменения ещё раз")]
    Stale { id: String, what: String },
}

fn bad(id: &str, reason: impl Into<String>) -> CatalogError {
    CatalogError::BadPack {
        id: id.chars().take(40).collect(),
        reason: reason.into(),
    }
}

fn text_ok(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(|c| c.is_control())
}

fn id_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 40
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
}

fn date_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Only `https://` URLs without credentials, spaces or control characters.
pub fn url_ok(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    s.len() <= 300
        && !host.is_empty()
        && !host.contains('@')
        && host.contains('.')
        && !s
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
}

/// Icon file names: `[a-z0-9-]+.(png|webp)`. SVG is never accepted.
pub fn icon_name_ok(s: &str) -> bool {
    let (stem, ext) = match s.rsplit_once('.') {
        Some(x) => x,
        None => return false,
    };
    id_ok(stem) && (ext == "png" || ext == "webp")
}

const FLOWSEAL_LISTS: &[&str] = &["list-general.txt", "list-google.txt"];

fn version_ok(v: &str) -> bool {
    semver::Version::parse(v).is_ok()
}

impl Pack {
    pub fn validate(&self) -> Result<(), CatalogError> {
        let id = &self.id;
        if !id_ok(id) {
            return Err(bad(id, "недопустимый идентификатор"));
        }
        if self.schema == 0 {
            return Err(bad(id, "неподдерживаемая схема 0"));
        }
        if self.schema > PACK_SCHEMA {
            // A newer pack format: not interpreted and not used by this client.
            return Ok(());
        }
        if self.version == 0 {
            return Err(bad(id, "версия должна быть ≥ 1"));
        }
        if !date_ok(&self.updated) {
            return Err(bad(id, "дата обновления не в формате ГГГГ-ММ-ДД"));
        }
        for (what, l) in [("название", &self.name), ("описание", &self.description)]
        {
            let max = if what == "название" {
                60
            } else {
                MAX_TEXT
            };
            if !text_ok(&l.ru, max) || !text_ok(&l.en, max) {
                return Err(bad(
                    id,
                    format!("{what} RU/EN пустое, слишком длинное или с управляющими символами"),
                ));
            }
        }
        if let Some(n) = &self.notes {
            if !text_ok(&n.ru, MAX_TEXT) || !text_ok(&n.en, MAX_TEXT) {
                return Err(bad(id, "примечания RU/EN недопустимы"));
            }
        }
        if let Some(icon) = &self.icon {
            if !icon_name_ok(icon) {
                return Err(bad(
                    id,
                    format!("недопустимое имя иконки «{icon}» (только .png/.webp)"),
                ));
            }
        }
        if let Some(l) = &self.flowseal_list {
            if !FLOWSEAL_LISTS.contains(&l.as_str()) {
                return Err(bad(id, format!("неизвестный список Flowseal «{l}»")));
            }
        }
        if let Some(v) = &self.min_app_version {
            if !version_ok(v) {
                return Err(bad(id, "minAppVersion не является версией SemVer"));
            }
        }
        if let Some(p) = &self.ports {
            p.validate().map_err(|e| bad(id, e))?;
        }
        if self.domains.is_empty() && self.ips.is_empty() {
            return Err(bad(id, "нет ни доменов, ни IP"));
        }
        if self.domains.len() > MAX_DOMAINS_PER_PACK {
            return Err(bad(id, format!("больше {MAX_DOMAINS_PER_PACK} доменов")));
        }
        if self.ips.len() > MAX_IPS_PER_PACK {
            return Err(bad(id, format!("больше {MAX_IPS_PER_PACK} IP")));
        }
        let mut seen = HashSet::new();
        for d in &self.domains {
            if d != d.trim() || d.starts_with('#') || !crate::lists::domain_ok(d) {
                return Err(bad(
                    id,
                    format!(
                        "«{}» не является доменом",
                        d.chars().take(80).collect::<String>()
                    ),
                ));
            }
            if !seen.insert(d.to_ascii_lowercase()) {
                return Err(bad(id, format!("домен «{d}» повторяется")));
            }
        }
        let mut nets = HashSet::new();
        for ip in &self.ips {
            if let Some(reason) = crate::ipset::include_entry_error(ip) {
                return Err(bad(
                    id,
                    format!("«{}»: {reason}", ip.chars().take(80).collect::<String>()),
                ));
            }
            if !nets.insert(Net::parse(ip).expect("validated")) {
                return Err(bad(id, format!("сеть «{ip}» повторяется")));
            }
        }
        for p in &self.provenance {
            if !url_ok(&p.url) {
                return Err(bad(id, "источник: допускаются только https-ссылки"));
            }
            if let Some(n) = &p.note {
                if !text_ok(n, MAX_TEXT) {
                    return Err(bad(id, "источник: недопустимое примечание"));
                }
            }
        }
        if !self.ips.is_empty()
            && !self
                .provenance
                .iter()
                .any(|p| p.kind == ProvenanceKind::Ips)
        {
            return Err(bad(id, "у IP-адресов не указан источник происхождения"));
        }
        Ok(())
    }
}

/// Width and height of a PNG or WebP image; anything else (SVG, GIF, …) is
/// rejected. Only the header is read: images are never decoded here.
pub fn image_size(b: &[u8]) -> Result<(&'static str, u32, u32), String> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if b.len() >= 24 && b.starts_with(PNG) && &b[12..16] == b"IHDR" {
        let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
        let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
        return Ok(("image/png", w, h));
    }
    if b.len() >= 30 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        let le24 =
            |i: usize| u32::from(b[i]) | u32::from(b[i + 1]) << 8 | u32::from(b[i + 2]) << 16;
        return match &b[12..16] {
            b"VP8X" => Ok(("image/webp", le24(24) + 1, le24(27) + 1)),
            b"VP8L" if b[20] == 0x2f => {
                let v = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
                Ok(("image/webp", (v & 0x3fff) + 1, ((v >> 14) & 0x3fff) + 1))
            }
            b"VP8 " if b[23..26] == [0x9d, 0x01, 0x2a] => {
                let w = u32::from(u16::from_le_bytes([b[26], b[27]]) & 0x3fff);
                let h = u32::from(u16::from_le_bytes([b[28], b[29]]) & 0x3fff);
                Ok(("image/webp", w, h))
            }
            _ => Err("неизвестный вариант WebP".into()),
        };
    }
    Err("допускаются только PNG и WebP".into())
}

fn check_icon(icon: &IconFile) -> Result<Vec<u8>, CatalogError> {
    let err = |reason: String| CatalogError::BadIcon {
        name: icon.name.chars().take(60).collect(),
        reason,
    };
    if !icon_name_ok(&icon.name) {
        return Err(err("недопустимое имя".into()));
    }
    if icon.data.len() > MAX_ICON_BYTES * 4 / 3 + 8 {
        return Err(err(format!("больше {} КБ", MAX_ICON_BYTES / 1024)));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&icon.data)
        .map_err(|e| err(e.to_string()))?;
    if bytes.len() > MAX_ICON_BYTES {
        return Err(err(format!("больше {} КБ", MAX_ICON_BYTES / 1024)));
    }
    if sha256_hex(&bytes) != icon.sha256 {
        return Err(err("SHA-256 не совпадает".into()));
    }
    let (mime, w, h) = image_size(&bytes).map_err(err)?;
    let want = if icon.name.ends_with(".png") {
        "image/png"
    } else {
        "image/webp"
    };
    if mime != want {
        return Err(err("содержимое не соответствует расширению".into()));
    }
    if w == 0 || h == 0 || w > MAX_ICON_SIDE || h > MAX_ICON_SIDE {
        return Err(err(format!("размер {w}×{h} вне 1…{MAX_ICON_SIDE}")));
    }
    Ok(bytes)
}

impl CatalogData {
    /// Validates every pack and icon (also used for the built-in snapshot,
    /// which is trusted because it is compiled into the app).
    pub fn validate(&self, app_version: &str) -> Result<(), CatalogError> {
        if self.format != CATALOG_FORMAT {
            return Err(CatalogError::UnsupportedFormat(self.format));
        }
        if self.packs.len() > MAX_PACKS {
            return Err(CatalogError::TooLarge(format!(
                "больше {MAX_PACKS} наборов"
            )));
        }
        if self.icons.len() > MAX_ICONS {
            return Err(CatalogError::TooLarge(format!("больше {MAX_ICONS} иконок")));
        }
        let mut icons = HashSet::new();
        for i in &self.icons {
            check_icon(i)?;
            if !icons.insert(i.name.as_str()) {
                return Err(CatalogError::BadIcon {
                    name: i.name.clone(),
                    reason: "повторяется".into(),
                });
            }
        }
        let mut ids = HashSet::new();
        for p in &self.packs {
            p.validate()?;
            if !ids.insert(p.id.as_str()) {
                return Err(bad(&p.id, "идентификатор повторяется"));
            }
            if let Some(i) = &p.icon {
                if !icons.contains(i.as_str()) {
                    return Err(bad(&p.id, format!("иконка «{i}» отсутствует в каталоге")));
                }
            }
            if let Some(v) = &p.min_app_version {
                if !app_at_least(app_version, v) {
                    // Not an error for the whole catalog: such packs are hidden.
                    continue;
                }
            }
        }
        Ok(())
    }

    /// Packs this client can use (those requiring a newer app are left out).
    pub fn usable_packs(&self, app_version: &str) -> Vec<&Pack> {
        self.packs
            .iter()
            .filter(|p| {
                p.schema <= PACK_SCHEMA
                    && p.min_app_version
                        .as_deref()
                        .is_none_or(|v| app_at_least(app_version, v))
            })
            .collect()
    }
}

fn app_at_least(app: &str, need: &str) -> bool {
    match (semver::Version::parse(app), semver::Version::parse(need)) {
        (Ok(a), Ok(n)) => a >= n,
        _ => false,
    }
}

/// A fully verified catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCatalog {
    pub manifest: CatalogManifest,
    pub data: CatalogData,
}

/// Parses and checks the signed manifest alone (before downloading data).
pub fn verify_manifest(
    signed_bytes: &[u8],
    keys: &[&str],
    app_version: &str,
) -> Result<CatalogManifest, CatalogError> {
    if signed_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(CatalogError::TooLarge("манифест больше 64 КБ".into()));
    }
    let envelope: SignedManifest = serde_json::from_slice(signed_bytes)
        .map_err(|e| CatalogError::Malformed(format!("конверт подписи: {e}")))?;
    let bytes = verify_signed_bytes(&envelope, keys).map_err(|e| match e {
        ManifestError::BadSignature => CatalogError::Signature("подпись недействительна".into()),
        ManifestError::UnknownKey(k) => CatalogError::Signature(format!("неизвестный ключ {k}")),
        other => CatalogError::Signature(other.to_string()),
    })?;
    // Kind first, so a document of another kind gets a clear error.
    let kind = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string));
    if kind.as_deref() != Some(CATALOG_KIND) {
        return Err(CatalogError::Signature(
            "подписанный документ не является каталогом Bridge".into(),
        ));
    }
    let probe: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| CatalogError::Malformed(e.to_string()))?;
    if let Some(f) = probe.get("format").and_then(|f| f.as_u64()) {
        if f != u64::from(CATALOG_FORMAT) {
            return Err(CatalogError::UnsupportedFormat(f as u32));
        }
    }
    let m: CatalogManifest = serde_json::from_slice(&bytes)
        .map_err(|e| CatalogError::Malformed(format!("манифест: {e}")))?;
    // A newer pack schema in the catalog is fine: such packs are left out
    // (`usable_packs`), the others stay usable.
    if let Some(v) = &m.min_app_version {
        if !app_at_least(app_version, v) {
            return Err(CatalogError::AppTooOld(v.clone()));
        }
    }
    if m.data.name != "catalog-data.json" {
        return Err(CatalogError::Malformed(
            "неожиданное имя файла данных".into(),
        ));
    }
    if m.data.size as usize > MAX_DATA_BYTES {
        return Err(CatalogError::TooLarge("данные больше 4 МБ".into()));
    }
    if m.packs > MAX_PACKS || m.icons > MAX_ICONS {
        return Err(CatalogError::TooLarge(
            "слишком много наборов или иконок".into(),
        ));
    }
    Ok(m)
}

/// Full verification: signed manifest + data file.
pub fn verify_catalog(
    signed_bytes: &[u8],
    data_bytes: &[u8],
    keys: &[&str],
    app_version: &str,
) -> Result<VerifiedCatalog, CatalogError> {
    let manifest = verify_manifest(signed_bytes, keys, app_version)?;
    if data_bytes.len() > MAX_DATA_BYTES {
        return Err(CatalogError::TooLarge("данные больше 4 МБ".into()));
    }
    if data_bytes.len() as u64 != manifest.data.size
        || sha256_hex(data_bytes) != manifest.data.sha256
    {
        return Err(CatalogError::HashMismatch(
            "размер или SHA-256 файла данных".into(),
        ));
    }
    let data: CatalogData = serde_json::from_slice(data_bytes)
        .map_err(|e| CatalogError::Malformed(format!("данные: {e}")))?;
    if data.serial != manifest.serial {
        return Err(CatalogError::HashMismatch("номер публикации".into()));
    }
    if data.packs.len() != manifest.packs || data.icons.len() != manifest.icons {
        return Err(CatalogError::HashMismatch(
            "число наборов или иконок".into(),
        ));
    }
    data.validate(app_version)?;
    Ok(VerifiedCatalog { manifest, data })
}

/// A newly fetched catalog is accepted only if it is not older than the
/// newest one ever accepted on this machine.
pub fn check_not_rollback(candidate: u64, highest_seen: u64) -> Result<(), CatalogError> {
    if candidate < highest_seen {
        Err(CatalogError::Rollback {
            got: candidate,
            have: highest_seen,
        })
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------- installed

/// Status of a catalog pack against the installed snapshots. Each value
/// means exactly one thing; "works" is never claimed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackState {
    /// Covered by a Flowseal list; cannot be installed (not "working").
    Flowseal,
    /// In the catalog, not installed.
    Available,
    /// Installed, same addresses as the catalog version.
    Current,
    /// The catalog has a newer version.
    UpdateAvailable,
    /// Installed snapshot differs from the catalog version of the same or an
    /// unknown (pre-catalog) version.
    Differs,
}

fn norm_domains(v: &[String]) -> BTreeSet<String> {
    v.iter()
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect()
}

fn norm_ips(v: &[String]) -> BTreeSet<String> {
    v.iter()
        .filter_map(|i| Net::parse(i))
        .map(|n| n.to_string())
        .collect()
}

fn same_addresses(i: &InstalledPack, p: &Pack) -> bool {
    norm_domains(&i.domains) == norm_domains(&p.domains) && norm_ips(&i.ips) == norm_ips(&p.ips)
}

pub fn pack_state(p: &Pack, installed: &[InstalledPack]) -> PackState {
    if p.flowseal_list.is_some() {
        return PackState::Flowseal;
    }
    let Some(i) = installed.iter().find(|x| x.id == p.id) else {
        return PackState::Available;
    };
    let iv = i.version.as_deref().and_then(|v| v.parse::<u32>().ok());
    match iv {
        Some(v) if v < p.version => PackState::UpdateAvailable,
        // Same version, or a pre-catalog snapshot (alpha.8 stored a date).
        _ if same_addresses(i, p) => PackState::Current,
        None => PackState::UpdateAvailable,
        Some(_) => PackState::Differs,
    }
}

/// Exact address changes between an installed snapshot and a catalog pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackDiff {
    pub id: String,
    /// Exactly what this diff was computed from (catalog pack + installed
    /// snapshot); passed back unchanged to confirm the update.
    pub confirm: PackConfirm,
    pub from_version: Option<String>,
    pub to_version: String,
    pub added_domains: Vec<String>,
    pub removed_domains: Vec<String>,
    pub added_ips: Vec<String>,
    pub removed_ips: Vec<String>,
}

pub fn pack_diff(serial: u64, installed: Option<&InstalledPack>, p: &Pack) -> PackDiff {
    let (od, oi) = installed
        .map(|i| (norm_domains(&i.domains), norm_ips(&i.ips)))
        .unwrap_or_default();
    let (nd, ni) = (norm_domains(&p.domains), norm_ips(&p.ips));
    PackDiff {
        id: p.id.clone(),
        confirm: PackConfirm {
            installed_sha256: installed.map(installed_sha256),
            ..PackConfirm::of(serial, p, &[])
        },
        from_version: installed.and_then(|i| i.version.clone()),
        to_version: p.version.to_string(),
        added_domains: nd.difference(&od).cloned().collect(),
        removed_domains: od.difference(&nd).cloned().collect(),
        added_ips: ni.difference(&oi).cloned().collect(),
        removed_ips: oi.difference(&ni).cloned().collect(),
    }
}

// ------------------------------------------------------------ confirmation

/// SHA-256 of one verified catalog pack: lowercase hex over the pack's
/// canonical JSON (`serde_json` of [`Pack`]: fixed field order, no maps, no
/// whitespace). Every field is covered — addresses, version and metadata —
/// so any change of the pack data gives a different value. Computed only
/// from verified data; the same rule for remote and built-in catalogs.
pub fn pack_sha256(p: &Pack) -> String {
    sha256_hex(&serde_json::to_vec(p).expect("a pack always serializes"))
}

/// SHA-256 of an installed snapshot (id, version, domains, IPs, title,
/// rollback point), canonical JSON of [`InstalledPack`]. Detects that the
/// settings changed between the preview and the confirmation.
pub fn installed_sha256(i: &InstalledPack) -> String {
    sha256_hex(&serde_json::to_vec(i).expect("a snapshot always serializes"))
}

/// What the user saw and confirmed: the exact pack snapshot of one catalog
/// publication, and the installed snapshot the preview was computed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PackConfirm {
    pub id: String,
    pub version: u32,
    /// Serial of the catalog (remote or built-in) the pack was shown from.
    pub serial: u64,
    /// [`pack_sha256`] of the shown pack.
    pub sha256: String,
    /// [`installed_sha256`] of the installed snapshot the diff was computed
    /// against; `None` = the pack was not installed.
    pub installed_sha256: Option<String>,
}

impl PackConfirm {
    pub fn of(serial: u64, p: &Pack, installed: &[InstalledPack]) -> Self {
        PackConfirm {
            id: p.id.clone(),
            version: p.version,
            serial,
            sha256: pack_sha256(p),
            installed_sha256: installed
                .iter()
                .find(|i| i.id == p.id)
                .map(installed_sha256),
        }
    }
}

/// Installs exactly the confirmed snapshot or nothing. `serial` and `p` are
/// the verified catalog pack as it is now; `installed` is the current
/// settings list. Any difference from `c` → [`CatalogError::Stale`] and the
/// caller keeps the settings untouched. The returned list contains `p`
/// itself — the value whose hash was just checked — so the check and the
/// stored data cannot come from two different catalog states.
pub fn install_confirmed(
    installed: &[InstalledPack],
    serial: u64,
    p: &Pack,
    c: &PackConfirm,
) -> Result<Vec<InstalledPack>, CatalogError> {
    let stale = |what: &str| CatalogError::Stale {
        id: c.id.chars().take(40).collect(),
        what: what.into(),
    };
    if p.id != c.id {
        return Err(stale("другой набор"));
    }
    if p.version != c.version {
        return Err(stale("версия набора"));
    }
    if serial != c.serial {
        return Err(stale("опубликован новый каталог"));
    }
    if pack_sha256(p) != c.sha256 {
        return Err(stale("содержимое набора"));
    }
    let now = installed
        .iter()
        .find(|i| i.id == c.id)
        .map(installed_sha256);
    if now != c.installed_sha256 {
        return Err(stale("установленная копия"));
    }
    install(installed, p)
}

/// The snapshot stored in settings for a verified catalog pack.
pub fn snapshot(p: &Pack) -> InstalledPack {
    InstalledPack {
        id: p.id.clone(),
        version: Some(p.version.to_string()),
        domains: p.domains.clone(),
        ips: p.ips.clone(),
        title: Some(p.name.clone()),
        previous: None,
    }
}

/// Installs or updates `p`; an existing snapshot becomes the rollback point.
pub fn install(installed: &[InstalledPack], p: &Pack) -> Result<Vec<InstalledPack>, CatalogError> {
    if p.flowseal_list.is_some() {
        return Err(bad(
            &p.id,
            "входит в списки Flowseal и не устанавливается отдельно",
        ));
    }
    let mut out = installed.to_vec();
    let mut new = snapshot(p);
    match out.iter_mut().find(|x| x.id == p.id) {
        Some(old) => {
            new.previous = Some(PackSnapshot {
                version: old.version.clone(),
                domains: old.domains.clone(),
                ips: old.ips.clone(),
            });
            *old = new;
        }
        None => out.push(new),
    }
    Ok(out)
}

/// Removes only this pack's snapshot (manual entries live elsewhere).
pub fn remove(installed: &[InstalledPack], id: &str) -> Vec<InstalledPack> {
    installed.iter().filter(|x| x.id != id).cloned().collect()
}

/// Restores the previous verified snapshot; the current one becomes the new
/// rollback point (so a rollback can be undone).
pub fn rollback(installed: &[InstalledPack], id: &str) -> Result<Vec<InstalledPack>, CatalogError> {
    let mut out = installed.to_vec();
    let Some(cur) = out.iter_mut().find(|x| x.id == id) else {
        return Err(bad(id, "набор не установлен"));
    };
    let Some(prev) = cur.previous.take() else {
        return Err(bad(id, "нет предыдущей версии для отката"));
    };
    let now = PackSnapshot {
        version: cur.version.take(),
        domains: std::mem::take(&mut cur.domains),
        ips: std::mem::take(&mut cur.ips),
    };
    cur.version = prev.version;
    cur.domains = prev.domains;
    cur.ips = prev.ips;
    cur.previous = Some(now);
    Ok(out)
}

pub mod store;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) mod tests_support {
    pub(crate) use super::tests::{key, pack, publish};
    use super::*;

    /// A small valid catalog with the given serial.
    pub fn sample(serial: u64) -> CatalogData {
        super::tests::data(
            serial,
            vec![super::tests::pack("steam", 1, &["steampowered.com"], &[])],
        )
    }
}
