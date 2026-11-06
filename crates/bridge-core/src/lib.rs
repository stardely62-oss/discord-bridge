//! Platform-independent core of Discord Bridge.
//!
//! Everything here is pure logic that can be unit-tested on any OS:
//! the strategy schema, the safe BAT importer (it never executes anything),
//! validation against the known winws option table, rendering of the final
//! argument vector, strategy diff, schema migrations, signed bundle manifests,
//! atomic file writes and the IPC protocol shared with the privileged helper.

pub mod atomic;
pub mod auto;
pub mod batparse;
pub mod catalog;
pub mod diff;
pub mod ipc;
pub mod ipset;
pub mod lists;
pub mod manifest;
pub mod packs;
pub mod ports;
pub mod services;
pub mod strategy;
pub mod templates;
pub mod updates;
pub mod versions;

pub use strategy::{Arg, ArgValue, Rule, Source, Strategy};

/// Version of the Flowseal release this build was audited against.
pub const FLOWSEAL_BASELINE_VERSION: &str = "1.10.4";
/// winws version shipped with that Flowseal release (`winws --version` reports v72.9).
pub const WINWS_BASELINE_VERSION: &str = "v72.9";

/// A resource name (fake packet or list file). Only plain file names are
/// allowed: no separators, no `..`, no drive letters, no ADS (`:`).
pub fn is_safe_resource_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_names() {
        assert!(is_safe_resource_name("tls_clienthello_www_google_com.bin"));
        assert!(is_safe_resource_name("list-general.txt"));
        assert!(!is_safe_resource_name("../winws.exe"));
        assert!(!is_safe_resource_name("..\\x"));
        assert!(!is_safe_resource_name("C:x"));
        assert!(!is_safe_resource_name("a/b"));
        assert!(!is_safe_resource_name("list.txt:stream"));
        assert!(!is_safe_resource_name(".hidden"));
        assert!(!is_safe_resource_name(""));
    }
}
