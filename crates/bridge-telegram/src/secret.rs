//! The MTProto proxy secret: 16 random bytes, 32 lowercase hex characters
//! (what tg-ws-proxy expects; the link adds the `dd` prefix for the
//! randomised-padding mode).
//!
//! The secret is never `Debug`-printed, `Display`ed or serialised. It is kept
//! in its own file in the user's profile, apart from the settings (which can
//! be exported with a report), with owner-only permissions where the OS
//! allows it (the Windows profile directory is already private to the user).

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, PartialEq, Eq)]
pub struct Secret([u8; 16]);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl Secret {
    /// 16 bytes from the operating system's CSPRNG.
    pub fn generate() -> Result<Secret, String> {
        let mut b = [0u8; 16];
        getrandom::getrandom(&mut b).map_err(|e| format!("генератор случайных чисел: {e}"))?;
        Ok(Secret(b))
    }

    /// Exactly 32 lowercase hex characters.
    pub fn parse_hex(s: &str) -> Result<Secret, String> {
        let s = s.trim();
        if s.len() != 32
            || !s
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err("секрет должен состоять из 32 строчных шестнадцатеричных символов".into());
        }
        let mut b = [0u8; 16];
        hex::decode_to_slice(s, &mut b).map_err(|e| e.to_string())?;
        Ok(Secret(b))
    }

    /// The hex form. Only for the engine's stdin and the link.
    pub fn expose_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Replaces every occurrence of this secret (any case) in `text`.
    pub fn redact(&self, text: &str) -> String {
        let hex = self.expose_hex();
        let lower = text.to_ascii_lowercase();
        if !lower.contains(&hex) {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while let Some(pos) = lower[i..].find(&hex) {
            out.push_str(&text[i..i + pos]);
            out.push_str("<secret>");
            i += pos + hex.len();
        }
        out.push_str(&text[i..]);
        out
    }
}

/// The secret file (`telegram-proxy.secret` in the app config directory).
pub struct SecretStore {
    path: PathBuf,
}

impl SecretStore {
    pub fn new(dir: &Path) -> SecretStore {
        SecretStore {
            path: dir.join("telegram-proxy.secret"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The stored secret, or a new one (stored) if there is none or the file
    /// is damaged.
    pub fn load_or_create(&self) -> Result<Secret, String> {
        if let Ok(text) = std::fs::read_to_string(&self.path) {
            if let Ok(s) = Secret::parse_hex(&text) {
                return Ok(s);
            }
        }
        let s = Secret::generate()?;
        self.store(&s)?;
        Ok(s)
    }

    /// Replaces the secret (a new link is needed in Telegram afterwards).
    pub fn regenerate(&self) -> Result<Secret, String> {
        let s = Secret::generate()?;
        self.store(&s)?;
        Ok(s)
    }

    fn store(&self, s: &Secret) -> Result<(), String> {
        let dir = self.path.parent().ok_or("нет каталога для секрета")?;
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("secret.tmp");
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts
                .open(&tmp)
                .map_err(|e| format!("секрет не сохранён: {e}"))?;
            f.write_all(format!("{}\n", s.expose_hex()).as_bytes())
                .and_then(|_| f.sync_all())
                .map_err(|e| format!("секрет не сохранён: {e}"))?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("секрет не сохранён: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_secrets_are_16_random_bytes_in_lowercase_hex() {
        let a = Secret::generate().unwrap();
        let b = Secret::generate().unwrap();
        assert_ne!(a, b);
        let h = a.expose_hex();
        assert_eq!(h.len(), 32);
        assert_eq!(Secret::parse_hex(&h).unwrap(), a);
    }

    #[test]
    fn malformed_secrets_are_refused() {
        for bad in [
            "",
            "dd0123456789abcdef0123456789abcdef",
            "0123456789ABCDEF0123456789ABCDEF",
            "0123",
            "zz23456789abcdef0123456789abcdef",
        ] {
            assert!(Secret::parse_hex(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_secret_never_appears_in_debug_output() {
        let s = Secret::generate().unwrap();
        let dbg = format!("{s:?} {:?}", Some(&s));
        assert!(!dbg.contains(&s.expose_hex()));
        assert!(dbg.contains("redacted"));
    }

    #[test]
    fn redaction_covers_every_case_and_occurrence() {
        let s = Secret::parse_hex("0123456789abcdef0123456789abcdef").unwrap();
        let t = format!(
            "a dd{} b {} c",
            s.expose_hex(),
            s.expose_hex().to_uppercase()
        );
        let r = s.redact(&t);
        assert_eq!(r, "a dd<secret> b <secret> c");
    }

    #[test]
    fn the_store_keeps_the_secret_across_loads_and_replaces_it_on_request() {
        let d = tempfile::tempdir().unwrap();
        let st = SecretStore::new(d.path());
        let a = st.load_or_create().unwrap();
        assert_eq!(st.load_or_create().unwrap(), a);
        let b = st.regenerate().unwrap();
        assert_ne!(a, b);
        assert_eq!(st.load_or_create().unwrap(), b);
        // A damaged file yields a fresh secret, never a crash.
        std::fs::write(st.path(), "garbage").unwrap();
        let c = st.load_or_create().unwrap();
        assert_ne!(c, b);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(st.path()).unwrap().permissions().mode() & 0o077,
                0
            );
        }
    }
}
