//! Build-time tools for Discord Bridge.
//!
//! bridge-tools bundle <lock.json> <archive.zip> <out-dir>
//!     Verifies the archive against the pinned SHA-256 in the lock file,
//!     extracts the strategy files, `bin/` and `lists/` into
//!     `<out-dir>/<version>/`, checks that every strategy imports cleanly, and
//!     writes `bundle-manifest.json`.
//!
//! bridge-tools sign <manifest.json> <out.signed.json>
//!     Signs a manifest with the Ed25519 key from the
//!     `FLOWSEAL_MANIFEST_SIGNING_KEY` environment variable (base64, 32 bytes).
//!
//! bridge-tools canonical <bundle-manifest.json> <payload.json>
//!     Validates a manifest and writes the exact bytes that get signed
//!     (compact serde JSON, the same bytes `sign` signs). The signing job of
//!     the Flowseal workflow signs this file with a dependency-free script,
//!     so no build tooling runs next to the key.
//!
//! bridge-tools verify <signed.json>
//!     Verifies a signed manifest against the trusted keys compiled into the app.
//!
//! bridge-tools catalog-verify <catalog.signed.json> <catalog-data.json>
//!     Verifies a published address catalog exactly as the app does
//!     (CATALOG_TRUSTED_KEYS, hashes, every pack and icon).

use base64::Engine as _;
use bridge_core::catalog::{build_catalog, is_strategy_file};
use bridge_core::manifest::{
    self, BundleManifest, FileEntry, SignedManifest, MANIFEST_FORMAT, TRUSTED_KEYS,
};
use serde::Deserialize;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lock {
    version: String,
    winws_version: String,
    url: String,
    sha256: String,
    archive_root: String,
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::FAILURE
}

/// Only what the app needs: strategies, engine binaries/fakes, lists.
fn wanted(rel: &str) -> bool {
    (!rel.contains('/') && is_strategy_file(rel))
        || rel.starts_with("bin/")
        || rel.starts_with("lists/")
}

/// Finds the engine version string (e.g. `v72.9`) embedded in winws.exe.
fn detect_winws_version(exe: &[u8]) -> Option<String> {
    exe.split(|b| *b == 0).find_map(|chunk| {
        let s = std::str::from_utf8(chunk).ok()?;
        let rest = s.strip_prefix('v')?;
        let (maj, min) = rest.split_once('.')?;
        let ok = !maj.is_empty()
            && maj.len() <= 3
            && maj.bytes().all(|b| b.is_ascii_digit())
            && !min.is_empty()
            && min.len() <= 3
            && min.bytes().all(|b| b.is_ascii_digit());
        ok.then(|| s.to_string())
    })
}

