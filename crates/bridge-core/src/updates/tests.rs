use super::*;
use crate::manifest::{sha256_hex, sign, BundleManifest, FileEntry};
use base64::Engine;
use ed25519_dalek::SigningKey;

const NOW: u64 = 1_800_000_000;

fn rel(tag: &str) -> Release {
    Release {
        tag_name: tag.into(),
        body: Some(format!("notes {tag}")),
        ..Default::default()
    }
}
fn pre(tag: &str) -> Release {
    Release {
        prerelease: true,
        ..rel(tag)
    }
}

// ---------------- Discord Bridge ----------------

#[test]
fn a_newer_alpha_is_offered_with_its_exact_release_page() {
    let list = [
        pre("v0.1.0-alpha.10"),
        pre("v0.1.0-alpha.11"),
        pre("v0.1.0-alpha.9"),
    ];
    let u = evaluate_app("0.1.0-alpha.10", &list, NOW);
    assert_eq!(u.status, UpdateStatus::Available);
    assert!(u.newer);
    assert_eq!(u.latest.as_deref(), Some("0.1.0-alpha.11"));
    assert_eq!(
        u.url.as_deref(),
        Some("https://github.com/stardely62-oss/discord-bridge/releases/tag/v0.1.0-alpha.11")
    );
    assert_eq!(u.changelog.as_deref(), Some("notes v0.1.0-alpha.11"));
    assert_eq!(u.checked_at, NOW);
}

#[test]
fn the_current_version_is_up_to_date_and_older_ones_are_never_offered() {
    let list = [
        pre("v0.1.0-alpha.10"),
        pre("v0.1.0-alpha.9"),
        pre("v0.1.0-alpha.2"),
    ];
    let u = evaluate_app("0.1.0-alpha.10", &list, NOW);
    assert_eq!(u.status, UpdateStatus::UpToDate);
    assert!(!u.newer);
    assert_eq!(u.url, None);
    // Only older releases visible (e.g. a newer one was withdrawn): no downgrade.
    let u = evaluate_app("0.1.0-alpha.11", &list, NOW);
    assert_eq!(u.status, UpdateStatus::UpToDate);
    assert!(!u.newer);
}

#[test]
fn semver_not_date_decides_alpha_beta_stable() {
    // API order is by date; the newest by date here is an older version.
    let list = [
        pre("v0.1.0-alpha.9"),
        pre("v0.1.0-beta.1"),
        pre("v0.1.0-alpha.12"),
        rel("v0.1.0"),
        pre("v0.2.0-alpha.1"),
    ];
    let u = evaluate_app("0.1.0-alpha.10", &list, NOW);
    assert_eq!(u.latest.as_deref(), Some("0.2.0-alpha.1"));
    // alpha.10 > alpha.9 numerically, beta > alpha, stable > beta.
    let u = evaluate_app("0.1.0-alpha.10", &list[..3], NOW);
    assert_eq!(u.latest.as_deref(), Some("0.1.0-beta.1"));
    let u = evaluate_app(
        "0.1.0-beta.1",
        &[pre("v0.1.0-alpha.12"), rel("v0.1.0")],
        NOW,
    );
    assert_eq!(u.latest.as_deref(), Some("0.1.0"));
    assert!(u.newer);
}

#[test]
fn a_stable_installation_is_offered_only_stable_releases() {
    let list = [rel("v0.1.0"), pre("v0.2.0-beta.1"), rel("v0.1.1")];
    let u = evaluate_app("0.1.0", &list, NOW);
    assert_eq!(u.latest.as_deref(), Some("0.1.1"));
    // A tag without a SemVer pre-release but marked pre-release on GitHub.
    let list = [rel("v0.1.0"), pre("v0.3.0")];
    assert_eq!(
        evaluate_app("0.1.0", &list, NOW).status,
        UpdateStatus::UpToDate
    );
}

#[test]
fn other_tags_and_drafts_are_not_app_releases() {
    let draft = Release {
        draft: true,
        ..pre("v0.1.0-alpha.20")
    };
    let list = [
        rel("flowseal-1.10.5"),
        rel("catalog-1793998507"),
        rel("v0.1"),
        rel("v01.2.3"),
        rel("0.9.0"),
        rel("v0.9.0 "),
        draft,
        pre("v0.1.0-alpha.10"),
    ];
    let u = evaluate_app("0.1.0-alpha.10", &list, NOW);
    assert_eq!(u.status, UpdateStatus::UpToDate);
    assert_eq!(u.latest.as_deref(), Some("0.1.0-alpha.10"));
}

