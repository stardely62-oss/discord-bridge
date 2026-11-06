//! Thin safe wrappers over the Win32 calls the helper needs.
#![allow(clippy::upper_case_acronyms)]

use std::ffi::{c_void, OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, BOOL, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
    SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, IsWellKnownSid,
    WinBuiltinAdministratorsSid, WinLocalSystemSid, ACL, DACL_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenThread, QueryFullProcessImageNameW, ResumeThread, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// Owned Win32 handle closed on drop.
pub struct Owned(pub HANDLE);
unsafe impl Send for Owned {}
unsafe impl Sync for Owned {}
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// A security descriptor built from SDDL (freed with LocalFree).
pub struct SecDesc {
    pub sd: PSECURITY_DESCRIPTOR,
}
unsafe impl Send for SecDesc {}
impl SecDesc {
    pub fn from_sddl(sddl: &str) -> std::io::Result<Self> {
        let w = wide(sddl);
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(SecDesc { sd })
    }
    pub fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.sd,
            bInheritHandle: 0,
        }
    }
}
impl Drop for SecDesc {
    fn drop(&mut self) {
        unsafe { LocalFree(self.sd as _) };
    }
}

/// Full Win32 image path of a process.
pub fn process_image(pid: u32) -> Option<PathBuf> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let h = Owned(h);
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        if QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) == 0 {
            return None;
        }
        Some(PathBuf::from(OsString::from_wide(&buf[..len as usize])))
    }
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .replace('/', "\\")
            .to_lowercase()
    };
    norm(a) == norm(b)
}

#[derive(Debug, Clone)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
}

pub fn list_processes() -> Vec<ProcInfo> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let snap = Owned(snap);
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok: BOOL = Process32FirstW(snap.0, &mut e);
        while ok != 0 {
            let len = e
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(e.szExeFile.len());
            out.push(ProcInfo {
                pid: e.th32ProcessID,
                name: String::from_utf16_lossy(&e.szExeFile[..len]),
            });
            ok = Process32NextW(snap.0, &mut e);
        }
    }
    out
}

/// Resume every thread of a process created with CREATE_SUSPENDED.
pub fn resume_process(pid: u32) -> bool {
    let mut resumed = false;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let snap = Owned(snap);
        let mut e: THREADENTRY32 = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut ok = Thread32First(snap.0, &mut e);
        while ok != 0 {
            if e.th32OwnerProcessID == pid {
                let t = OpenThread(THREAD_SUSPEND_RESUME, 0, e.th32ThreadID);
                if !t.is_null() {
                    let t = Owned(t);
                    if ResumeThread(t.0) != u32::MAX {
                        resumed = true;
                    }
                }
            }
            ok = Thread32Next(snap.0, &mut e);
        }
    }
    resumed
}

/// Directory DACL: only SYSTEM and Administrators, inheritance protected.
pub const PROTECTED_DIR_SDDL: &str = "O:SYD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)";

fn owner_is_trusted(path: &Path) -> Option<bool> {
    let w = wide(path.as_os_str());
    unsafe {
        let mut owner: *mut c_void = null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        let rc = GetNamedSecurityInfoW(
            w.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut sd,
        );
        if rc != 0 {
            return None;
        }
        let trusted = IsWellKnownSid(owner, WinLocalSystemSid) != 0
            || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0;
        LocalFree(sd as _);
        Some(trusted)
    }
}

/// Create (or take over) a directory owned by SYSTEM with a protected DACL.
/// A pre-existing directory owned by an untrusted account is moved aside
/// rather than reused, so an unprivileged user cannot pre-plant files.
pub fn ensure_protected_dir(path: &Path) -> std::io::Result<()> {
    if path.exists() && owner_is_trusted(path) == Some(false) {
        let aside = path.with_extension(format!("untrusted-{}", crate::hlog::now_unix()));
        std::fs::rename(path, aside)?;
    }
    std::fs::create_dir_all(path)?;
    let sd = SecDesc::from_sddl(PROTECTED_DIR_SDDL)?;
    unsafe {
        let mut present: BOOL = 0;
        let mut defaulted: BOOL = 0;
        let mut dacl: *mut ACL = null_mut();
        if GetSecurityDescriptorDacl(sd.sd, &mut present, &mut dacl, &mut defaulted) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut owner: *mut c_void = null_mut();
        if GetSecurityDescriptorOwner(sd.sd, &mut owner, &mut defaulted) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let w = wide(path.as_os_str());
        let rc = SetNamedSecurityInfoW(
            w.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION,
            owner,
            null_mut(),
            dacl,
            null(),
        );
        if rc != 0 {
            return Err(std::io::Error::from_raw_os_error(rc as i32));
        }
    }
    Ok(())
}

