//! Flowseal bundles known to the helper: the one shipped in the protected
//! install directory and verified updates stored under ProgramData.

use bridge_core::catalog::{build_catalog, is_strategy_file, resource_index};
use bridge_core::lists::IPSET_ALL_BACKUP;
use bridge_core::manifest::{self, BundleManifest, ManifestError, SignedManifest};
use bridge_core::strategy::validate::ResourceIndex;
use std::fs::{self, File};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

pub const MANIFEST_FILE: &str = "bundle-manifest.json";

/// Engine files that must be locked against modification while in use.
pub const ENGINE_FILES: &[&str] = &[
    "bin/winws.exe",
    "bin/cygwin1.dll",
    "bin/WinDivert.dll",
    "bin/WinDivert64.sys",
];

#[derive(Debug, Clone)]
pub struct Bundle {
    pub dir: PathBuf,
    pub builtin: bool,
    pub manifest: BundleManifest,
}

impl Bundle {
    pub fn version(&self) -> &str {
        &self.manifest.flowseal_version
    }
    pub fn path_of(&self, rel: &str) -> PathBuf {
        rel.split('/').fold(self.dir.clone(), |p, s| p.join(s))
    }
    pub fn bin_dir(&self) -> PathBuf {
        self.dir.join("bin")
    }

    pub fn resource_index(&self) -> ResourceIndex {
        let bin: Vec<&str> = self
            .manifest
            .files
            .iter()
            .filter_map(|f| f.path.strip_prefix("bin/"))
            .collect();
        let lists: Vec<&str> = self
            .manifest
            .files
            .iter()
            .filter_map(|f| f.path.strip_prefix("lists/"))
            .filter(|n| *n != IPSET_ALL_BACKUP)
            .collect();
        resource_index(bin, lists)
    }

    /// Hash every file listed in the manifest.
    pub fn check_integrity(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for f in &self.manifest.files {
            match fs::read(self.path_of(&f.path)) {
                Ok(data) => {
                    if let Err(e) = self.manifest.verify_file(&f.path, &data) {
                        problems.push(e.to_string());
                    }
                }
                Err(_) => problems.push(format!("файл {} отсутствует", f.path)),
            }
        }
        problems
    }

    /// Original strategy files (verified against the manifest).
    pub fn strategy_files(&self) -> Vec<(String, String)> {
        self.manifest
            .files
            .iter()
            .filter(|f| !f.path.contains('/') && is_strategy_file(&f.path))
            .filter_map(|f| {
                let data = fs::read(self.path_of(&f.path)).ok()?;
                self.manifest.verify_file(&f.path, &data).ok()?;
                Some((f.path.clone(), String::from_utf8_lossy(&data).into_owned()))
            })
            .collect()
    }

    pub fn read_backup_ipset(&self) -> Option<String> {
        let rel = format!("lists/{IPSET_ALL_BACKUP}");
        let data = fs::read(self.path_of(&rel)).ok()?;
        self.manifest.verify_file(&rel, &data).ok()?;
        String::from_utf8(data).ok()
    }
}

/// A bundle requires at least `min_app_version` of Discord Bridge (SemVer).
pub fn check_app_compat(m: &BundleManifest) -> Result<(), String> {
    check_app_compat_with(m, env!("CARGO_PKG_VERSION"))
}

pub fn check_app_compat_with(m: &BundleManifest, app: &str) -> Result<(), String> {
    match bridge_core::versions::app_satisfies(app, &m.min_app_version) {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "Flowseal {} требует Discord Bridge {} или новее (установлена {app})",
            m.flowseal_version, m.min_app_version
        )),
        Err(e) => Err(format!("некорректное поле minAppVersion: {e}")),
    }
}

fn load_dir(dir: &Path, builtin: bool) -> Option<Bundle> {
    let text = fs::read_to_string(dir.join(MANIFEST_FILE)).ok()?;
    let manifest: BundleManifest = serde_json::from_str(&text).ok()?;
    manifest.validate().ok()?;
    let name = dir.file_name()?.to_str()?;
    if name != manifest.flowseal_version {
        return None;
    }
    Some(Bundle {
        dir: dir.to_path_buf(),
        builtin,
        manifest,
    })
}