#[test]
fn no_app_release_is_an_error_not_up_to_date() {
    for list in [vec![], vec![rel("flowseal-1.10.4")]] {
        let u = evaluate_app("0.1.0-alpha.10", &list, NOW);
        assert_eq!(u.status, UpdateStatus::Error);
        assert!(!u.newer);
        assert!(u.error.is_some());
    }
}

#[test]
fn release_urls_come_only_from_validated_tags() {
    assert_eq!(
        app_tag_version("v0.1.0-alpha.11").unwrap().to_string(),
        "0.1.0-alpha.11"
    );
    for bad in [
        "v0.1.0/../../x",
        "v0.1.0?x=1",
        "0.1.0",
        "v0.1.0-alpha.01",
        "javascript:alert(1)",
    ] {
        assert!(app_tag_version(bad).is_none(), "{bad}");
    }
    assert_eq!(
        flowseal_release_url("1.10.5").as_deref(),
        Some("https://github.com/Flowseal/zapret-discord-youtube/releases/tag/1.10.5")
    );
    for bad in ["1.10.5/../x", "1.10.5?a", "", "a.b", "1.10 .5"] {
        assert!(flowseal_release_url(bad).is_none(), "{bad}");
    }
}

#[test]
fn github_errors_are_explained() {
    assert!(http_error(403, Some("0"), Some(NOW + 120), NOW).contains("через 2 мин"));
    assert!(http_error(429, None, None, NOW).contains("лимит"));
    assert!(http_error(403, Some("12"), None, NOW).contains("403"));
    assert!(http_error(502, None, None, NOW).contains("временно"));
    assert!(http_error(404, None, None, NOW).contains("404"));
}

// ---------------- Flowseal ----------------

fn manifest_for(version: &str, min_app: &str) -> BundleManifest {
    BundleManifest {
        format: 1,
        flowseal_version: version.into(),
        winws_version: "v72.9".into(),
        source_url: format!("https://github.com/Flowseal/zapret-discord-youtube/releases/download/{version}/zapret-discord-youtube-{version}.zip"),
        source_sha256: "a".repeat(64),
        archive_root: format!("zapret-discord-youtube-{version}/"),
        min_app_version: min_app.into(),
        files: vec![FileEntry {
            path: "bin/winws.exe".into(),
            sha256: sha256_hex(b"x"),
            size: 1,
        }],
    }
}
fn key(seed: u8) -> (SigningKey, String) {
    let k = SigningKey::from_bytes(&[seed; 32]);
    let pk = base64::engine::general_purpose::STANDARD.encode(k.verifying_key().as_bytes());
    (k, pk)
}
fn signed_bytes(m: &BundleManifest, k: &SigningKey) -> Vec<u8> {
    serde_json::to_vec(&sign(m, k)).unwrap()
}
fn newer_flowseal() -> UpdateInfo {
    let u = evaluate_flowseal_release(Some("1.10.4"), &rel("1.10.5"), NOW);
    assert_eq!(u.status, UpdateStatus::Available);
    assert!(u.newer && !u.installable);
    u
}

#[test]
fn flowseal_release_comparison_keeps_its_own_scheme() {
    let u = evaluate_flowseal_release(Some("1.10.4"), &rel("1.10.4"), NOW);
    assert_eq!(u.status, UpdateStatus::UpToDate);
    assert_eq!(
        evaluate_flowseal_release(Some("1.10.4b"), &rel("1.10.4"), NOW).status,
        UpdateStatus::UpToDate
    );
    assert!(evaluate_flowseal_release(Some("1.10.4"), &rel("1.10.10"), NOW).newer);
    assert!(evaluate_flowseal_release(Some("1.10.4"), &rel("1.10.4b"), NOW).newer);
    // Unknown active version: any release is newer.
    assert!(evaluate_flowseal_release(None, &rel("1.10.4"), NOW).newer);
    // A comparison failure is an error, never "up to date".
    let u = evaluate_flowseal_release(Some("weird version"), &rel("1.10.5"), NOW);
    assert_eq!(u.status, UpdateStatus::Error);
    let u = evaluate_flowseal_release(Some("1.10.4"), &rel("1.10.5/../../x"), NOW);
    assert_eq!(u.status, UpdateStatus::Error);
    assert_eq!(
        evaluate_flowseal_release(Some("1.10.4"), &pre("1.10.5"), NOW).status,
        UpdateStatus::Error
    );
}

