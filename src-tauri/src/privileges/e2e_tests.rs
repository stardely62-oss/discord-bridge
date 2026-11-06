//! Windows end-to-end tests of the **production** client
//! ([`HelperClient`]) against the **real** helper pipe server
//! (`bridge_helper::server::serve_on`) on a real named pipe.
//!
//! The server only differs from production by its pipe name, its ACL
//! (Authenticated Users instead of INTERACTIVE, since CI runs in a service
//! session) and the accepted client image (this test executable).

use super::{HelperClient, HelperError};
use bridge_core::ipc::{read_frame, write_frame, EngineState, Request, Response, PROTOCOL_VERSION};
use bridge_helper::paths::Paths;
use bridge_helper::server::{serve_on, Helper};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::Security::{
    CreateRestrictedToken, GetTokenInformation, TokenElevation, LUA_TOKEN, TOKEN_ADJUST_DEFAULT,
    TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, GetCurrentProcess, GetExitCodeProcess, OpenProcessToken,
    WaitForSingleObject, PROCESS_INFORMATION, STARTUPINFOW,
};

const ROLE_ENV: &str = "DB_CLIENT_E2E_PIPE";
const TEST_SDDL: &str = "O:BAD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;AU)";

struct Server {
    _dir: tempfile::TempDir,
    pipe: String,
    helper: Arc<Helper>,
}

fn start_server(tag: &str, client_exe: PathBuf) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths {
        install_dir: dir.path().to_path_buf(),
        data_dir: dir.path().join("data"),
        client_exe_override: Some(client_exe),
    };
    let helper = Arc::new(Helper::new(paths));
    let pipe = format!(
        r"\\.\pipe\DiscordBridgeClientE2E-{}-{tag}",
        std::process::id()
    );
    let (h2, p2) = (helper.clone(), pipe.clone());
    std::thread::spawn(move || {
        if let Err(e) = serve_on(h2, &p2, TEST_SDDL) {
            eprintln!("test server failed: {e}");
        }
    });
    // Wait for the first pipe instance with the production client itself:
    // NotRunning means "not created yet".
    let probe = HelperClient::custom(&pipe, PROTOCOL_VERSION);
    for _ in 0..100 {
        match probe.call(&Request::Hello {
            protocol: PROTOCOL_VERSION,
        }) {
            Err(HelperError::NotRunning) => std::thread::sleep(Duration::from_millis(50)),
            _ => break,
        }
    }
    Server {
        _dir: dir,
        pipe,
        helper,
    }
}

fn self_elevated() -> bool {
    unsafe {
        let mut tok = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
            return true;
        }
        let mut e: TOKEN_ELEVATION = std::mem::zeroed();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            tok,
            TokenElevation,
            &mut e as *mut _ as _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        ) != 0;
        CloseHandle(tok);
        !ok || e.TokenIsElevated != 0
    }
}

