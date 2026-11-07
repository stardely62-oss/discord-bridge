//! JSON lines printed by the headless engine on stdout
//! (`telegram-proxy/tgproxy_headless.py`).

use serde::{Deserialize, Serialize};

/// Counters of the engine (`proxy/stats.py`), cumulative since its start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineStats {
    pub connections_total: u64,
    pub connections_active: u64,
    pub connections_ws: u64,
    pub connections_h2: u64,
    pub connections_tcp_fallback: u64,
    pub connections_cfproxy: u64,
    pub connections_fronting: u64,
    pub connections_bad: u64,
    pub connections_masked: u64,
    pub ws_errors: u64,
    pub bytes_up: u64,
    pub bytes_down: u64,
}

impl EngineStats {
    /// Client connections that were relayed towards Telegram by any transport.
    pub fn relayed(&self) -> u64 {
        self.connections_ws
            + self.connections_h2
            + self.connections_tcp_fallback
            + self.connections_cfproxy
            + self.connections_fronting
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum EngineEvent {
    #[serde(rename_all = "camelCase")]
    Ready {
        host: String,
        port: u16,
        version: String,
        #[serde(default)]
        fallback_cfproxy: bool,
    },
    Status {
        listening: bool,
        #[serde(default)]
        stats: EngineStats,
    },
    Error {
        code: String,
        #[serde(default)]
        message: String,
    },
    Stopped {
        #[serde(default)]
        stats: EngineStats,
    },
}

/// One stdout line → event (unknown or malformed lines are ignored).
pub fn parse_line(line: &str) -> Option<EngineEvent> {
    let line = line.trim();
    if !line.starts_with('{') || line.len() > 64 * 1024 {
        return None;
    }
    serde_json::from_str(line).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_lines_are_parsed() {
        assert_eq!(
            parse_line(
                r#"{"event":"ready","host":"127.0.0.1","port":1443,"version":"1.11.4","runner":"1","fallbackCfproxy":false}"#
            ),
            Some(EngineEvent::Ready {
                host: "127.0.0.1".into(),
                port: 1443,
                version: "1.11.4".into(),
                fallback_cfproxy: false
            })
        );
        let st = parse_line(
            r#"{"event":"status","listening":true,"at":1,"stats":{"connections_total":3,"connections_active":1,"connections_ws":2,"bytes_down":10,"new_counter":5}}"#,
        );
        let Some(EngineEvent::Status { listening, stats }) = st else {
            panic!()
        };
        assert!(listening);
        assert_eq!(
            (stats.connections_total, stats.relayed(), stats.bytes_down),
            (3, 2, 10)
        );
        assert_eq!(
            parse_line(r#"{"event":"error","code":"addressInUse","message":"OSError: errno 98"}"#),
            Some(EngineEvent::Error {
                code: "addressInUse".into(),
                message: "OSError: errno 98".into()
            })
        );
        for junk in ["", "hello", "{", r#"{"event":"unknown"}"#] {
            assert_eq!(parse_line(junk), None, "{junk}");
        }
    }
}
