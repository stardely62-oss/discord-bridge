//! Bridge privileged helper.
//!
//! Runs as the `DiscordBridgeHelper` Windows service (LocalSystem) and exposes
//! a fixed set of operations over a local named pipe to the unprivileged UI.
//!
//! Command line (used by the installer, requires elevation):
//!   discord-bridge-helper.exe --install-service
//!   discord-bridge-helper.exe --uninstall-service
//!   discord-bridge-helper.exe --stop-engine     (stops the service and so the engine)
//!   discord-bridge-helper.exe --tcp-changed     (exit 10 if we changed TCP timestamps)
//!   discord-bridge-helper.exe --restore-tcp     (restore the recorded TCP timestamps value)

#[cfg(windows)]
use bridge_helper::service;

#[cfg(windows)]
fn report(o: &bridge_helper::stopcheck::StopOutcome) -> i32 {
    if o.exit_code() == 0 {
        println!("{}", o.message());
    } else {
        eprintln!("{}", o.message());
    }
    o.exit_code()
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        Some("--install-service") => service::install().map(|_| 0),
        // Exit codes: 0 stopped, 3 service did not stop, 4 our winws still runs.
        Some("--uninstall-service") => service::uninstall().map(|o| report(&o)),
        Some("--stop-engine") => Ok(report(&service::stop_service())),
        // Uninstaller: exit code 10 if Bridge changed TCP timestamps
        // and the value is still the one it set.
        Some("--tcp-changed") => Ok(if service::tcp_changed_by_us() { 10 } else { 0 }),
        Some("--restore-tcp") => service::restore_tcp().map(|_| 0),
        Some(other) => {
            eprintln!("unknown argument {other}");
            Ok(2)
        }
        None => service::run_dispatcher().map(|_| 0),
    };
    match code {
        Ok(c) => std::process::exit(c),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1)
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("discord-bridge-helper runs only on Windows");
    std::process::exit(1);
}
