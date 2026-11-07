//! The MTProto proxy secret: 16 random bytes, 32 lowercase hex characters
//! (what tg-ws-proxy expects; the link adds the `dd` prefix for the
//! randomised-padding mode).
//!
//! The secret is never `Debug`-printed, `Display`ed or serialised. It is kept
//! in its own file in the user's profile, apart from the settings (which can
//! be exported with a report).
//!
//! Permissions: on Unix the file is 0600. On Windows it gets an explicit,
//! protected DACL (no inherited entries): full control for the current user,
//! SYSTEM and Administrators — the principals a user profile normally has, so
//! backups and administration keep working, but nothing broader that a parent
//! directory might grant (Users, Everyone, Authenticated Users). The DACL is
//! set on the temporary file before the secret is written into it, kept by
//! the rename, and re-applied when an existing file is loaded.

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
                #[cfg(windows)]
                acl::restrict(&self.path).map_err(|e| format!("права файла секрета: {e}"))?;
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
            // Restrict the still empty file before the secret goes in.
            #[cfg(windows)]
            acl::restrict(&tmp).map_err(|e| format!("права файла секрета: {e}"))?;
            f.write_all(format!("{}\n", s.expose_hex()).as_bytes())
                .and_then(|_| f.sync_all())
                .map_err(|e| format!("секрет не сохранён: {e}"))?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| format!("секрет не сохранён: {e}"))
    }
}

