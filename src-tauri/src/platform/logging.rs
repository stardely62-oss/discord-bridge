//! Diagnostic log with redaction and a hard size cap.
//!
//! * user profile paths and the account name are masked;
//! * the file is rotated at 1 MiB with a single backup (≤ 2 MiB total);
//! * nothing about Discord accounts or traffic content is ever logged.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MAX_LOG: u64 = 1024 * 1024;

#[derive(Clone)]
pub struct LogStore {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    path: PathBuf,
    enabled: bool,
    secrets: Vec<(String, String)>,
}

impl LogStore {
    pub fn new(path: PathBuf) -> Self {
        let mut secrets = Vec::new();
        for (var, mask) in [
            ("USERPROFILE", "%USERPROFILE%"),
            ("LOCALAPPDATA", "%LOCALAPPDATA%"),
            ("APPDATA", "%APPDATA%"),
        ] {
            if let Ok(v) = std::env::var(var) {
                if v.len() > 3 {
                    secrets.push((v, mask.to_string()));
                }
            }
        }
        if let Ok(u) = std::env::var("USERNAME") {
            if u.len() > 1 {
                secrets.push((u, "<user>".into()));
            }
        }
        // Longest first so paths are masked before the bare user name.
        secrets.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
        if let Some(p) = path.parent() {
            let _ = fs::create_dir_all(p);
        }
        LogStore {
            inner: Arc::new(Mutex::new(Inner {
                path,
                enabled: true,
                secrets,
            })),
        }
    }

    pub fn set_enabled(&self, on: bool) {
        if let Ok(mut i) = self.inner.lock() {
            i.enabled = on;
        }
    }

    pub fn redact(&self, s: &str) -> String {
        let i = self.inner.lock().expect("log lock");
        redact_with(&i.secrets, s)
    }

    pub fn append(&self, line: &str) {
        let Ok(i) = self.inner.lock() else { return };
        if !i.enabled {
            return;
        }
        let line = redact_with(&i.secrets, line);
        if fs::metadata(&i.path)
            .map(|m| m.len() > MAX_LOG)
            .unwrap_or(false)
        {
            let _ = fs::rename(&i.path, i.path.with_extension("log.1"));
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&i.path) {
            let _ = f.write_all(line.as_bytes());
            if !line.ends_with('\n') {
                let _ = f.write_all(b"\n");
            }
        }
    }

    pub fn tail(&self, max_lines: usize) -> Vec<String> {
        let Ok(i) = self.inner.lock() else {
            return vec![];
        };
        let mut text = fs::read_to_string(i.path.with_extension("log.1")).unwrap_or_default();
        text.push_str(&fs::read_to_string(&i.path).unwrap_or_default());
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        lines[lines.len().saturating_sub(max_lines)..].to_vec()
    }

    pub fn clear(&self) -> io::Result<()> {
        let i = self.inner.lock().expect("log lock");
        let _ = fs::remove_file(i.path.with_extension("log.1"));
        fs::write(&i.path, b"")
    }
}

fn redact_with(secrets: &[(String, String)], s: &str) -> String {
    let mut out = s.to_string();
    for (secret, mask) in secrets {
        // case-insensitive replace for Windows paths
        let lower = out.to_lowercase();
        let needle = secret.to_lowercase();
        if needle.is_empty() || lower.len() != out.len() || !lower.contains(&needle) {
            continue;
        }
        let mut res = String::new();
        let mut idx = 0;
        while let Some(pos) = lower[idx..].find(&needle) {
            res.push_str(&out[idx..idx + pos]);
            res.push_str(mask);
            idx += pos + needle.len();
        }
        res.push_str(&out[idx..]);
        out = res;
    }
    // Defensive: never persist anything that looks like a Discord token.
    mask_tokens(&out)
}

fn mask_tokens(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let parts: Vec<&str> = w.split('.').collect();
            let looks = parts.len() == 3
                && parts[0].len() >= 20
                && parts[2].len() >= 20
                && w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
            if looks {
                "<masked>"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `tracing` writer that forwards formatted lines to the LogStore.
pub struct StoreWriter(pub LogStore);

impl Write for StoreWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.append(&String::from_utf8_lossy(buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogStore {
    type Writer = StoreWriter;
    fn make_writer(&'a self) -> Self::Writer {
        StoreWriter(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_paths_and_tokens() {
        let secrets = vec![
            (r"C:\Users\Ivan".to_string(), "%USERPROFILE%".to_string()),
            ("Ivan".to_string(), "<user>".to_string()),
        ];
        let s = redact_with(&secrets, r"open c:\users\ivan\AppData\x by Ivan");
        assert_eq!(s, r"open %USERPROFILE%\AppData\x by <user>");
        let t = redact_with(
            &[],
            &format!("token {} end", ["MTIzNDU2Nzg5MDEyMzQ1Njc4OTAx", "GaBcDe", "abcdefghijklmnopqrstuvwxyz012345"].join(".")),
        );
        assert!(t.contains("<masked>") && !t.contains("GaBcDe"));
    }

    #[test]
    fn caps_size() {
        let d = tempfile::tempdir().unwrap();
        let l = LogStore::new(d.path().join("app.log"));
        let line = "x".repeat(10_000);
        for _ in 0..300 {
            l.append(&line);
        }
        let total: u64 = ["app.log", "app.log.1"]
            .iter()
            .filter_map(|f| fs::metadata(d.path().join(f)).ok())
            .map(|m| m.len())
            .sum();
        assert!(total <= 2 * MAX_LOG + 20_000, "{total}");
        l.clear().unwrap();
        assert!(l.tail(10).is_empty());
    }
}
