//! Signed description of one Bridge installer, for installing an update from
//! inside the app.
//!
//! * The document is an Ed25519 envelope (the same format as the Flowseal
//!   manifests) over [`AppReleaseManifest`]: version, installer file name,
//!   size and SHA-256. It is signed by the owner in a protected environment
//!   after the release is published and reviewed, and published as the
//!   release `app-<version>`.
//! * Its keys ([`APP_RELEASE_TRUSTED_KEYS`]) sign nothing else: not the
//!   Flowseal manifests, not the catalog.
//! * Nothing in the document is used as a location: the manifest and the
//!   installer are fetched from URLs built from the validated version.
//! * Without a verified document the app behaves as before: it shows the
//!   new version and opens the release page.
//!
//! A hash published next to the installer (`SHA256SUMS.txt`) proves nothing
//! about who built it; the signature made with a key that never leaves the
//! signing environment does.

use crate::manifest::{verify_signed_bytes, SignedManifest};
use crate::updates::{APP_REPO, MAX_MANIFEST_BYTES};
use serde::{Deserialize, Serialize};

pub const APP_RELEASE_KIND: &str = "bridge-app-release";
pub const APP_RELEASE_FORMAT: u32 = 1;

/// Public keys (32-byte Ed25519, base64) allowed to sign app releases.
/// Empty until the owner generates the key (docs/release-security.md):
/// with no key pinned, no manifest is requested and installing from inside
/// the app is off.
pub const APP_RELEASE_TRUSTED_KEYS: &[&str] = &[];

pub const MIN_INSTALLER_BYTES: u64 = 1024 * 1024;
pub const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppReleaseManifest {
    pub kind: String,
    pub format: u32,
    /// Canonical SemVer without the `v` of the tag.
    pub version: String,
    pub installer_name: String,
    pub installer_size: u64,
    /// Lowercase hex.
    pub installer_sha256: String,
}

/// The installer asset of a release, as `release.yml` names it.
pub fn installer_name(version: &semver::Version) -> String {
    format!("Bridge_{version}_x64-setup.exe")
}

/// `base` is `https://github.com` in production.
pub fn installer_url(base: &str, version: &semver::Version) -> String {
    format!(
        "{base}/{APP_REPO}/releases/download/v{version}/{}",
        installer_name(version)
    )
}

/// `base` is [`crate::updates::MANIFEST_BASE`] in production.
pub fn app_manifest_url(base: &str, version: &semver::Version) -> String {
    format!("{base}/app-{version}/app-{version}.signed.json")
}

fn canonical(version: &str) -> Option<semver::Version> {
    let v = semver::Version::parse(version).ok()?;
    (v.to_string() == version).then_some(v)
}

/// Checks the fields of a manifest (not the signature).
pub fn validate(m: &AppReleaseManifest) -> Result<semver::Version, String> {
    if m.kind != APP_RELEASE_KIND {
        return Err("документ другого вида, не манифест релиза Bridge".into());
    }
    if m.format != APP_RELEASE_FORMAT {
        return Err(format!("неизвестный формат манифеста релиза: {}", m.format));
    }
    let v = canonical(&m.version).ok_or("некорректная версия в манифесте релиза")?;
    if m.installer_name != installer_name(&v) {
        return Err("имя установщика в манифесте не соответствует версии".into());
    }
    if !(MIN_INSTALLER_BYTES..=MAX_INSTALLER_BYTES).contains(&m.installer_size) {
        return Err("недопустимый размер установщика в манифесте".into());
    }
    let hex_ok = m.installer_sha256.len() == 64
        && m.installer_sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !hex_ok {
        return Err("некорректный SHA-256 установщика в манифесте".into());
    }
    Ok(v)
}

