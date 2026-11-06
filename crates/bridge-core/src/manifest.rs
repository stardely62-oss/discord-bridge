//! Flowseal bundle manifests.
//!
//! A manifest lists every file of a Flowseal release with its SHA-256 and
//! size, plus the hash of the official release archive. Two uses:
//!
//! 1. **Integrity of the installed bundle.** The helper re-hashes the engine
//!    and its DLL/driver against the manifest (shipped inside the protected
//!    install directory) before every start.
//! 2. **Updates.** A manifest for a new Flowseal version is accepted only if
//!    it carries a valid Ed25519 signature from the Discord Bridge release key
//!    (public key compiled into the binaries). The archive itself is
//!    downloaded from the official Flowseal GitHub release and must match the
//!    hash *inside the signed manifest* — a checksum obtained from the same
//!    place as the download is never trusted on its own.

use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MANIFEST_FORMAT: u32 = 1;

/// Ed25519 public keys (base64) trusted to sign Flowseal bundle manifests.
/// The private seed lives only in the `flowseal-sign` environment secret and
/// the owner's offline backup (rotated for 0.1.0-alpha.10; see
/// docs/release-security.md §8).
pub const TRUSTED_KEYS: &[&str] = &["hmycBFe3lANoV2kdSLLK73UmEF5RlIyyg8IJ5kM19b4="];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileEntry {
    /// Relative path with forward slashes, e.g. `bin/winws.exe`.
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BundleManifest {
    pub format: u32,
    pub flowseal_version: String,
    pub winws_version: String,
    /// Official archive URL (must be on github.com/Flowseal/...).
    pub source_url: String,
    pub source_sha256: String,
    /// Prefix inside the archive, e.g. `zapret-discord-youtube-1.10.4/`.
    pub archive_root: String,
    /// Minimum strategy schema / app version able to use this bundle.
    pub min_app_version: String,
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedManifest {
    /// The exact manifest JSON bytes that were signed (base64).
    pub manifest: String,
    /// Ed25519 signature over those bytes (base64).
    pub signature: String,
    /// Key id (first 8 bytes of the public key, hex) for rotation.
    pub key_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("подпись манифеста недействительна")]
    BadSignature,
    #[error("манифест подписан неизвестным ключом {0}")]
    UnknownKey(String),
    #[error("повреждённый манифест: {0}")]
    Malformed(String),
    #[error("файл {path}: ожидался SHA-256 {expected}, получен {actual}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("файл {0} отсутствует")]
    Missing(String),
    #[error("недопустимый путь в манифесте: {0}")]
    BadPath(String),
    #[error("источник архива не является официальным релизом Flowseal: {0}")]
    UntrustedSource(String),
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn key_id(vk: &VerifyingKey) -> String {
    hex::encode(&vk.as_bytes()[..8])
}

/// Paths must be relative, forward-slash, without `..`, and limited to the
/// directories the bundle consists of.
pub fn is_safe_bundle_path(p: &str) -> bool {
    let parts: Vec<&str> = p.split('/').collect();
    !p.is_empty()
        && p.len() <= 200
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && parts
            .iter()
            .all(|s| !s.is_empty() && *s != "." && *s != "..")
        && parts.iter().all(|s| {
            s.chars()
                .all(|c| c.is_ascii_alphanumeric() || " ._-()".contains(c))
        })
        && (parts.len() == 1 || matches!(parts[0], "bin" | "lists" | "utils"))
}

impl BundleManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.format != MANIFEST_FORMAT {
            return Err(ManifestError::Malformed(format!("формат {}", self.format)));
        }
        if !self
            .source_url
            .starts_with("https://github.com/Flowseal/zapret-discord-youtube/releases/download/")
        {
            return Err(ManifestError::UntrustedSource(self.source_url.clone()));
        }
        let v = &self.flowseal_version;
        if v.is_empty()
            || v.len() > 32
            || !v
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            return Err(ManifestError::Malformed("версия".into()));
        }
        for f in &self.files {
            if !is_safe_bundle_path(&f.path) {
                return Err(ManifestError::BadPath(f.path.clone()));
            }
            if f.sha256.len() != 64 || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(ManifestError::Malformed(format!("хэш {}", f.path)));
            }
        }
        if !self.files.iter().any(|f| f.path == "bin/winws.exe") {
            return Err(ManifestError::Malformed("нет bin/winws.exe".into()));
        }
        Ok(())
    }

    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }

    pub fn verify_file(&self, path: &str, data: &[u8]) -> Result<(), ManifestError> {
        let e = self
            .file(path)
            .ok_or_else(|| ManifestError::Missing(path.into()))?;
        let actual = sha256_hex(data);
        if !actual.eq_ignore_ascii_case(&e.sha256) || data.len() as u64 != e.size {
            return Err(ManifestError::HashMismatch {
                path: path.into(),
                expected: e.sha256.clone(),
                actual,
            });
        }
        Ok(())
    }
}