fn bundle(lock: &Path, archive: &Path, out: &Path) -> Result<(), String> {
    let lock: Lock = serde_json::from_slice(&std::fs::read(lock).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let data = std::fs::read(archive).map_err(|e| e.to_string())?;
    let actual = manifest::sha256_hex(&data);
    if actual != lock.sha256 {
        return Err(format!(
            "archive SHA-256 mismatch: expected {}, got {actual}",
            lock.sha256
        ));
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&data)).map_err(|e| e.to_string())?;
    let dest = out.join(&lock.version);
    let _ = std::fs::remove_dir_all(&dest);
    let mut files = Vec::new();
    let mut bats = Vec::new();
    let mut detected = None;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        let Some(rel) = name.strip_prefix(&lock.archive_root) else {
            continue;
        };
        if !wanted(rel) {
            continue;
        }
        if !manifest::is_safe_bundle_path(rel) {
            return Err(format!("unsafe path in archive: {rel}"));
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let target: PathBuf = rel.split('/').fold(dest.clone(), |p, s| p.join(s));
        std::fs::create_dir_all(target.parent().expect("has parent")).map_err(|e| e.to_string())?;
        std::fs::write(&target, &buf).map_err(|e| e.to_string())?;
        if !rel.contains('/') && is_strategy_file(rel) {
            bats.push((rel.to_string(), String::from_utf8_lossy(&buf).into_owned()));
        }
        if rel == "bin/winws.exe" {
            detected = detect_winws_version(&buf);
        }
        files.push(FileEntry {
            path: rel.to_string(),
            sha256: manifest::sha256_hex(&buf),
            size: buf.len() as u64,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let cat = build_catalog(&lock.version, &bats);
    if !cat.errors.is_empty() || cat.strategies.is_empty() {
        return Err(format!("strategies failed to import: {:?}", cat.errors));
    }
    let m = BundleManifest {
        format: MANIFEST_FORMAT,
        flowseal_version: lock.version.clone(),
        winws_version: match detected {
            Some(v) => {
                if v != lock.winws_version && lock.winws_version.starts_with('v') {
                    eprintln!(
                        "note: winws.exe reports {v}, lock file says {}",
                        lock.winws_version
                    );
                }
                v
            }
            None => lock.winws_version,
        },
        source_url: lock.url,
        source_sha256: lock.sha256,
        archive_root: lock.archive_root,
        min_app_version: env!("CARGO_PKG_VERSION").into(),
        files,
    };
    m.validate().map_err(|e| e.to_string())?;
    std::fs::write(
        dest.join("bundle-manifest.json"),
        serde_json::to_vec_pretty(&m).expect("serializable"),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "Flowseal {} → {} ({} files, {} strategies)",
        m.flowseal_version,
        dest.display(),
        m.files.len(),
        cat.strategies.len()
    );
    Ok(())
}

fn sign(manifest_path: &Path, out: &Path) -> Result<(), String> {
    let key_b64 = std::env::var("FLOWSEAL_MANIFEST_SIGNING_KEY")
        .map_err(|_| "FLOWSEAL_MANIFEST_SIGNING_KEY is not set".to_string())?;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(key_b64.trim())
        .map_err(|e| e.to_string())?;
    let bytes: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| "signing key must be 32 bytes".to_string())?;
    let key = ed25519_dalek::SigningKey::from_bytes(&bytes);
    let m: BundleManifest =
        serde_json::from_slice(&std::fs::read(manifest_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    m.validate().map_err(|e| e.to_string())?;
    let signed = manifest::sign(&m, &key);
    manifest::verify(&signed, TRUSTED_KEYS)
        .map_err(|e| format!("signed with a key that the app does not trust: {e}"))?;
    std::fs::write(
        out,
        serde_json::to_vec_pretty(&signed).expect("serializable"),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "signed manifest for Flowseal {} → {}",
        m.flowseal_version,
        out.display()
    );
    Ok(())
}

fn canonical(manifest_path: &Path, out: &Path) -> Result<(), String> {
    let m: BundleManifest =
        serde_json::from_slice(&std::fs::read(manifest_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    m.validate().map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&m).expect("serializable");
    std::fs::write(out, &bytes).map_err(|e| e.to_string())?;
    println!("{}", manifest::sha256_hex(&bytes));
    Ok(())
}

fn verify(path: &Path) -> Result<(), String> {
    let s: SignedManifest =
        serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let m = manifest::verify(&s, TRUSTED_KEYS).map_err(|e| e.to_string())?;
    println!(
        "OK: Flowseal {} ({} files)",
        m.flowseal_version,
        m.files.len()
    );
    Ok(())
}

/// Print the built-in catalog of an unpacked bundle as JSON (debugging, UI previews).
fn catalog(dir: &Path) -> Result<(), String> {
    let m: BundleManifest = serde_json::from_slice(
        &std::fs::read(dir.join("bundle-manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let files: Vec<(String, String)> = m
        .files
        .iter()
        .filter(|f| !f.path.contains('/') && is_strategy_file(&f.path))
        .filter_map(|f| {
            std::fs::read(dir.join(&f.path))
                .ok()
                .map(|d| (f.path.clone(), String::from_utf8_lossy(&d).into_owned()))
        })
        .collect();
    let cat = build_catalog(&m.flowseal_version, &files);
    println!(
        "{}",
        serde_json::to_string(&cat).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn main() -> ExitCode {
    let a: Vec<String> = std::env::args().collect();
    let r = match a.get(1).map(String::as_str) {
        Some("bundle") if a.len() == 5 => bundle(Path::new(&a[2]), Path::new(&a[3]), Path::new(&a[4])),
        Some("sign") if a.len() == 4 => sign(Path::new(&a[2]), Path::new(&a[3])),
        Some("canonical") if a.len() == 4 => canonical(Path::new(&a[2]), Path::new(&a[3])),
        Some("verify") if a.len() == 3 => verify(Path::new(&a[2])),
        Some("catalog-verify") if a.len() == 4 => catalog_verify(Path::new(&a[2]), Path::new(&a[3])),
        Some("catalog") if a.len() == 3 => catalog(Path::new(&a[2])),
        Some("specs") => {
            println!("{}", serde_json::to_string(&bridge_core::strategy::options::option_specs()).expect("serializable"));
            Ok(())
        }
        _ => Err("usage: bridge-tools bundle <lock> <zip> <out> | sign <manifest> <out> | verify <signed>".into()),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(e),
    }
}

fn catalog_verify(signed: &Path, data: &Path) -> Result<(), String> {
    use bridge_core::packs::{verify_catalog, CATALOG_TRUSTED_KEYS};
    let s = std::fs::read(signed).map_err(|e| e.to_string())?;
    let d = std::fs::read(data).map_err(|e| e.to_string())?;
    let v = verify_catalog(&s, &d, CATALOG_TRUSTED_KEYS, env!("CARGO_PKG_VERSION"))
        .map_err(|e| e.to_string())?;
    println!(
        "catalog {} published {}: {} packs, {} icons — signature, hashes, packs and icons verified",
        v.manifest.serial,
        v.manifest.published,
        v.data.packs.len(),
        v.data.icons.len()
    );
    Ok(())
}
