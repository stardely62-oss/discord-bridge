//! Atomic, crash-safe file writes and corruption recovery.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Write to `path.tmp`, fsync, then rename over `path`. The previous version
/// is kept as `path.bak` so a corrupted file can be recovered.
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent"))?;
    fs::create_dir_all(dir)?;
    let tmp = with_suffix(path, "tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    if path.exists() {
        let bak = with_suffix(path, "bak");
        // Keep the last good copy; ignore failures (e.g. read-only backup).
        let _ = fs::copy(path, &bak);
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".");
    s.push(suffix);
    PathBuf::from(s)
}

/// Copy a file aside before a schema migration rewrites it.
pub fn backup_before_migration(path: &Path, from_version: u64) -> io::Result<PathBuf> {
    let dest = with_suffix(path, &format!("v{from_version}.bak"));
    fs::copy(path, &dest)?;
    Ok(dest)
}

/// Read and parse `path`; if it is corrupted, fall back to `path.bak` and
/// quarantine the corrupted file as `path.corrupt`. Returns `(value, recovered)`.
pub fn read_with_recovery<T, F>(path: &Path, parse: F) -> io::Result<Option<(T, bool)>>
where
    F: Fn(&[u8]) -> Result<T, String>,
{
    match fs::read(path) {
        Ok(bytes) => match parse(&bytes) {
            Ok(v) => Ok(Some((v, false))),
            Err(_) => {
                let _ = fs::rename(path, with_suffix(path, "corrupt"));
                let bak = with_suffix(path, "bak");
                match fs::read(&bak).ok().and_then(|b| parse(&b).ok()) {
                    Some(v) => {
                        let _ = fs::copy(&bak, path);
                        Ok(Some((v, true)))
                    }
                    None => Ok(None),
                }
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_recover() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        write_atomic(&p, b"{\"a\":1}").unwrap();
        write_atomic(&p, b"{\"a\":2}").unwrap();
        fs::write(&p, b"{garbage").unwrap();
        let parse =
            |b: &[u8]| serde_json::from_slice::<serde_json::Value>(b).map_err(|e| e.to_string());
        let (v, recovered) = read_with_recovery(&p, parse).unwrap().unwrap();
        assert!(recovered);
        assert_eq!(v["a"], 1);
        assert!(with_suffix(&p, "corrupt").exists());
    }

    #[test]
    fn missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.json");
        let parse = |_: &[u8]| Ok::<_, String>(());
        assert!(read_with_recovery(&p, parse).unwrap().is_none());
    }
}
