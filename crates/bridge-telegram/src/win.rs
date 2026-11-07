//! Windows specifics: a Job Object that holds the engine (and the Python
//! process the PyInstaller bootloader starts), resuming a process created
//! suspended, and the owner of a listening TCP port.

use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::ptr::null;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, NO_ERROR};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows_sys::Win32::Networking::WinSock::AF_INET;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

/// Creation flags for the engine: no console window, suspended until it is
/// in the job.
pub const CREATE_FLAGS: u32 = 0x0800_0000 /* CREATE_NO_WINDOW */ | 0x0000_0004 /* CREATE_SUSPENDED */;

pub struct Job(HANDLE);

// The handle is only used through the Win32 job functions, which are thread safe.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// A job whose processes all end when its last handle closes — also when
    /// Bridge crashes.
    pub fn new() -> io::Result<Job> {
        unsafe {
            let h = CreateJobObjectW(null(), null());
            if h.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Job(h);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }

    pub fn assign(&self, process: HANDLE) -> io::Result<()> {
        unsafe {
            if AssignProcessToJobObject(self.0, process) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }

    pub fn terminate(&self) {
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }

    /// Process ids currently in the job.
    pub fn pids(&self) -> Vec<u32> {
        // Header (2 × u32) + up to 64 ids.
        let mut buf = [0usize; 2 + 64];
        unsafe {
            if QueryInformationJobObject(
                self.0,
                JobObjectBasicProcessIdList,
                buf.as_mut_ptr() as *mut c_void,
                size_of::<[usize; 66]>() as u32,
                std::ptr::null_mut(),
            ) == 0
            {
                return vec![];
            }
        }
        // JOBOBJECT_BASIC_PROCESS_ID_LIST: two u32 counts, then ULONG_PTR ids.
        let listed = buf[0] >> 32;
        buf[1..1 + listed.min(65)]
            .iter()
            .map(|p| *p as u32)
            .collect()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Resumes every thread of a process created with CREATE_SUSPENDED.
pub fn resume_process(pid: u32) -> io::Result<()> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let mut te: THREADENTRY32 = zeroed();
        te.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut resumed = 0;
        let mut ok = Thread32First(snap, &mut te) != 0;
        while ok {
            if te.th32OwnerProcessID == pid {
                let t = OpenThread(THREAD_SUSPEND_RESUME, 0, te.th32ThreadID);
                if !t.is_null() {
                    if ResumeThread(t) != u32::MAX {
                        resumed += 1;
                    }
                    CloseHandle(t);
                }
            }
            ok = Thread32Next(snap, &mut te) != 0;
        }
        CloseHandle(snap);
        if resumed == 0 {
            return Err(io::Error::other("no thread of the engine could be resumed"));
        }
    }
    Ok(())
}

/// Owner process of the IPv4 TCP listener on 127.0.0.1:`port` (or 0.0.0.0).
pub fn listener_pid(port: u16) -> Option<u32> {
    unsafe {
        let mut size: u32 = 0;
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize + 1024];
        let mut len = buf.len() as u32;
        if GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        ) != NO_ERROR
        {
            return None;
        }
        let n = u32::from_ne_bytes(buf[0..4].try_into().ok()?) as usize;
        let row_size = size_of::<MIB_TCPROW_OWNER_PID>();
        // dwNumEntries is followed by the rows (4-byte aligned).
        for i in 0..n {
            let off = 4 + i * row_size;
            if off + row_size > buf.len() {
                break;
            }
            let row: MIB_TCPROW_OWNER_PID =
                std::ptr::read_unaligned(buf.as_ptr().add(off) as *const MIB_TCPROW_OWNER_PID);
            let local_port = u16::from_be((row.dwLocalPort & 0xffff) as u16);
            let addr = u32::from_be(row.dwLocalAddr);
            if local_port == port && (addr == 0x7f00_0001 || addr == 0) {
                return Some(row.dwOwningPid);
            }
        }
        None
    }
}