pub fn system_dir() -> PathBuf {
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buf = vec![0u16; 260];
    let n = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if n == 0 || n > buf.len() {
        return PathBuf::from(r"C:\Windows\System32");
    }
    PathBuf::from(OsString::from_wide(&buf[..n]))
}

// ---------------- Client identity (impersonation) ----------------

use crate::policy::ClientIdentity;
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{
    GetTokenInformation, RevertToSelf, TokenElevation, TokenSessionId, TokenUser, TOKEN_ELEVATION,
    TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};

/// Reverts impersonation when dropped, even on early return.
struct Impersonation;
impl Drop for Impersonation {
    fn drop(&mut self) {
        unsafe { RevertToSelf() };
    }
}

fn impersonate(pipe: HANDLE) -> std::io::Result<Impersonation> {
    if unsafe { ImpersonateNamedPipeClient(pipe) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(Impersonation)
}

fn token_info<T: Copy>(token: HANDLE, class: i32) -> Option<Vec<u8>> {
    let _ = std::marker::PhantomData::<T>;
    unsafe {
        let mut len = 0u32;
        GetTokenInformation(token, class, null_mut(), 0, &mut len);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        if GetTokenInformation(token, class, buf.as_mut_ptr() as _, len, &mut len) == 0 {
            return None;
        }
        Some(buf)
    }
}

/// Identity of the pipe client taken from its token (not from a PID).
pub fn client_identity(pipe: HANDLE) -> std::io::Result<ClientIdentity> {
    let guard = impersonate(pipe)?;
    let mut token: HANDLE = null_mut();
    let ok = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) };
    drop(guard);
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let token = Owned(token);
    let user =
        token_info::<TOKEN_USER>(token.0, TokenUser).ok_or_else(std::io::Error::last_os_error)?;
    let sid = unsafe {
        let tu = &*(user.as_ptr() as *const TOKEN_USER);
        let mut s: *mut u16 = null_mut();
        if ConvertSidToStringSidW(tu.User.Sid, &mut s) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let len = (0..).take_while(|&i| *s.add(i) != 0).count();
        let out = String::from_utf16_lossy(std::slice::from_raw_parts(s, len));
        LocalFree(s as _);
        out
    };
    let elevated = token_info::<TOKEN_ELEVATION>(token.0, TokenElevation)
        .map(|b| unsafe { (*(b.as_ptr() as *const TOKEN_ELEVATION)).TokenIsElevated != 0 })
        .unwrap_or(false);
    let session_id = token_info::<u32>(token.0, TokenSessionId)
        .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
        .unwrap_or(u32::MAX);
    Ok(ClientIdentity {
        sid,
        session_id,
        elevated,
    })
}

/// Read a file with the *client's* access rights (impersonation), bounded in size.
pub fn read_file_as_client(pipe: HANDLE, path: &Path, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let _guard = impersonate(pipe).map_err(|e| format!("олицетворение клиента: {e}"))?;
    let f = std::fs::File::open(path).map_err(|e| format!("файл обновления недоступен: {e}"))?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    if len > max {
        return Err(format!("файл обновления больше {max} байт"));
    }
    let mut buf = Vec::with_capacity(len as usize);
    f.take(max + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > max {
        return Err("файл обновления слишком большой".into());
    }
    Ok(buf)
}

/// Whether a logon session id still exists (for ownership expiry).
pub fn session_alive(id: u32) -> bool {
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSEnumerateSessionsW, WTSFreeMemory, WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW,
    };
    unsafe {
        let mut info: *mut WTS_SESSION_INFOW = null_mut();
        let mut count = 0u32;
        if WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut info, &mut count) == 0 {
            return true; // unknown → keep ownership (safe default)
        }
        let alive = std::slice::from_raw_parts(info, count as usize)
            .iter()
            .any(|s| s.SessionId == id);
        WTSFreeMemory(info as _);
        alive
    }
}
