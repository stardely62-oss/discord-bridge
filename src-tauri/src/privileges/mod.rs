//! Client side of the privileged helper (PrivilegedHelper).
//!
//! The UI never runs elevated. Every privileged operation is one of the
//! fixed `Request` variants sent over the local named pipe. Before trusting
//! the pipe, the client checks that the pipe object is owned by LocalSystem or Administrators,
//! so an unprivileged process squatting the pipe name is rejected.
//!
//! Connection protocol (enforced by the service): every connection starts
//! with `Hello`. [`HelperClient::call`] is the single entry point: it opens a
//! connection, performs the handshake and only then sends the request on the
//! same connection. Commands never perform their own handshake.

use bridge_core::ipc::{Request, Response, PIPE_NAME, PROTOCOL_VERSION};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum HelperError {
    #[error("служба Discord Bridge Helper не запущена или не установлена")]
    NotRunning,
    #[error("канал службы не принадлежит системе или администраторам — соединение отклонено")]
    Untrusted,
    #[error("служба отклонила подключение: {0}")]
    Unauthorized(String),
    #[error("версии приложения и службы не совпадают — переустановите Bridge ({0})")]
    Protocol(String),
    #[error("не удалось установить связь со службой: {0}")]
    Handshake(String),
    #[error("ошибка связи со службой: {0}")]
    Io(String),
    #[error("служба вернула ошибку: {0}")]
    Remote(String),
    #[error("доступно только в Windows")]
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct HelperClient {
    pipe: String,
    protocol: u32,
}

impl Default for HelperClient {
    fn default() -> Self {
        HelperClient {
            pipe: PIPE_NAME.to_string(),
            protocol: PROTOCOL_VERSION,
        }
    }
}

impl HelperClient {
    /// Client for a non-default pipe / protocol (tests only).
    #[cfg(test)]
    pub(crate) fn custom(pipe: &str, protocol: u32) -> Self {
        HelperClient {
            pipe: pipe.to_string(),
            protocol,
        }
    }

    /// Blocking request/response over a fresh, handshaken connection.
    /// Call from `spawn_blocking`.
    pub fn call(&self, req: &Request) -> Result<Response, HelperError> {
        imp::call(&self.pipe, self.protocol, req)
    }

    pub async fn request(&self, req: Request) -> Result<Response, HelperError> {
        let me = self.clone();
        tokio::task::spawn_blocking(move || me.call(&req))
            .await
            .map_err(|e| HelperError::Io(e.to_string()))?
    }

    pub async fn hello(&self) -> Result<String, HelperError> {
        match self
            .request(Request::Hello {
                protocol: self.protocol,
            })
            .await?
        {
            Response::Hello { helper_version, .. } => Ok(helper_version),
            Response::Error { message, .. } => Err(HelperError::Remote(message)),
            _ => Err(HelperError::Io("неожиданный ответ".into())),
        }
    }
}

/// Interprets the answer to the opening `Hello`. Platform independent so it
/// is unit-tested everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
fn check_hello(sent: u32, resp: Response) -> Result<Response, HelperError> {
    match resp {
        Response::Hello { protocol, .. } if protocol == sent => Ok(resp),
        Response::Hello { protocol, .. } => Err(HelperError::Protocol(format!(
            "приложение {sent}, служба {protocol}"
        ))),
        Response::Error { code, message } => Err(match code.as_str() {
            "unauthorized" | "forbidden" => HelperError::Unauthorized(message),
            "protocol" => HelperError::Protocol(format!("приложение {sent}: {message}")),
            "busy" => HelperError::Handshake(message),
            _ => HelperError::Handshake(format!("{code}: {message}")),
        }),
        other => Err(HelperError::Handshake(format!(
            "неожиданный ответ на Hello: {}",
            serde_json::to_string(&other).unwrap_or_default()
        ))),
    }
}

#[cfg(windows)]
mod imp {
    use super::{check_hello, HelperError};
    use bridge_core::ipc::{read_frame, write_frame, Request, Response};
    use std::fs::File;
    use std::os::windows::io::AsRawHandle;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{LocalFree, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY};
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows_sys::Win32::Security::{
        IsWellKnownSid, WinBuiltinAdministratorsSid, WinLocalSystemSid, OWNER_SECURITY_INFORMATION,
    };

    fn owned_by_system(h: *mut core::ffi::c_void) -> bool {
        unsafe {
            let mut owner = std::ptr::null_mut();
            let mut sd = std::ptr::null_mut();
            let rc = GetSecurityInfo(
                h,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut sd,
            );
            if rc != 0 {
                return false;
            }
            // SYSTEM, or Administrators (the default owner for objects created
            // by a LocalSystem token). A non-elevated process can create
            // neither, so a squatter is still rejected.
            let ok = IsWellKnownSid(owner, WinLocalSystemSid) != 0
                || IsWellKnownSid(owner, WinBuiltinAdministratorsSid) != 0;
            LocalFree(sd as _);
            ok
        }
    }

    /// The only place that talks to the service: connect, verify the pipe
    /// owner, handshake, then send `req` on the same connection. On any
    /// handshake failure the request is not sent and the connection is
    /// closed (dropped).
    pub fn call(pipe: &str, protocol: u32, req: &Request) -> Result<Response, HelperError> {
        let mut f = connect(pipe)?;
        let hello_protocol = match req {
            Request::Hello { protocol } => *protocol,
            _ => protocol,
        };
        write_frame(
            &mut f,
            &Request::Hello {
                protocol: hello_protocol,
            },
        )
        .map_err(|e| HelperError::Handshake(e.to_string()))?;
        let resp: Response =
            read_frame(&mut f).map_err(|e| HelperError::Handshake(e.to_string()))?;
        let hello = check_hello(hello_protocol, resp)?;
        if matches!(req, Request::Hello { .. }) {
            return Ok(hello);
        }
        write_frame(&mut f, req).map_err(|e| HelperError::Io(e.to_string()))?;
        let resp: Response = read_frame(&mut f).map_err(|e| HelperError::Io(e.to_string()))?;
        if let Response::Error { code, message } = &resp {
            if code == "unauthorized" {
                return Err(HelperError::Unauthorized(message.clone()));
            }
        }
        Ok(resp)
    }

    fn connect(pipe: &str) -> Result<File, HelperError> {
        for _ in 0..20 {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(pipe)
            {
                Ok(f) => {
                    if !owned_by_system(f.as_raw_handle() as _) {
                        return Err(HelperError::Untrusted);
                    }
                    return Ok(f);
                }
                Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => {
                    return Err(HelperError::NotRunning)
                }
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                    std::thread::sleep(Duration::from_millis(100))
                }
                Err(e) => return Err(HelperError::Io(e.to_string())),
            }
        }
        Err(HelperError::Io("канал занят".into()))
    }
}

