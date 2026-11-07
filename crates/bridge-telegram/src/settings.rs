//! Persisted Telegram Proxy options (part of the app settings, without the
//! secret). Older settings files have no `telegramProxy` object: the defaults
//! apply, nothing else changes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TelegramProxySettings {
    /// Start the proxy when Bridge starts (independent of the bypass).
    pub autostart: bool,
    pub port: u16,
    /// Use Cloudflare relays (CfProxy) when a direct WebSocket to Telegram
    /// fails. Off by default: third-party relays see connection metadata.
    pub fallback_cfproxy: bool,
    /// Media over HTTP/2 through the relays (only with the fallback).
    pub h2_media: bool,
    /// Own Cloudflare domains instead of the upstream relay list.
    pub cfproxy_domains: Vec<String>,
    /// Own Cloudflare Worker domains.
    pub cfproxy_worker_domains: Vec<String>,
}

impl Default for TelegramProxySettings {
    fn default() -> Self {
        TelegramProxySettings {
            autostart: false,
            port: 1443,
            fallback_cfproxy: false,
            h2_media: true,
            cfproxy_domains: vec![],
            cfproxy_worker_domains: vec![],
        }
    }
}

fn domain_ok(d: &str) -> bool {
    let labels: Vec<&str> = d.split('.').collect();
    d.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        && labels
            .last()
            .is_some_and(|t| t.len() >= 2 && t.bytes().any(|b| b.is_ascii_alphabetic()))
}

impl TelegramProxySettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.port < 1024 {
            return Err("порт Telegram Proxy должен быть от 1024 до 65535".into());
        }
        for (name, list) in [
            ("домены Cloudflare", &self.cfproxy_domains),
            ("домены Cloudflare Worker", &self.cfproxy_worker_domains),
        ] {
            if list.len() > 32 {
                return Err(format!("{name}: не больше 32"));
            }
            if let Some(bad) = list.iter().find(|d| !domain_ok(d)) {
                return Err(format!(
                    "{name}: «{}» не является доменом",
                    bad.chars().take(80).collect::<String>()
                ));
            }
        }
        Ok(())
    }

    /// Relays actually used: only with the fallback switched on.
    pub fn effective_relays(&self) -> (bool, &[String], &[String]) {
        if self.fallback_cfproxy {
            (true, &self.cfproxy_domains, &self.cfproxy_worker_domains)
        } else {
            (false, &[], &[])
        }
    }

    /// The single JSON line sent to the engine's stdin.
    pub fn engine_config(&self, secret: &crate::Secret) -> String {
        let (fallback, domains, workers) = self.effective_relays();
        serde_json::json!({
            "host": crate::LISTEN_HOST,
            "port": self.port,
            "secret": secret.expose_hex(),
            "fallbackCfproxy": fallback,
            "h2Media": fallback && self.h2_media,
            "cfproxyDomains": domains,
            "cfproxyWorkerDomains": workers,
        })
        .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_local_port_1443_without_third_party_relays() {
        let s = TelegramProxySettings::default();
        assert_eq!(s.port, 1443);
        assert!(!s.autostart);
        assert!(!s.fallback_cfproxy);
        s.validate().unwrap();
        // Missing in an older settings file: defaults.
        let old: TelegramProxySettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, s);
        let partial: TelegramProxySettings = serde_json::from_str(r#"{"port":2443}"#).unwrap();
        assert_eq!(partial.port, 2443);
        assert!(!partial.fallback_cfproxy);
    }

    #[test]
    fn invalid_values_are_refused() {
        let mut s = TelegramProxySettings {
            port: 80,
            ..Default::default()
        };
        assert!(s.validate().is_err());
        s.port = 1443;
        s.cfproxy_domains = vec!["not a domain".into()];
        assert!(s.validate().is_err());
        s.cfproxy_domains = vec!["relay.example.com".into()];
        s.validate().unwrap();
    }

    #[test]
    fn relays_are_sent_only_when_the_fallback_is_chosen() {
        let secret = crate::Secret::parse_hex("0123456789abcdef0123456789abcdef").unwrap();
        let mut s = TelegramProxySettings {
            cfproxy_domains: vec!["relay.example.com".into()],
            ..Default::default()
        };
        let off: serde_json::Value = serde_json::from_str(&s.engine_config(&secret)).unwrap();
        assert_eq!(off["fallbackCfproxy"], false);
        assert_eq!(off["h2Media"], false);
        assert_eq!(off["cfproxyDomains"], serde_json::json!([]));
        assert_eq!(off["host"], "127.0.0.1");
        s.fallback_cfproxy = true;
        let on: serde_json::Value = serde_json::from_str(&s.engine_config(&secret)).unwrap();
        assert_eq!(on["fallbackCfproxy"], true);
        assert_eq!(
            on["cfproxyDomains"],
            serde_json::json!(["relay.example.com"])
        );
        // The secret travels only in this stdin line, never in Debug output.
        assert!(!format!("{s:?}").contains(&secret.expose_hex()));
    }
}
