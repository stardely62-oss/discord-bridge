//! Platform glue: process listing, redacted logging, tray.

pub mod logging;
pub mod tray;

#[derive(Debug, Clone)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
}

#[cfg(windows)]
pub fn processes() -> Vec<Proc> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut e);
        while ok != 0 {
            let len = e
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(e.szExeFile.len());
            out.push(Proc {
                pid: e.th32ProcessID,
                name: String::from_utf16_lossy(&e.szExeFile[..len]),
            });
            ok = Process32NextW(snap, &mut e);
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(not(windows))]
pub fn processes() -> Vec<Proc> {
    Vec::new()
}

/// Full image path of a process, if it can be queried.
#[cfg(windows)]
pub fn process_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) != 0;
        CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[cfg(not(windows))]
pub fn process_path(_pid: u32) -> Option<String> {
    None
}

/// Reopens a downloaded file so that it cannot be changed, replaced or
/// deleted while the returned handle lives (on Windows: read sharing only),
/// and checks its SHA-256 **through that same handle**. What is launched
/// afterwards is therefore exactly the bytes that were verified.
pub fn lock_and_verify(path: &std::path::Path, sha256: &str) -> Result<std::fs::File, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ only: no writer and no delete/rename while we hold it.
        o.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    }
    let mut f = o
        .open(path)
        .map_err(|e| format!("не удалось открыть скачанный установщик: {e}"))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("не удалось прочитать скачанный установщик: {e}"))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != sha256 {
        return Err("скачанный установщик изменился после проверки — запуск отменён".into());
    }
    Ok(f)
}

#[cfg(test)]
mod lock_tests {
    use super::lock_and_verify;

    #[test]
    fn only_the_verified_bytes_pass() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("setup.exe");
        std::fs::write(&p, b"installer").unwrap();
        let good = bridge_core::manifest::sha256_hex(b"installer");
        let held = lock_and_verify(&p, &good).expect("verified");
        assert!(lock_and_verify(&p, &"0".repeat(64)).is_err());
        assert!(lock_and_verify(&dir.path().join("missing.exe"), &good).is_err());
        // While the handle lives, Windows refuses to change or delete the file.
        #[cfg(windows)]
        {
            assert!(std::fs::write(&p, b"other").is_err());
            assert!(std::fs::remove_file(&p).is_err());
            assert!(std::fs::rename(&p, dir.path().join("x.exe")).is_err());
        }
        drop(held);
        assert!(std::fs::write(&p, b"other").is_ok());
    }
}
