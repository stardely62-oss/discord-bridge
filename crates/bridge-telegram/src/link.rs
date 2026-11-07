//! The `tg://proxy` link for Telegram Desktop.
//!
//! Same form as tg-ws-proxy prints (`proxy/tg_ws_proxy.py`):
//! `tg://proxy?server=<host>&port=<port>&secret=dd<32 hex>`. The `dd` prefix
//! selects MTProto with random padding, which the engine accepts.

use crate::secret::Secret;
use crate::LISTEN_HOST;

pub fn tg_link(port: u16, secret: &Secret) -> String {
    format!(
        "tg://proxy?server={LISTEN_HOST}&port={port}&secret=dd{}",
        secret.expose_hex()
    )
}

/// Only links of exactly the form above, for this computer, are opened.
pub fn validate_tg_link(link: &str) -> Result<(), String> {
    let bad = || Err("ссылка не является локальной ссылкой Telegram Proxy".to_string());
    let Some(rest) = link.strip_prefix(&format!("tg://proxy?server={LISTEN_HOST}&port=")) else {
        return bad();
    };
    let Some((port, secret)) = rest.split_once("&secret=dd") else {
        return bad();
    };
    let port_ok = !port.is_empty()
        && port.len() <= 5
        && port.bytes().all(|b| b.is_ascii_digit())
        && !port.starts_with('0')
        && port
            .parse::<u32>()
            .is_ok_and(|p| (1024..=65535).contains(&p));
    if !port_ok || Secret::parse_hex(secret).is_err() || secret.len() != 32 {
        return bad();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_has_the_upstream_form() {
        // Test value built at runtime (not a real secret).
        let hex = "0123456789abcdef".repeat(2);
        let s = Secret::parse_hex(&hex).unwrap();
        let l = tg_link(1443, &s);
        assert_eq!(
            l,
            format!("tg://proxy?server=127.0.0.1&port=1443&secret=dd{hex}")
        );
        validate_tg_link(&l).unwrap();
    }

    #[test]
    fn anything_else_is_refused() {
        let ok = "0123456789abcdef".repeat(2);
        for bad in [
            format!("tg://proxy?server=evil.example&port=1443&secret=dd{ok}"),
            format!("tg://proxy?server=127.0.0.1&port=80&secret=dd{ok}"),
            format!("tg://proxy?server=127.0.0.1&port=70000&secret=dd{ok}"),
            format!("tg://proxy?server=127.0.0.1&port=01443&secret=dd{ok}"),
            format!("tg://proxy?server=127.0.0.1&port=1443&secret=ee{ok}"),
            format!("tg://proxy?server=127.0.0.1&port=1443&secret=dd{ok}&x=1"),
            format!(
                "tg://proxy?server=127.0.0.1&port=1443&secret=dd{}",
                ok.to_uppercase()
            ),
            format!("tg://socks?server=127.0.0.1&port=1443&secret=dd{ok}"),
            "https://t.me/proxy".into(),
            "file:///C:/Windows/System32/calc.exe".into(),
        ] {
            assert!(validate_tg_link(&bad).is_err(), "{bad}");
        }
    }
}
