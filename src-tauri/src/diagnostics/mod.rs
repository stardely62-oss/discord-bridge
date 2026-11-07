//! NetworkDiagnostics. Every status has a concrete basis: a DNS answer, an
//! HTTP response, a helper check, or an explicit user confirmation. Voice is
//! never reported as working without the user confirming it.

use bridge_core::ipc::{EngineState, EngineStatus, HelperDiagnostics};
use bridge_core::ports::{PortItem, PortSpec};
use bridge_core::strategy::Strategy;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warning,
    Error,
    Unknown,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    pub id: String,
    pub group: String,
    pub title: String,
    pub status: CheckStatus,
    pub detail: String,
    pub hint: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VoiceState {
    /// The strategy has no UDP rules for Discord voice.
    NoUdpRules,
    NotChecked,
    UserConfirmed,
    UserReportedBroken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsReport {
    pub at: u64,
    pub strategy_id: Option<String>,
    pub engine_running: bool,
    /// None when no HTTPS check ran.
    pub https_ok: Option<bool>,
    pub voice: VoiceState,
    pub items: Vec<CheckItem>,
}

pub const HTTPS_TARGETS: &[(&str, &str)] = &[
    ("discord.com", "https://discord.com/api/v9/gateway"),
    ("gateway.discord.gg", "https://gateway.discord.gg/"),
    ("cdn.discordapp.com", "https://cdn.discordapp.com/"),
    ("updates.discord.com", "https://updates.discord.com/"),
];

fn item(
    id: &str,
    group: &str,
    title: &str,
    status: CheckStatus,
    detail: impl Into<String>,
) -> CheckItem {
    CheckItem {
        id: id.into(),
        group: group.into(),
        title: title.into(),
        status,
        detail: detail.into(),
        hint: None,
        duration_ms: None,
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(6))
        .user_agent(concat!("DiscordBridge/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

pub async fn dns_check(host: &str) -> CheckItem {
    let t = Instant::now();
    let res =
        tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((host, 443))).await;
    let mut it = match res {
        Ok(Ok(addrs)) => {
            let v: Vec<String> = addrs.map(|a| a.ip().to_string()).take(3).collect();
            if v.is_empty() {
                item(
                    &format!("dns:{host}"),
                    "dns",
                    host,
                    CheckStatus::Error,
                    "DNS не вернул адресов",
                )
            } else if v
                .iter()
                .any(|ip| ip == "0.0.0.0" || ip == "127.0.0.1" || ip == "::")
            {
                let mut i = item(
                    &format!("dns:{host}"),
                    "dns",
                    host,
                    CheckStatus::Warning,
                    format!("подозрительный ответ DNS: {}", v.join(", ")),
                );
                i.hint = Some("Возможна DNS-блокировка или запись в hosts. Bridge не меняет DNS и hosts автоматически.".into());
                i
            } else {
                item(
                    &format!("dns:{host}"),
                    "dns",
                    host,
                    CheckStatus::Ok,
                    v.join(", "),
                )
            }
        }
        Ok(Err(e)) => item(
            &format!("dns:{host}"),
            "dns",
            host,
            CheckStatus::Error,
            format!("ошибка DNS: {e}"),
        ),
        Err(_) => item(
            &format!("dns:{host}"),
            "dns",
            host,
            CheckStatus::Error,
            "тайм-аут DNS (5 с)",
        ),
    };
    it.duration_ms = Some(t.elapsed().as_millis() as u64);
    it
}

pub async fn https_check(client: &reqwest::Client, host: &str, url: &str) -> CheckItem {
    let t = Instant::now();
    let id = format!("https:{host}");
    let mut it = match client.get(url).send().await {
        // Any HTTP response proves the TLS handshake and request went through.
        Ok(r) => item(
            &id,
            "https",
            host,
            CheckStatus::Ok,
            format!("ответ HTTP {}", r.status().as_u16()),
        ),
        Err(e) => {
            let kind = if e.is_timeout() {
                "тайм-аут — характерно для блокировки DPI"
            } else if e.is_connect() {
                "не удалось установить соединение"
            } else {
                "ошибка TLS/HTTP"
            };
            let mut i = item(
                &id,
                "https",
                host,
                CheckStatus::Error,
                format!("{kind}: {}", short_err(&e)),
            );
            i.hint = Some("Попробуйте другую стратегию или режим Auto.".into());
            i
        }
    };
    it.duration_ms = Some(t.elapsed().as_millis() as u64);
    it
}

fn short_err(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(x) = src {
        s = x.to_string();
        src = x.source();
    }
    s.chars().take(160).collect()
}

/// Discord API check: `GET /api/v9/gateway` must return HTTP 200 with a JSON
/// body `{"url": "wss://..."}`. Unlike "any HTTP response" this shows the
/// request reached Discord's API and came back intact.
pub async fn api_check(client: &reqwest::Client) -> CheckItem {
    let t = Instant::now();
    let mut it = match client
        .get("https://discord.com/api/v9/gateway")
        .send()
        .await
    {
        Ok(r) if r.status().as_u16() == 200 => match r.json::<serde_json::Value>().await {
            Ok(v)
                if v.get("url")
                    .and_then(|u| u.as_str())
                    .is_some_and(|u| u.starts_with("wss://")) =>
            {
                item(
                    "api:discord",
                    "https",
                    "Discord API",
                    CheckStatus::Ok,
                    "API отвечает корректно (gateway url получен)",
                )
            }
            Ok(_) => item(
                "api:discord",
                "https",
                "Discord API",
                CheckStatus::Error,
                "ответ API не похож на ответ Discord (возможна подмена)",
            ),
            Err(e) => item(
                "api:discord",
                "https",
                "Discord API",
                CheckStatus::Error,
                format!("ответ API оборван или повреждён: {}", short_err(&e)),
            ),
        },
        Ok(r) => item(
            "api:discord",
            "https",
            "Discord API",
            CheckStatus::Error,
            format!("API вернул HTTP {}", r.status().as_u16()),
        ),
        Err(e) => item(
            "api:discord",
            "https",
            "Discord API",
            CheckStatus::Error,
            format!("запрос не прошёл: {}", short_err(&e)),
        ),
    };
    it.duration_ms = Some(t.elapsed().as_millis() as u64);
    it
}

/// Result of the connectivity criterion used after a switch and by Auto.
pub struct Probe {
    pub ok: bool,
    pub items: Vec<CheckItem>,
}

/// Decide from the collected results. Missing results (a failed task,
/// a timeout) are failures — an empty set is never a success.
pub fn probe_verdict(expected: usize, items: &[CheckItem]) -> bool {
    items.len() == expected
        && !items.is_empty()
        && items.iter().all(|i| i.status == CheckStatus::Ok)
}

/// Documented criterion: Discord API answers correctly AND the gateway host
/// completes TLS. It does not prove the client or voice work.
pub async fn quick_probe(client: &reqwest::Client) -> Probe {
    let (api, gw) = tokio::join!(
        api_check(client),
        https_check(client, "gateway.discord.gg", "https://gateway.discord.gg/")
    );
    let items = vec![api, gw];
    Probe {
        ok: probe_verdict(2, &items),
        items,
    }
}

fn covers(spec: Option<&str>, lo: u16, hi: u16) -> bool {
    let Some(spec) = spec.and_then(|s| PortSpec::parse(s).ok()) else {
        return false;
    };
    spec.0.iter().any(|i| match i {
        PortItem::Range(a, b) => *a <= lo && hi <= *b,
        PortItem::Single(p) => lo == hi && *p == lo,
        PortItem::Var(_) => false,
    })
}

/// Static analysis of the strategy's UDP handling for Discord voice.
pub fn voice_rules(s: &Strategy) -> CheckItem {
    let wf_udp = s
        .global
        .iter()
        .find(|a| a.opt == "--wf-udp")
        .and_then(|a| a.text_value());
    let rule = s.rules.iter().filter(|r| r.enabled).find(|r| {
        r.args.iter().any(|a| {
            a.opt == "--filter-l7"
                && a.text_value()
                    .is_some_and(|v| v.contains("discord") || v.contains("stun"))
        })
    });
    match rule {
        None => {
            let mut i = item(
                "udp:voice-rules",
                "udp",
                "UDP-правила голоса",
                CheckStatus::Warning,
                "в стратегии нет правила для голосового трафика Discord (filter-l7=discord/stun)",
            );
            i.hint = Some("Голосовые каналы могут не работать с этой стратегией.".into());
            i
        }
        Some(r) => {
            let ports = r
                .args
                .iter()
                .find(|a| a.opt == "--filter-udp")
                .and_then(|a| a.text_value())
                .unwrap_or("—");
            let captured = covers(wf_udp, 50000, 50100) && covers(wf_udp, 19294, 19344);
            if captured {
                item("udp:voice-rules", "udp", "UDP-правила голоса", CheckStatus::Ok, format!("есть правило для голоса (порты {ports}), порты перехватываются --wf-udp. Это проверка конфигурации, а не соединения."))
            } else {
                item("udp:voice-rules", "udp", "UDP-правила голоса", CheckStatus::Warning, format!("правило есть (порты {ports}), но --wf-udp не перехватывает все голосовые порты"))
            }
        }
    }
}

pub fn engine_items(st: Option<&EngineStatus>, err: Option<&str>) -> Vec<CheckItem> {
    match (st, err) {
        (_, Some(e)) => vec![{
            let mut i = item(
                "helper",
                "zapret",
                "Служба Discord Bridge Helper",
                CheckStatus::Error,
                e,
            );
            i.hint = Some("Переустановите Bridge или запустите службу «Discord Bridge Helper» в services.msc.".into());
            i
        }],
        (Some(st), None) => {
            let mut v = vec![item(
                "helper",
                "zapret",
                "Служба Discord Bridge Helper",
                CheckStatus::Ok,
                "служба отвечает",
            )];
            v.push(match st.state {
                EngineState::Running => item(
                    "engine",
                    "zapret",
                    "Zapret (winws)",
                    CheckStatus::Ok,
                    format!(
                        "запущен, PID {}, стратегия «{}»",
                        st.pid.unwrap_or(0),
                        st.strategy_name.clone().unwrap_or_default()
                    ),
                ),
                EngineState::Stopped => item(
                    "engine",
                    "zapret",
                    "Zapret (winws)",
                    CheckStatus::Skipped,
                    "остановлен",
                ),
                EngineState::Failed => item(
                    "engine",
                    "zapret",
                    "Zapret (winws)",
                    CheckStatus::Error,
                    st.last_error
                        .clone()
                        .unwrap_or_else(|| "аварийно завершён".into()),
                ),
            });
            v
        }
        (None, None) => vec![],
    }
}

pub fn helper_items(d: &HelperDiagnostics, needs_ts: bool) -> Vec<CheckItem> {
    let mut v = Vec::new();
    v.push(if d.integrity_ok {
        item(
            "flowseal",
            "zapret",
            "Компоненты Flowseal",
            CheckStatus::Ok,
            format!(
                "версия {}, целостность подтверждена (SHA-256)",
                d.bundle_version.clone().unwrap_or_default()
            ),
        )
    } else {
        let mut i = item(
            "flowseal",
            "zapret",
            "Компоненты Flowseal",
            CheckStatus::Error,
            d.integrity_problems.join("; "),
        );
        i.hint = Some("Файлы повреждены или изменены. Переустановите Bridge.".into());
        i
    });
    v.push(match d.tcp_timestamps {
        Some(true) => item("tcp-ts", "system", "TCP timestamps", CheckStatus::Ok, "включены"),
        Some(false) if needs_ts => {
            let mut i = item("tcp-ts", "system", "TCP timestamps", CheckStatus::Warning, "выключены, а активная стратегия использует fooling=ts");
            i.hint = Some("Flowseal включает их автоматически; Bridge — только с вашего согласия (кнопка ниже).".into());
            i
        }
        Some(false) => item("tcp-ts", "system", "TCP timestamps", CheckStatus::Ok, "выключены (активной стратегии не требуются)"),
        None => item("tcp-ts", "system", "TCP timestamps", CheckStatus::Unknown, "не удалось определить"),
    });
    v.push(match d.base_filtering_engine_running {
        Some(true) => item(
            "bfe",
            "system",
            "Base Filtering Engine",
            CheckStatus::Ok,
            "служба запущена",
        ),
        Some(false) => item(
            "bfe",
            "system",
            "Base Filtering Engine",
            CheckStatus::Error,
            "служба BFE не запущена — WinDivert работать не будет",
        ),
        None => item(
            "bfe",
            "system",
            "Base Filtering Engine",
            CheckStatus::Unknown,
            "не удалось определить",
        ),
    });
    v.push(item(
        "windivert",
        "system",
        "Драйвер WinDivert",
        CheckStatus::Ok,
        match &d.windivert_service {
            Some(s) => format!("служба WinDivert: {s}"),
            None => "не загружен (загружается при запуске winws)".into(),
        },
    ));
    if d.conflicts.is_empty() {
        v.push(item(
            "conflicts",
            "system",
            "Конфликты",
            CheckStatus::Ok,
            "других движков обхода DPI не обнаружено",
        ));
    } else {
        let mut i = item(
            "conflicts",
            "system",
            "Конфликты",
            CheckStatus::Warning,
            d.conflicts
                .iter()
                .map(|c| {
                    format!(
                        "{} {}{}",
                        c.kind,
                        c.name,
                        c.pid.map(|p| format!(" (PID {p})")).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
        );
        i.hint = Some("Bridge не останавливает чужие процессы и службы. Остановите их сами (например, service.bat → Remove Services у Flowseal) или запустите обход с явным подтверждением.".into());
        v.push(i);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::Strategy;

    #[test]
    fn empty_or_partial_results_are_not_success() {
        let ok = item("x", "https", "x", CheckStatus::Ok, "");
        let bad = item("y", "https", "y", CheckStatus::Error, "");
        assert!(!probe_verdict(2, &[]));
        assert!(!probe_verdict(0, &[]));
        assert!(
            !probe_verdict(2, std::slice::from_ref(&ok)),
            "one result missing"
        );
        assert!(!probe_verdict(2, &[ok.clone(), bad]));
        assert!(probe_verdict(2, &[ok.clone(), ok]));
    }

    #[test]
    fn voice_rule_detection() {
        let s = Strategy::new_empty("a", "A");
        assert_eq!(voice_rules(&s).status, CheckStatus::Warning);
        let mut s2 = s.clone();
        s2.global = vec![bridge_core::Arg::text(
            "--wf-udp",
            "443,19294-19344,50000-50100",
        )];
        s2.rules.push(bridge_core::Rule {
            label: None,
            enabled: true,
            args: vec![
                bridge_core::Arg::text("--filter-udp", "19294-19344,50000-50100"),
                bridge_core::Arg::text("--filter-l7", "discord,stun"),
            ],
        });
        let r = voice_rules(&s2);
        assert_eq!(r.status, CheckStatus::Ok);
        assert!(r.detail.contains("не соединения"));
    }
}