/// Bundles sorted newest first.
pub fn discover(builtin_root: &Path, updates_root: &Path) -> Vec<Bundle> {
    let mut out = Vec::new();
    for (root, builtin) in [(builtin_root, true), (updates_root, false)] {
        if let Ok(rd) = fs::read_dir(root) {
            for e in rd.flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(b) = load_dir(&e.path(), builtin) {
                        if !out.iter().any(|x: &Bundle| x.version() == b.version()) {
                            out.push(b);
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| {
        bridge_core::versions::flowseal_cmp(b.version(), a.version())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[derive(Debug)]
pub enum InstallError {
    Manifest(ManifestError),
    Archive(String),
    Incompatible(String),
    Io(std::io::Error),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Manifest(e) => write!(f, "{e}"),
            InstallError::Archive(e) => write!(f, "архив: {e}"),
            InstallError::Incompatible(e) => write!(f, "несовместимая версия: {e}"),
            InstallError::Io(e) => write!(f, "ошибка записи: {e}"),
        }
    }
}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        InstallError::Io(e)
    }
}

/// Verify and install a Flowseal release into `updates_root/<version>`.
/// Nothing is written unless the signature, the archive hash, every file hash
/// and the strategy compatibility check pass.
pub fn install(
    signed: &SignedManifest,
    archive: &[u8],
    updates_root: &Path,
    trusted: &[&str],
) -> Result<String, InstallError> {
    let m = manifest::verify(signed, trusted).map_err(InstallError::Manifest)?;
    check_app_compat(&m).map_err(InstallError::Incompatible)?;
    let actual = manifest::sha256_hex(archive);
    if !actual.eq_ignore_ascii_case(&m.source_sha256) {
        return Err(InstallError::Manifest(ManifestError::HashMismatch {
            path: "archive".into(),
            expected: m.source_sha256.clone(),
            actual,
        }));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(archive))
        .map_err(|e| InstallError::Archive(e.to_string()))?;
    let mut contents: Vec<(String, Vec<u8>)> = Vec::new();
    for f in &m.files {
        let name = format!("{}{}", m.archive_root, f.path);
        let mut entry = zip
            .by_name(&name)
            .map_err(|_| InstallError::Manifest(ManifestError::Missing(f.path.clone())))?;
        if entry.size() > 64 * 1024 * 1024 {
            return Err(InstallError::Archive(format!("{} слишком большой", f.path)));
        }
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut data)
            .map_err(|e| InstallError::Archive(e.to_string()))?;
        m.verify_file(&f.path, &data)
            .map_err(InstallError::Manifest)?;
        contents.push((f.path.clone(), data));
    }
    // Compatibility: every strategy file must be importable by this version.
    let bats: Vec<(String, String)> = contents
        .iter()
        .filter(|(p, _)| !p.contains('/') && is_strategy_file(p))
        .map(|(p, d)| (p.clone(), String::from_utf8_lossy(d).into_owned()))
        .collect();
    let cat = build_catalog(&m.flowseal_version, &bats);
    if cat.strategies.is_empty() || !cat.errors.is_empty() {
        return Err(InstallError::Incompatible(
            cat.errors
                .iter()
                .map(|e| format!("{}: {}", e.file, e.error))
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }

    let final_dir = updates_root.join(&m.flowseal_version);
    let staging = updates_root.join(format!("{}.staging", m.flowseal_version));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    for (p, data) in &contents {
        let target = p.split('/').fold(staging.clone(), |acc, s| acc.join(s));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, data)?;
    }
    fs::write(
        staging.join(MANIFEST_FILE),
        serde_json::to_vec_pretty(&m).expect("serializable"),
    )?;
    if final_dir.exists() {
        let old = updates_root.join(format!("{}.old", m.flowseal_version));
        let _ = fs::remove_dir_all(&old);
        fs::rename(&final_dir, &old)?;
    }
    fs::rename(&staging, &final_dir)?;
    Ok(m.flowseal_version)
}

/// Opens engine files so they cannot be modified or replaced while held,
/// and verifies their hashes *through the same handles* (no TOCTOU window
/// between verification and process creation).
pub struct LockedFiles {
    _files: Vec<File>,
}

/// Read-only access to a list file of an installed Flowseal version for the
/// UI (which cannot read the protected bundle directory). Only files listed
/// in the bundle manifest under `lists/` are served, after their SHA-256 was
/// checked against the manifest; the file is returned in chunks.
pub fn read_bundle_list(
    all: &[Bundle],
    version: &str,
    name: &str,
    offset: u64,
) -> Result<bridge_core::ipc::Response, String> {
    use base64::Engine as _;
    use bridge_core::ipc::{Response, LIST_CHUNK, MAX_LIST_FILE};
    if name.is_empty()
        || name.len() > 64
        || name.contains(['/', '\\', ':'])
        || name.starts_with('.')
        || name.contains("..")
    {
        return Err("недопустимое имя списка".into());
    }
    let b = all
        .iter()
        .find(|b| b.version() == version)
        .ok_or_else(|| format!("Flowseal {version} не установлен"))?;
    let rel = format!("lists/{name}");
    let entry = b
        .manifest
        .files
        .iter()
        .find(|f| f.path == rel)
        .ok_or_else(|| format!("в Flowseal {version} нет файла {rel}"))?;
    if entry.size > MAX_LIST_FILE {
        return Err(format!("{rel}: файл больше допустимого"));
    }
    let data = std::fs::read(b.path_of(&rel)).map_err(|e| format!("{rel}: {e}"))?;
    b.manifest
        .verify_file(&rel, &data)
        .map_err(|e| format!("{rel}: {e}"))?;
    let start = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(data.len());
    let end = (start + LIST_CHUNK).min(data.len());
    Ok(Response::BundleList {
        version: version.into(),
        name: name.into(),
        sha256: entry.sha256.clone(),
        size: data.len() as u64,
        offset: start as u64,
        data: base64::engine::general_purpose::STANDARD.encode(&data[start..end]),
    })
}

pub fn lock_and_verify(bundle: &Bundle) -> Result<LockedFiles, String> {
    let mut files = Vec::new();
    for rel in ENGINE_FILES {
        let path = bundle.path_of(rel);
        let mut f = open_deny_write(&path).map_err(|e| format!("{rel}: {e}"))?;
        let mut data = Vec::new();
        f.read_to_end(&mut data)
            .map_err(|e| format!("{rel}: {e}"))?;
        bundle
            .manifest
            .verify_file(rel, &data)
            .map_err(|e| e.to_string())?;
        files.push(f);
    }
    Ok(LockedFiles { _files: files })
}

#[cfg(windows)]
fn open_deny_write(p: &Path) -> std::io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x1;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(p)
}

#[cfg(not(windows))]
fn open_deny_write(p: &Path) -> std::io::Result<File> {
    File::open(p)
}

/// Test fixtures shared with other modules' tests.
#[cfg(test)]
pub mod tests_support {
    use super::*;
    use bridge_core::manifest::{sha256_hex, FileEntry};

    pub fn write_std_bundle(root: &Path, ver: &str) {
        let files: &[(&str, &[u8])] = &[
            ("bin/winws.exe", b"exe"),
            ("bin/cygwin1.dll", b"cyg"),
            ("bin/WinDivert.dll", b"dll"),
            ("bin/WinDivert64.sys", b"sys"),
            ("bin/stun.bin", b"fake"),
            ("lists/list-general.txt", b"discord.com\n"),
        ];
        let dir = root.join(ver);
        let mut entries = Vec::new();
        for (p, d) in files {
            let t = p.split('/').fold(dir.clone(), |a, s| a.join(s));
            fs::create_dir_all(t.parent().unwrap()).unwrap();
            fs::write(&t, d).unwrap();
            entries.push(FileEntry {
                path: p.to_string(),
                sha256: sha256_hex(d),
                size: d.len() as u64,
            });
        }
        let m = BundleManifest {
            format: 1,
            flowseal_version: ver.into(),
            winws_version: "v72.9".into(),
            source_url: format!(
                "https://github.com/Flowseal/zapret-discord-youtube/releases/download/{ver}/x.zip"
            ),
            source_sha256: "0".repeat(64),
            archive_root: String::new(),
            min_app_version: "0.1.0-alpha.1".into(),
            files: entries,
        };
        fs::write(dir.join(MANIFEST_FILE), serde_json::to_vec(&m).unwrap()).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::manifest::{sha256_hex, FileEntry};
    use std::io::Write;

    fn write_bundle(root: &Path, ver: &str, files: &[(&str, &[u8])]) {
        let dir = root.join(ver);
        let mut entries = Vec::new();
        for (p, d) in files {
            let t = p.split('/').fold(dir.clone(), |a, s| a.join(s));
            fs::create_dir_all(t.parent().unwrap()).unwrap();
            fs::write(&t, d).unwrap();
            entries.push(FileEntry {
                path: p.to_string(),
                sha256: sha256_hex(d),
                size: d.len() as u64,
            });
        }
        let m = BundleManifest {
            format: 1,
            flowseal_version: ver.into(),
            winws_version: "v72.9".into(),
            source_url: format!(
                "https://github.com/Flowseal/zapret-discord-youtube/releases/download/{ver}/x.zip"
            ),
            source_sha256: "0".repeat(64),
            archive_root: String::new(),
            min_app_version: "0.1.0-alpha.1".into(),
            files: entries,
        };
        fs::write(dir.join(MANIFEST_FILE), serde_json::to_vec(&m).unwrap()).unwrap();
    }

    const BAT: &[u8] = b"\"%BIN%winws.exe\" --wf-tcp=443 --filter-tcp=443 --hostlist=\"%LISTS%list-general.txt\" --dpi-desync=fake\r\n";

    fn std_files() -> Vec<(&'static str, &'static [u8])> {
        vec![
            ("bin/winws.exe", b"exe"),
            ("bin/cygwin1.dll", b"cyg"),
            ("bin/WinDivert.dll", b"dll"),
            ("bin/WinDivert64.sys", b"sys"),
            ("bin/stun.bin", b"fake"),
            ("lists/list-general.txt", b"discord.com\n"),
            ("general.bat", BAT),
        ]
    }

    #[test]
    fn discover_verify_and_detect_tamper() {
        let t = tempfile::tempdir().unwrap();
        let builtin = t.path().join("builtin");
        let updates = t.path().join("updates");
        write_bundle(&builtin, "1.10.4", &std_files());
        write_bundle(&updates, "1.10.10", &std_files());
        let b = discover(&builtin, &updates);
        assert_eq!(
            b.iter().map(|x| x.version()).collect::<Vec<_>>(),
            vec!["1.10.10", "1.10.4"]
        );
        assert!(b[1].check_integrity().is_empty());
        assert!(lock_and_verify(&b[1]).is_ok());
        assert_eq!(b[1].strategy_files().len(), 1);
        let idx = b[1].resource_index();
        assert!(idx.fakes.contains("stun.bin") && idx.lists.contains("list-general-user.txt"));

        fs::write(builtin.join("1.10.4/bin/winws.exe"), b"evil").unwrap();
        assert!(!b[1].check_integrity().is_empty());
        assert!(lock_and_verify(&b[1]).is_err());
    }

    #[test]
    fn min_app_version_is_enforced_with_semver() {
        let mut m = sample_manifest();
        m.min_app_version = "0.1.0-alpha.10".into();
        assert!(check_app_compat_with(&m, "0.1.0-alpha.3").is_err());
        assert!(check_app_compat_with(&m, "0.1.0-alpha.10").is_ok());
        assert!(check_app_compat_with(&m, "0.1.0").is_ok());
        m.min_app_version = "0.2.0".into();
        assert!(check_app_compat_with(&m, "0.1.1").is_err());
        m.min_app_version = "not-a-version".into();
        assert!(check_app_compat_with(&m, "0.1.0").is_err());
    }

    fn sample_manifest() -> BundleManifest {
        BundleManifest {
            format: 1,
            flowseal_version: "1.11.0".into(),
            winws_version: "v72.9".into(),
            source_url:
                "https://github.com/Flowseal/zapret-discord-youtube/releases/download/1.11.0/x.zip"
                    .into(),
            source_sha256: "0".repeat(64),
            archive_root: String::new(),
            min_app_version: "0.1.0-alpha.1".into(),
            files: vec![],
        }
    }

    #[test]
    fn install_requires_valid_signature_and_hashes() {
        let t = tempfile::tempdir().unwrap();
        // build a zip in memory
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::SimpleFileOptions = Default::default();
            for (p, d) in std_files() {
                z.start_file(format!("root/{p}"), opts).unwrap();
                z.write_all(d).unwrap();
            }
            z.finish().unwrap();
        }
        let archive = buf.into_inner();
        let m = BundleManifest {
            format: 1,
            flowseal_version: "1.11.0".into(),
            winws_version: "v72.9".into(),
            source_url:
                "https://github.com/Flowseal/zapret-discord-youtube/releases/download/1.11.0/x.zip"
                    .into(),
            source_sha256: sha256_hex(&archive),
            archive_root: "root/".into(),
            min_app_version: "0.1.0-alpha.1".into(),
            files: std_files()
                .iter()
                .map(|(p, d)| FileEntry {
                    path: p.to_string(),
                    sha256: sha256_hex(d),
                    size: d.len() as u64,
                })
                .collect(),
        };
        let (signed, pk) = sign_with_fresh_key(&m);
        let v = install(&signed, &archive, t.path(), &[&pk]).unwrap();
        assert_eq!(v, "1.11.0");
        assert!(t.path().join("1.11.0/bin/winws.exe").exists());

        // wrong key
        let (_, other_pk) = sign_with_fresh_key(&m);
        assert!(install(&signed, &archive, t.path(), &[&other_pk]).is_err());
        // tampered archive
        let mut bad = archive.clone();
        let n = bad.len();
        bad[n / 2] ^= 0xff;
        assert!(install(&signed, &bad, t.path(), &[&pk]).is_err());
    }

    fn sign_with_fresh_key(m: &BundleManifest) -> (SignedManifest, String) {
        use base64::Engine as _;
        let key = ed25519_dalek::SigningKey::generate(&mut rand_core::OsRng);
        let pk = base64::engine::general_purpose::STANDARD.encode(key.verifying_key().as_bytes());
        (manifest::sign(m, &key), pk)
    }

    #[test]
    fn list_files_are_served_verified_in_chunks_and_nothing_else() {
        use base64::Engine as _;
        use bridge_core::ipc::{Response, LIST_CHUNK};
        let t = tempfile::tempdir().unwrap();
        let big: Vec<u8> = (0..LIST_CHUNK + 1000)
            .map(|i| b"0123456789\r\n"[i % 12])
            .collect();
        let mut files = std_files();
        files.push(("lists/ipset-all.txt.backup", &big));
        write_bundle(t.path(), "1.10.5", &files);
        let all = discover(t.path(), &t.path().join("none"));
        let mut got = Vec::new();
        let mut offset = 0;
        loop {
            let Response::BundleList {
                data, size, sha256, ..
            } = read_bundle_list(&all, "1.10.5", "ipset-all.txt.backup", offset).unwrap()
            else {
                panic!("unexpected response")
            };
            let chunk = base64::engine::general_purpose::STANDARD
                .decode(data)
                .unwrap();
            assert!(chunk.len() <= LIST_CHUNK);
            assert_eq!(sha256, sha256_hex(&big));
            got.extend_from_slice(&chunk);
            offset = got.len() as u64;
            if offset >= size || chunk.is_empty() {
                break;
            }
        }
        assert_eq!(got, big);
        // Only manifest files under lists/; no traversal; other versions refused.
        for bad in [
            "../general.bat",
            "..\\general.bat",
            "general.bat",
            "",
            ".hidden",
            "list-exclude.txt",
        ] {
            assert!(read_bundle_list(&all, "1.10.5", bad, 0).is_err(), "{bad}");
        }
        assert!(read_bundle_list(&all, "9.9.9", "list-general.txt", 0).is_err());
        // A tampered file is refused.
        fs::write(
            t.path()
                .join("1.10.5")
                .join("lists")
                .join("list-general.txt"),
            b"evil.example\n",
        )
        .unwrap();
        assert!(read_bundle_list(&all, "1.10.5", "list-general.txt", 0).is_err());
    }
}