#[test]
fn a_newer_flowseal_without_a_signed_manifest_is_not_installable() {
    let (_, pk) = key(1);
    let mut u = newer_flowseal();
    apply_manifest(&mut u, ManifestFetch::NotFound, "0.1.0-alpha.10", &[&pk]);
    assert_eq!(u.status, UpdateStatus::Unavailable);
    assert!(!u.installable);
    assert!(u.unavailable_reason.unwrap().contains("ещё не проверен"));
}

#[test]
fn a_verified_signed_manifest_makes_flowseal_installable() {
    let (k, pk) = key(1);
    let mut u = newer_flowseal();
    let b = signed_bytes(&manifest_for("1.10.5", "0.1.0-alpha.10"), &k);
    apply_manifest(&mut u, ManifestFetch::Bytes(b), "0.1.0-alpha.10", &[&pk]);
    assert_eq!(u.status, UpdateStatus::Available);
    assert!(u.installable);
    assert_eq!(u.unavailable_reason, None);
}

#[test]
fn a_manifest_that_does_not_verify_or_does_not_fit_is_refused() {
    let (k, pk) = key(1);
    let (other, _) = key(2);
    let app = "0.1.0-alpha.10";
    let cases: Vec<(Vec<u8>, &str)> = vec![
        (
            signed_bytes(&manifest_for("1.10.5", app), &other),
            "подпись",
        ),
        (
            signed_bytes(&manifest_for("1.10.6", app), &k),
            "другой версии",
        ),
        (
            signed_bytes(&manifest_for("1.10.5", "0.1.0-alpha.11"), &k),
            "требует Discord Bridge 0.1.0-alpha.11",
        ),
        (b"not json".to_vec(), "повреждён"),
        (vec![b' '; MAX_MANIFEST_BYTES + 1], "слишком большой"),
    ];
    for (bytes, why) in cases {
        let mut u = newer_flowseal();
        apply_manifest(&mut u, ManifestFetch::Bytes(bytes), app, &[&pk]);
        assert_eq!(u.status, UpdateStatus::Unavailable, "{why}");
        assert!(!u.installable);
        let r = u.unavailable_reason.unwrap();
        assert!(r.contains(why), "{r}");
    }
}

#[test]
fn a_failed_manifest_download_is_an_error_not_unavailable_or_installable() {
    let (_, pk) = key(1);
    let mut u = newer_flowseal();
    apply_manifest(
        &mut u,
        ManifestFetch::Failed("тайм-аут".into()),
        "0.1.0-alpha.10",
        &[&pk],
    );
    assert_eq!(u.status, UpdateStatus::Error);
    assert!(!u.installable);
    assert!(u.error.unwrap().contains("тайм-аут"));
}

#[test]
fn the_manifest_is_not_consulted_when_nothing_is_newer() {
    let (k, pk) = key(1);
    let mut u = evaluate_flowseal_release(Some("1.10.5"), &rel("1.10.5"), NOW);
    let b = signed_bytes(&manifest_for("1.10.5", "0.1.0-alpha.1"), &k);
    apply_manifest(&mut u, ManifestFetch::Bytes(b), "0.1.0-alpha.10", &[&pk]);
    assert_eq!(u.status, UpdateStatus::UpToDate);
    assert!(!u.installable);
}

#[test]
fn the_shipped_keys_reject_a_manifest_from_an_unknown_key() {
    let (k, _) = key(3);
    let mut u = newer_flowseal();
    let b = signed_bytes(&manifest_for("1.10.5", "0.1.0-alpha.1"), &k);
    apply_manifest(
        &mut u,
        ManifestFetch::Bytes(b),
        "0.1.0-alpha.10",
        crate::manifest::TRUSTED_KEYS,
    );
    assert_eq!(u.status, UpdateStatus::Unavailable);
}

#[test]
fn manifest_urls_point_at_this_project() {
    assert_eq!(
        manifest_url(MANIFEST_BASE, "1.10.5"),
        "https://github.com/stardely62-oss/discord-bridge/releases/download/flowseal-1.10.5/flowseal-1.10.5.signed.json"
    );
}
