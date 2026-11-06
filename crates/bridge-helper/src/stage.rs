//! Immutable engine copies.
//!
//! winws and the WinDivert driver are never run from the install directory:
//! the installer must be able to replace those files at any time, and a
//! loaded driver keeps its image file locked. Before a start, the verified
//! bundle is copied into `engines/<version>-<manifest hash>` under the
//! protected ProgramData directory and run from there.
//!
//! - Content-addressed: the same bundle (e.g. reinstalling the same version)
//!   maps to the same directory, which is reused after verification and never
//!   overwritten in place.
//! - Atomic: files are copied into a temporary directory, verified, then the
//!   directory is renamed into place.
//! - A damaged copy is moved aside (not overwritten) and staged again.
//! - Old copies are removed only when nothing holds them (best effort).

use crate::bundles::Bundle;
use std::fs;
use std::path::{Path, PathBuf};

/// Directory name for a bundle: `<version>-<first 16 hex of manifest sha256>`.
pub fn stage_key(bundle: &Bundle) -> String {
    let json = serde_json::to_vec(&bundle.manifest).unwrap_or_default();
    let hash = bridge_core::manifest::sha256_hex(&json);
    let ver: String = bundle
        .version()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{ver}-{}", &hash[..16])
}

fn unique(base: &Path, prefix: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    base.join(format!("{prefix}-{}-{nanos}", std::process::id()))
}

fn copy_verified(src: &Bundle, dst: &Path) -> Result<(), String> {
    for f in &src.manifest.files {
        let data = fs::read(src.path_of(&f.path)).map_err(|e| format!("{}: {e}", f.path))?;
        src.manifest
            .verify_file(&f.path, &data)
            .map_err(|e| e.to_string())?;
        let to = f.path.split('/').fold(dst.to_path_buf(), |p, s| p.join(s));
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&to, &data).map_err(|e| format!("{}: {e}", f.path))?;
    }
    Ok(())
}

/// Returns the bundle as run from its immutable copy under `engines`.
pub fn stage_bundle(src: &Bundle, engines: &Path) -> Result<Bundle, String> {
    fs::create_dir_all(engines).map_err(|e| format!("{}: {e}", engines.display()))?;
    let target = engines.join(stage_key(src));
    let staged = |dir: PathBuf| Bundle {
        dir,
        builtin: src.builtin,
        manifest: src.manifest.clone(),
    };
    if target.exists() {
        let b = staged(target.clone());
        if b.check_integrity().is_empty() {
            return Ok(b);
        }
        // Damaged copy: move it aside instead of writing into it.
        fs::rename(&target, unique(engines, ".damaged")).map_err(|e| {
            format!("повреждённая копия движка занята и не может быть заменена: {e}")
        })?;
    }
    let tmp = unique(engines, ".tmp");
    let result = copy_verified(src, &tmp).and_then(|_| {
        let b = staged(tmp.clone());
        let problems = b.check_integrity();
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("; "))
        }
    });
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("не удалось подготовить копию движка: {e}"));
    }
    match fs::rename(&tmp, &target) {
        Ok(()) => Ok(staged(target)),
        Err(e) => {
            let _ = fs::remove_dir_all(&tmp);
            // Another start staged it concurrently: use it if it verifies.
            let b = staged(target);
            if b.check_integrity().is_empty() {
                Ok(b)
            } else {
                Err(format!("не удалось подготовить копию движка: {e}"))
            }
        }
    }
}

/// Removes engine copies and leftovers except `keep`. Directories still in
/// use (a running winws, a loaded driver) cannot be deleted and are skipped.
pub fn gc_engines(engines: &Path, keep: &[&str]) -> usize {
    let Ok(rd) = fs::read_dir(engines) else {
        return 0;
    };
    let mut removed = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if keep.contains(&name.as_str()) || !e.path().is_dir() {
            continue;
        }
        if fs::remove_dir_all(e.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundles::{discover, Bundle};

    fn bundle(root: &Path) -> Bundle {
        crate::bundles::tests_support::write_std_bundle(root, "1.10.4");
        discover(root, &root.join("none")).remove(0)
    }

    #[test]
    fn stages_a_verified_copy_and_reuses_it() {
        let t = tempfile::tempdir().unwrap();
        let src = bundle(&t.path().join("builtin"));
        let engines = t.path().join("engines");
        let a = stage_bundle(&src, &engines).unwrap();
        assert!(a.dir.starts_with(&engines));
        assert!(a.check_integrity().is_empty());
        assert!(a.path_of("bin/WinDivert64.sys").exists());
        let mtime = fs::metadata(a.path_of("bin/winws.exe"))
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        // Same bundle again (e.g. reinstalling the same version): same dir, not rewritten.
        let b = stage_bundle(&src, &engines).unwrap();
        assert_eq!(a.dir, b.dir);
        assert_eq!(
            fs::metadata(b.path_of("bin/winws.exe"))
                .unwrap()
                .modified()
                .unwrap(),
            mtime
        );
        // The source can now be replaced freely: the staged copy is independent.
        fs::write(src.path_of("bin/WinDivert64.sys"), b"new installer write").unwrap();
        assert!(a.check_integrity().is_empty());
    }

    #[test]
    fn damaged_copy_is_moved_aside_and_restaged() {
        let t = tempfile::tempdir().unwrap();
        let src = bundle(&t.path().join("builtin"));
        let engines = t.path().join("engines");
        let a = stage_bundle(&src, &engines).unwrap();
        fs::write(a.path_of("bin/winws.exe"), b"tampered").unwrap();
        let b = stage_bundle(&src, &engines).unwrap();
        assert_eq!(a.dir, b.dir);
        assert!(b.check_integrity().is_empty());
        let names: Vec<String> = fs::read_dir(&engines)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(names.iter().any(|n| n.starts_with(".damaged-")));
    }

    #[test]
    fn tampered_source_is_never_staged() {
        let t = tempfile::tempdir().unwrap();
        let src = bundle(&t.path().join("builtin"));
        fs::write(src.path_of("bin/winws.exe"), b"evil").unwrap();
        let engines = t.path().join("engines");
        assert!(stage_bundle(&src, &engines).is_err());
        // No half-written copy is left behind.
        assert_eq!(fs::read_dir(&engines).unwrap().count(), 0);
    }

    #[test]
    fn gc_keeps_the_current_copy() {
        let t = tempfile::tempdir().unwrap();
        let src = bundle(&t.path().join("builtin"));
        let engines = t.path().join("engines");
        let a = stage_bundle(&src, &engines).unwrap();
        fs::create_dir_all(engines.join("1.10.3-0123456789abcdef/bin")).unwrap();
        fs::create_dir_all(engines.join(".tmp-1-2")).unwrap();
        let key = a.dir.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(gc_engines(&engines, &[&key]), 2);
        assert!(a.dir.exists());
    }
}