/// Full check of a downloaded envelope: size, signature by a pinned key,
/// kind, fields, and that it describes exactly `expect_version`.
pub fn verify_app_release(
    envelope: &[u8],
    keys: &[&str],
    expect_version: &str,
) -> Result<AppReleaseManifest, String> {
    if envelope.len() > MAX_MANIFEST_BYTES {
        return Err("манифест релиза слишком большой".into());
    }
    let signed: SignedManifest =
        serde_json::from_slice(envelope).map_err(|_| "манифест релиза повреждён".to_string())?;
    let bytes = verify_signed_bytes(&signed, keys)
        .map_err(|_| "подпись манифеста релиза не прошла проверку закреплённым ключом")?;
    // Kind first, so a document of another kind signed by mistake gets a clear error.
    let kind = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string));
    if kind.as_deref() != Some(APP_RELEASE_KIND) {
        return Err("документ другого вида, не манифест релиза Bridge".into());
    }
    let m: AppReleaseManifest =
        serde_json::from_slice(&bytes).map_err(|_| "манифест релиза повреждён".to_string())?;
    validate(&m)?;
    if m.version != expect_version {
        return Err("манифест относится к другой версии Bridge".into());
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{key_id, sign_bytes};
    use base64::Engine;
    use ed25519_dalek::SigningKey;
    use rand_core::OsRng;

    fn manifest(version: &str) -> AppReleaseManifest {
        AppReleaseManifest {
            kind: APP_RELEASE_KIND.into(),
            format: APP_RELEASE_FORMAT,
            version: version.into(),
            installer_name: format!("Bridge_{version}_x64-setup.exe"),
            installer_size: 20 * 1024 * 1024,
            installer_sha256: "ab".repeat(32),
        }
    }

    fn envelope(m: &impl Serialize, key: &SigningKey) -> Vec<u8> {
        serde_json::to_vec(&sign_bytes(&serde_json::to_vec(m).unwrap(), key)).unwrap()
    }

    fn keypair() -> (SigningKey, String) {
        let k = SigningKey::generate(&mut OsRng);
        let b64 = base64::engine::general_purpose::STANDARD.encode(k.verifying_key().to_bytes());
        (k, b64)
    }

    #[test]
    fn a_signed_manifest_for_the_expected_version_is_accepted() {
        let (k, pk) = keypair();
        let m = manifest("0.2.0");
        assert_eq!(
            verify_app_release(&envelope(&m, &k), &[&pk], "0.2.0").unwrap(),
            m
        );
        let pre = manifest("0.2.0-beta.1");
        assert!(verify_app_release(&envelope(&pre, &k), &[&pk], "0.2.0-beta.1").is_ok());
    }

    #[test]
    fn other_keys_versions_and_kinds_are_rejected() {
        let (k, pk) = keypair();
        let (other, other_pk) = keypair();
        let m = manifest("0.2.0");
        let env = envelope(&m, &k);
        assert!(verify_app_release(&env, &[&other_pk], "0.2.0").is_err());
        assert!(verify_app_release(&env, &[], "0.2.0").is_err());
        assert!(verify_app_release(&env, &[&pk], "0.2.1").is_err());
        assert!(verify_app_release(&envelope(&m, &other), &[&pk], "0.2.0").is_err());
        // A Flowseal-like document signed with the app key is not an app release.
        let foreign = serde_json::json!({"kind":"discord-bridge-catalog","serial":1});
        assert!(verify_app_release(&envelope(&foreign, &k), &[&pk], "0.2.0").is_err());
        // A tampered payload keeps the old signature.
        let mut signed: SignedManifest = serde_json::from_slice(&env).unwrap();
        let mut forged = manifest("0.2.0");
        forged.installer_sha256 = "cd".repeat(32);
        signed.manifest =
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&forged).unwrap());
        assert!(
            verify_app_release(&serde_json::to_vec(&signed).unwrap(), &[&pk], "0.2.0").is_err()
        );
        assert_eq!(key_id(&k.verifying_key()).len(), 16);
    }

    #[test]
    fn fields_are_strict() {
        let ok = manifest("0.2.0");
        assert!(validate(&ok).is_ok());
        type Change = Box<dyn Fn(&mut AppReleaseManifest)>;
        let cases: Vec<Change> = vec![
            Box::new(|m| m.format = 2),
            Box::new(|m| m.version = "v0.2.0".into()),
            Box::new(|m| m.version = "0.2".into()),
            Box::new(|m| m.installer_name = "evil.exe".into()),
            Box::new(|m| m.installer_name = "../Bridge_0.2.0_x64-setup.exe".into()),
            Box::new(|m| m.installer_size = 10),
            Box::new(|m| m.installer_size = MAX_INSTALLER_BYTES + 1),
            Box::new(|m| m.installer_sha256 = "AB".repeat(32)),
            Box::new(|m| m.installer_sha256 = "ab".repeat(31)),
        ];
        for change in cases {
            let mut m = ok.clone();
            change(&mut m);
            assert!(validate(&m).is_err(), "{m:?}");
        }
        let extra = r#"{"kind":"bridge-app-release","format":1,"version":"0.2.0","installerName":"Bridge_0.2.0_x64-setup.exe","installerSize":20971520,"installerSha256":"abababababababababababababababababababababababababababababababab","url":"https://example.com/x.exe"}"#;
        assert!(serde_json::from_str::<AppReleaseManifest>(extra).is_err());
    }

    #[test]
    fn locations_come_from_the_version_only() {
        let v = semver::Version::parse("0.2.0").unwrap();
        assert_eq!(
            installer_url("https://github.com", &v),
            "https://github.com/stardely62-oss/discord-bridge/releases/download/v0.2.0/Bridge_0.2.0_x64-setup.exe"
        );
        assert_eq!(
            app_manifest_url("https://x/releases/download", &v),
            "https://x/releases/download/app-0.2.0/app-0.2.0.signed.json"
        );
    }

    /// The signer checks the key against `keys/app-release-public-key.b64`,
    /// the app against the constant: they must name the same key, and
    /// neither may exist without the other.
    #[test]
    fn the_key_file_and_the_compiled_key_agree() {
        let file = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../keys/app-release-public-key.b64"
        );
        match std::fs::read_to_string(file) {
            Ok(text) => assert_eq!(APP_RELEASE_TRUSTED_KEYS.first(), Some(&text.trim())),
            Err(_) => assert!(APP_RELEASE_TRUSTED_KEYS.is_empty()),
        }
    }

    #[test]
    fn pinned_keys_are_well_formed_and_not_shared_with_other_documents() {
        for k in APP_RELEASE_TRUSTED_KEYS {
            let raw = base64::engine::general_purpose::STANDARD.decode(k).unwrap();
            assert_eq!(raw.len(), 32);
            assert!(!crate::manifest::TRUSTED_KEYS.contains(k));
            assert!(!crate::packs::CATALOG_TRUSTED_KEYS.contains(k));
        }
    }
}