#[cfg(windows)]
pub mod acl {
    //! Explicit protected DACL for the secret file (see the module docs).
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
        ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
        SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        GetSecurityDescriptorDacl, GetTokenInformation, TokenUser, ACL, DACL_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain([0]).collect()
    }

    fn from_wide(p: *const u16) -> String {
        let mut n = 0;
        unsafe {
            while *p.add(n) != 0 {
                n += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
        }
    }

    fn os_err() -> String {
        std::io::Error::last_os_error().to_string()
    }

    /// String SID of the current user (from the process token).
    pub fn current_user_sid() -> Result<String, String> {
        unsafe {
            let mut tok: HANDLE = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
                return Err(os_err());
            }
            let mut len = 0u32;
            GetTokenInformation(tok, TokenUser, null_mut(), 0, &mut len);
            let mut buf = vec![0u8; len as usize];
            let ok = GetTokenInformation(tok, TokenUser, buf.as_mut_ptr().cast(), len, &mut len);
            CloseHandle(tok);
            if ok == 0 {
                return Err(os_err());
            }
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut s: *mut u16 = null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut s) == 0 {
                return Err(os_err());
            }
            let out = from_wide(s);
            LocalFree(s.cast());
            Ok(out)
        }
    }

    /// Sets the DACL of `path` from SDDL (`D:P…` = protected, no inheritance).
    pub fn apply_sddl(path: &Path, sddl: &str) -> Result<(), String> {
        let sddl = wide(sddl);
        let p = wide(&path.to_string_lossy());
        let protected = if String::from_utf16_lossy(&sddl).starts_with("D:P") {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            0
        };
        unsafe {
            let mut sd: PSECURITY_DESCRIPTOR = null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                null_mut(),
            ) == 0
            {
                return Err(os_err());
            }
            let (mut present, mut defaulted) = (0, 0);
            let mut dacl: *mut ACL = null_mut();
            let r = if GetSecurityDescriptorDacl(sd, &mut present, &mut dacl, &mut defaulted) == 0
                || present == 0
            {
                Err(os_err())
            } else {
                let e = SetNamedSecurityInfoW(
                    p.as_ptr(),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | protected,
                    null_mut(),
                    null_mut(),
                    dacl,
                    null(),
                );
                if e == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::from_raw_os_error(e as i32).to_string())
                }
            };
            LocalFree(sd.cast());
            r
        }
    }

    /// Full control for the current user, SYSTEM and Administrators only;
    /// inheritance from the parent directory is cut off.
    pub fn restrict(path: &Path) -> Result<(), String> {
        let user = current_user_sid()?;
        apply_sddl(
            path,
            &format!("D:P(A;;FA;;;{user})(A;;FA;;;SY)(A;;FA;;;BA)"),
        )
    }

    /// The file's DACL in SDDL form (for checks; contains no secret).
    pub fn dacl_sddl(path: &Path) -> Result<String, String> {
        let p = wide(&path.to_string_lossy());
        unsafe {
            let mut sd: PSECURITY_DESCRIPTOR = null_mut();
            let e = GetNamedSecurityInfoW(
                p.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                null_mut(),
                null_mut(),
                &mut sd,
            );
            if e != 0 {
                return Err(std::io::Error::from_raw_os_error(e as i32).to_string());
            }
            let mut s: *mut u16 = null_mut();
            let ok = ConvertSecurityDescriptorToStringSecurityDescriptorW(
                sd,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut s,
                null_mut(),
            );
            LocalFree(sd.cast());
            if ok == 0 {
                return Err(os_err());
            }
            let out = from_wide(s);
            LocalFree(s.cast());
            Ok(out)
        }
    }

    /// Trustees of the access-allowed entries of an SDDL DACL.
    pub fn allowed_trustees(sddl: &str) -> Vec<String> {
        sddl.split('(')
            .skip(1)
            .filter_map(|ace| {
                let f: Vec<&str> = ace.trim_end_matches(')').split(';').collect();
                (f.first() == Some(&"A")).then(|| f.last().unwrap_or(&"").to_string())
            })
            .collect()
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

    /// Windows: only the user, SYSTEM and Administrators can open the secret
    /// file — even when the parent folder grants Everyone access; no
    /// temporary file is left; broader permissions are repaired on load.
    #[cfg(windows)]
    #[test]
    fn the_secret_file_is_private_on_windows() {
        use super::acl::{allowed_trustees, apply_sddl, current_user_sid, dacl_sddl};
        let user = current_user_sid().unwrap();
        let check = |p: &Path| {
            let sddl = dacl_sddl(p).unwrap();
            assert!(
                sddl.starts_with("D:P"),
                "protected DACL without inheritance: {sddl}"
            );
            let t = allowed_trustees(&sddl);
            assert!(!t.is_empty());
            // SDDL writes well-known accounts as aliases: the built-in
            // Administrator account (RID 500, the CI runner) appears as "LA".
            let is_user = |x: &str| x == user || (x == "LA" && user.ends_with("-500"));
            for x in &t {
                assert!(
                    is_user(x) || x == "SY" || x == "BA",
                    "unexpected trustee {x} in {sddl}"
                );
            }
            sddl
        };
        let d = tempfile::tempdir().unwrap();
        let parent = d.path().join("cfg");
        std::fs::create_dir(&parent).unwrap();
        // A careless parent folder: Everyone may read everything below it.
        apply_sddl(&parent, &format!("D:(A;OICI;FA;;;{user})(A;OICI;FR;;;WD)")).unwrap();
        assert!(allowed_trustees(&dacl_sddl(&parent).unwrap())
            .iter()
            .any(|t| t == "WD"));
        let st = SecretStore::new(&parent);
        let secret = st.load_or_create().unwrap();
        let sddl = check(st.path());
        assert!(!sddl.contains(&secret.expose_hex()));
        let tmp: Vec<String> = std::fs::read_dir(&parent)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(tmp.is_empty(), "{tmp:?}");
        st.regenerate().unwrap();
        check(st.path());
        // An existing file with broad permissions is repaired on load.
        apply_sddl(st.path(), &format!("D:(A;;FA;;;{user})(A;;FR;;;WD)")).unwrap();
        assert!(allowed_trustees(&dacl_sddl(st.path()).unwrap())
            .iter()
            .any(|t| t == "WD"));
        st.load_or_create().unwrap();
        check(st.path());
    }

    #[cfg(windows)]
    #[test]
    fn sddl_trustees_are_parsed() {
        let t = super::acl::allowed_trustees(
            "D:P(A;;FA;;;S-1-5-21-1-2-3-1001)(A;;FA;;;SY)(D;;FA;;;WD)(A;;FA;;;BA)",
        );
        assert_eq!(t, vec!["S-1-5-21-1-2-3-1001", "SY", "BA"]);
    }
}
