//! Telegram WS Proxy for Discord Bridge.
//!
//! Discord Bridge runs Flowseal's tg-ws-proxy network engine (Python, built
//! headless from a pinned upstream commit, see `telegram-proxy/`) as an
//! ordinary child process of the user — independent of Zapret, WinWS and the
//! privileged helper service:
//!
//! * [`component`] — finds the bundled engine and checks its SHA-256 against
//!   the value compiled into the app and the manifest next to it;
//! * [`settings`] — persisted options (port, autostart, relay choices);
//! * [`secret`] — the MTProto secret: generated with the OS CSPRNG, stored in
//!   a separate file, never logged, never serialised;
//! * [`link`] — the `tg://proxy` link and its strict validation;
//! * [`protocol`] — the JSON lines the engine prints on stdout;
//! * [`manager`] — start / stop / restart, state from the real child process.

pub mod component;
pub mod link;
pub mod manager;
pub mod protocol;
pub mod secret;
pub mod settings;
#[cfg(windows)]
mod win;

pub use component::{Component, ComponentProblem};
pub use manager::{ProxyManager, ProxyState, ProxyStatus};
pub use secret::Secret;
pub use settings::TelegramProxySettings;

/// Name of the engine executable in the installed resources.
pub const EXE_NAME: &str = "discord-bridge-tg-ws-proxy.exe";
/// Manifest written by the build next to the executable.
pub const MANIFEST_NAME: &str = "tg-ws-proxy.json";
/// The only listen address: the proxy is for this computer's Telegram only.
pub const LISTEN_HOST: &str = "127.0.0.1";