#[test]
fn production_client_handshake_and_requests() {
    if std::env::var(ROLE_ENV).is_ok() {
        return; // running as the non-elevated child
    }
    let me = std::env::current_exe().unwrap();
    let srv = start_server("main", me.clone());
    let client = HelperClient::custom(&srv.pipe, PROTOCOL_VERSION);

    // 1. Hello: exactly one handshake, its answer is returned.
    let before = srv.helper.handshakes();
    match client.call(&Request::Hello {
        protocol: PROTOCOL_VERSION,
    }) {
        Ok(Response::Hello { protocol, .. }) => assert_eq!(protocol, PROTOCOL_VERSION),
        other => panic!("Hello: {other:?}"),
    }
    assert_eq!(srv.helper.handshakes(), before + 1, "no double Hello");
    // The async wrapper used by the app goes through the same path.
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    assert!(rt.block_on(client.hello()).is_ok(), "HelperClient::hello");

    // 2. Status: automatic Hello + Status. The service rejects any
    //    connection whose first frame is not Hello, so a Status answer
    //    proves both went over the same connection.
    let before = srv.helper.handshakes();
    match client.call(&Request::Status) {
        Ok(Response::Status { status }) => {
            assert_eq!(status.state, EngineState::Stopped);
            assert!(status.can_control);
        }
        other => panic!("Status: {other:?}"),
    }
    assert_eq!(srv.helper.handshakes(), before + 1);

    // 3. ListBundles.
    match client.call(&Request::ListBundles) {
        Ok(Response::Bundles { .. }) => {}
        other => panic!("ListBundles: {other:?}"),
    }

    // 4. Sequential calls: each opens a new connection with its own handshake.
    let before = srv.helper.handshakes();
    for i in 0..5 {
        match client.call(&Request::Status) {
            Ok(Response::Status { .. }) => {}
            other => panic!("Status #{i}: {other:?}"),
        }
    }
    assert_eq!(
        srv.helper.handshakes(),
        before + 5,
        "one handshake per call"
    );

    // 5. Protocol mismatch: clear error, the command is not sent.
    let before = srv.helper.handshakes();
    let wrong = HelperClient::custom(&srv.pipe, PROTOCOL_VERSION + 100);
    match wrong.call(&Request::Status) {
        Err(e @ HelperError::Protocol(_)) => {
            assert!(e.to_string().contains("версии приложения и службы"), "{e}")
        }
        other => panic!("protocol mismatch: {other:?}"),
    }
    assert_eq!(
        srv.helper.handshakes(),
        before,
        "rejected Hello not counted"
    );

    // 6. Command without Hello (raw frames, bypassing the client): the
    //    service refuses.
    let mut raw = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&srv.pipe)
        .unwrap();
    write_frame(&mut raw, &Request::Status).unwrap();
    let answer: Result<Response, _> = read_frame(&mut raw);
    match answer {
        Ok(Response::Error { code, .. }) => assert_eq!(code, "bad_request"),
        other => panic!("no-Hello: {other:?}"),
    }
    drop(raw);

    // 7. Unauthorized client (image is not the accepted executable).
    let strict = start_server(
        "strict",
        PathBuf::from(r"C:\nonexistent\discord-bridge.exe"),
    );
    let c = HelperClient::custom(&strict.pipe, PROTOCOL_VERSION);
    match c.call(&Request::Status) {
        Err(HelperError::Unauthorized(_)) => {}
        other => panic!("unauthorized: {other:?}"),
    }
    assert_eq!(strict.helper.handshakes(), 0);

    // 8. The same production client from a non-elevated (LUA) process, as
    //    the real UI runs.
    assert_eq!(run_child(&srv.pipe), 0, "non-elevated production client");
}

/// Child entry: production client, non-elevated, Hello + Status + ListBundles.
#[test]
fn client_role() {
    let Ok(pipe) = std::env::var(ROLE_ENV) else {
        return;
    };
    let code = (|| -> i32 {
        if self_elevated() {
            return 4;
        }
        let c = HelperClient::custom(&pipe, PROTOCOL_VERSION);
        match c.call(&Request::Status) {
            Ok(Response::Status { status })
                if status.can_control && status.state == EngineState::Stopped => {}
            other => {
                eprintln!("child Status: {other:?}");
                return 6;
            }
        }
        match c.call(&Request::ListBundles) {
            Ok(Response::Bundles { .. }) => 0,
            other => {
                eprintln!("child ListBundles: {other:?}");
                7
            }
        }
    })();
    std::process::exit(code);
}

fn run_child(pipe: &str) -> u32 {
    std::env::set_var(ROLE_ENV, pipe);
    let exe = std::env::current_exe().unwrap();
    let cmd = format!(
        "\"{}\" privileges::e2e_tests::client_role --exact --nocapture --test-threads=1",
        exe.display()
    );
    let mut cmdw: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
    let code = unsafe {
        let mut tok = null_mut();
        assert!(
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_DUPLICATE | TOKEN_QUERY | TOKEN_ASSIGN_PRIMARY | TOKEN_ADJUST_DEFAULT,
                &mut tok
            ) != 0
        );
        let mut lua = null_mut();
        assert!(
            CreateRestrictedToken(tok, LUA_TOKEN, 0, null(), 0, null(), 0, null(), &mut lua) != 0,
            "CreateRestrictedToken: {}",
            std::io::Error::last_os_error()
        );
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessAsUserW(
            lua,
            null(),
            cmdw.as_mut_ptr(),
            null(),
            null(),
            0,
            0,
            null(),
            null(),
            &si,
            &mut pi,
        );
        assert!(
            ok != 0,
            "CreateProcessAsUserW: {}",
            std::io::Error::last_os_error()
        );
        assert_eq!(
            WaitForSingleObject(pi.hProcess, 60_000),
            WAIT_OBJECT_0,
            "child timed out"
        );
        let mut code = 0u32;
        GetExitCodeProcess(pi.hProcess, &mut code);
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        CloseHandle(lua);
        CloseHandle(tok);
        code
    };
    std::env::remove_var(ROLE_ENV);
    code
}
