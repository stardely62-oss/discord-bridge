//! The dependency-free signer of the Flowseal workflow
//! (`scripts/sign/sign-ed25519.mjs`) produces envelopes that
//! `bridge_core::manifest` accepts. The fixture was signed with a one-time
//! test key whose seed was destroyed; it is not trusted by the app.

use bridge_core::manifest::{self, BundleManifest, SignedManifest, TRUSTED_KEYS};
use std::fs;
use std::path::PathBuf;

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/flowseal-sign")
            .join(name),
    )
    .unwrap()
}

#[test]
fn node_signed_flowseal_manifest_verifies_in_rust() {
    let signed: SignedManifest = serde_json::from_slice(&fixture("signed.json")).unwrap();
    let key = String::from_utf8(fixture("test-public-key.b64")).unwrap();
    let m: BundleManifest = manifest::verify(&signed, &[key.trim()]).unwrap();
    assert_eq!(m.flowseal_version, "1.10.4");
    // The signed bytes are exactly the canonical payload.
    let bytes = manifest::verify_signed_bytes(&signed, &[key.trim()]).unwrap();
    assert_eq!(bytes, fixture("payload.json"));
    assert_eq!(bytes, serde_json::to_vec(&m).unwrap());
}

#[test]
fn the_test_key_is_not_trusted_by_the_app() {
    let signed: SignedManifest = serde_json::from_slice(&fixture("signed.json")).unwrap();
    assert!(manifest::verify(&signed, TRUSTED_KEYS).is_err());
}

#[test]
fn the_pinned_public_key_file_is_the_trusted_key() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../keys/flowseal-manifest-public-key.b64");
    let file = fs::read_to_string(path).unwrap();
    assert_eq!(TRUSTED_KEYS, &[file.trim()]);
}

#[test]
fn a_changed_payload_does_not_verify() {
    let mut signed: SignedManifest = serde_json::from_slice(&fixture("signed.json")).unwrap();
    let key = String::from_utf8(fixture("test-public-key.b64")).unwrap();
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut bytes = b64.decode(&signed.manifest).unwrap();
    let n = bytes.len() - 3;
    bytes[n] ^= 1;
    signed.manifest = b64.encode(bytes);
    assert!(manifest::verify(&signed, &[key.trim()]).is_err());
}

/// The Flowseal key was rotated for 0.1.0-alpha.10. The manifest published
/// for 1.10.4 with the previous key is genuine (it verifies with that key)
/// but is no longer accepted; it has to be re-issued with the new key for the
/// same official archive.
const PREVIOUS_KEY: &str = "9LRzeYjmnNwWjLKdRaYHVuJf2te+4cLk1cNMSRYB64o=";

#[test]
fn manifests_signed_with_the_previous_key_are_rejected() {
    let signed: SignedManifest =
        serde_json::from_slice(&fixture("previous-key-1.10.4.signed.json")).unwrap();
    let m = manifest::verify(&signed, &[PREVIOUS_KEY]).unwrap();
    assert_eq!(m.flowseal_version, "1.10.4");
    assert!(!TRUSTED_KEYS.contains(&PREVIOUS_KEY));
    assert!(matches!(
        manifest::verify(&signed, TRUSTED_KEYS),
        Err(manifest::ManifestError::UnknownKey(_))
    ));
}

#[test]
fn the_reissued_manifest_keeps_the_pinned_archive_hash() {
    let signed: SignedManifest =
        serde_json::from_slice(&fixture("previous-key-1.10.4.signed.json")).unwrap();
    let m = manifest::verify(&signed, &[PREVIOUS_KEY]).unwrap();
    let lock: serde_json::Value = serde_json::from_slice(
        &fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/flowseal.lock.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(m.source_sha256, lock["sha256"].as_str().unwrap());
    assert_eq!(m.source_url, lock["url"].as_str().unwrap());
}