#[cfg(not(windows))]
mod imp {
    use super::HelperError;
    use bridge_core::ipc::{Request, Response};
    pub fn call(_pipe: &str, _protocol: u32, _req: &Request) -> Result<Response, HelperError> {
        Err(HelperError::Unsupported)
    }
}

#[cfg(all(test, windows))]
mod e2e_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_answer_is_checked() {
        let ok = Response::Hello {
            protocol: PROTOCOL_VERSION,
            helper_version: "x".into(),
        };
        assert!(check_hello(PROTOCOL_VERSION, ok).is_ok());
        let other = Response::Hello {
            protocol: PROTOCOL_VERSION + 1,
            helper_version: "x".into(),
        };
        assert!(matches!(
            check_hello(PROTOCOL_VERSION, other),
            Err(HelperError::Protocol(_))
        ));
        let denied = Response::Error {
            code: "unauthorized".into(),
            message: "no".into(),
        };
        assert!(matches!(
            check_hello(PROTOCOL_VERSION, denied),
            Err(HelperError::Unauthorized(_))
        ));
        let proto = Response::Error {
            code: "protocol".into(),
            message: "mismatch".into(),
        };
        assert!(matches!(
            check_hello(PROTOCOL_VERSION, proto),
            Err(HelperError::Protocol(_))
        ));
        assert!(matches!(
            check_hello(PROTOCOL_VERSION, Response::Ok),
            Err(HelperError::Handshake(_))
        ));
    }
}