pub fn sign(manifest: &BundleManifest, key: &SigningKey) -> SignedManifest {
    let bytes = serde_json::to_vec(manifest).expect("serializable");
    let sig = key.sign(&bytes);
    let b64 = base64::engine::general_purpose::STANDARD;
    SignedManifest {
        manifest: b64.encode(&bytes),
        signature: b64.encode(sig.to_bytes()),
        key_id: key_id(&key.verifying_key()),
    }
}

/// Verifies an Ed25519 envelope against a set of trusted public keys
/// (32-byte, base64) and returns the exact signed bytes. Shared by the
/// Flowseal bundle manifests and the address catalog; each caller passes its
/// own key set and checks the document kind (keys are never shared).
pub fn verify_signed_bytes(
    signed: &SignedManifest,
    trusted_keys_b64: &[&str],
) -> Result<Vec<u8>, ManifestError> {
    let b64 = base64::engine::general_purpose::STANDARD;
    let key = trusted_keys_b64
        .iter()
        .filter_map(|k| b64.decode(k).ok())
        .filter_map(|k| <[u8; 32]>::try_from(k.as_slice()).ok())
        .filter_map(|k| VerifyingKey::from_bytes(&k).ok())
        .find(|k| key_id(k) == signed.key_id)
        .ok_or_else(|| ManifestError::UnknownKey(signed.key_id.clone()))?;
    let bytes = b64
        .decode(&signed.manifest)
        .map_err(|e| ManifestError::Malformed(e.to_string()))?;
    let sig_bytes = b64
        .decode(&signed.signature)
        .map_err(|e| ManifestError::Malformed(e.to_string()))?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| ManifestError::BadSignature)?;
    key.verify(&bytes, &sig)
        .map_err(|_| ManifestError::BadSignature)?;
    Ok(bytes)
}

/// Signs raw bytes into the shared envelope format.
pub fn sign_bytes(bytes: &[u8], key: &SigningKey) -> SignedManifest {
    let sig = key.sign(bytes);
    let b64 = base64::engine::general_purpose::STANDARD;
    SignedManifest {
        manifest: b64.encode(bytes),
        signature: b64.encode(sig.to_bytes()),
        key_id: key_id(&key.verifying_key()),
    }
}

/// Verify against a set of trusted public keys (32-byte Ed25519, base64).
pub fn verify(
    signed: &SignedManifest,
    trusted_keys_b64: &[&str],
) -> Result<BundleManifest, ManifestError> {
    let bytes = verify_signed_bytes(signed, trusted_keys_b64)?;
    let m: BundleManifest =
        serde_json::from_slice(&bytes).map_err(|e| ManifestError::Malformed(e.to_string()))?;
    m.validate()?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    fn sample() -> BundleManifest {
        BundleManifest {
            format: 1,
            flowseal_version: "1.10.4".into(),
            winws_version: "v72.9".into(),
            source_url: "https://github.com/Flowseal/zapret-discord-youtube/releases/download/1.10.4/zapret-discord-youtube-1.10.4.zip".into(),
            source_sha256: "0".repeat(64),
            archive_root: "zapret-discord-youtube-1.10.4/".into(),
            min_app_version: "0.1.0-alpha.1".into(),
            files: vec![FileEntry { path: "bin/winws.exe".into(), sha256: sha256_hex(b"x"), size: 1 }],
        }
    }

    #[test]
    fn sign_verify_and_tamper() {
        let key = SigningKey::generate(&mut OsRng);
        let pk = base64::engine::general_purpose::STANDARD.encode(key.verifying_key().as_bytes());
        let signed = sign(&sample(), &key);
        let m = verify(&signed, &[&pk]).unwrap();
        assert_eq!(m, sample());
        m.verify_file("bin/winws.exe", b"x").unwrap();
        assert!(m.verify_file("bin/winws.exe", b"y").is_err());

        // tamper with the payload
        let mut bad = signed.clone();
        let mut mm = sample();
        mm.files[0].sha256 = sha256_hex(b"evil");
        bad.manifest =
            base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&mm).unwrap());
        assert!(matches!(
            verify(&bad, &[&pk]),
            Err(ManifestError::BadSignature)
        ));

        // other key
        let other = SigningKey::generate(&mut OsRng);
        let opk =
            base64::engine::general_purpose::STANDARD.encode(other.verifying_key().as_bytes());
        assert!(matches!(
            verify(&signed, &[&opk]),
            Err(ManifestError::UnknownKey(_))
        ));
    }

    #[test]
    fn rejects_untrusted_source_and_paths() {
        let mut m = sample();
        m.source_url = "https://evil.example/zapret.zip".into();
        assert!(m.validate().is_err());
        assert!(!is_safe_bundle_path("../winws.exe"));
        assert!(!is_safe_bundle_path("bin/../../x"));
        assert!(!is_safe_bundle_path("C:/x"));
        assert!(!is_safe_bundle_path("system32/x.dll"));
        assert!(is_safe_bundle_path("general (ALT2).bat"));
        assert!(is_safe_bundle_path("lists/ipset-all.txt.backup"));
    }
}
