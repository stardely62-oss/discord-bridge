//! Table of winws options understood by Bridge.
//!
//! The table is the single source of truth for validation (Rust) and for the
//! visual editor (exported to the UI through `option_specs()`). Anything not
//! listed here is rejected: imported strategies are untrusted data.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Applies to the whole engine; rendered before the first rule.
    Global,
    /// Applies to one rule (winws profile).
    Rule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Capture,
    Filter,
    Lists,
    Desync,
    Fake,
    Split,
    Misc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Kind {
    /// Comma-separated ports/ranges, may contain game filter variables.
    Ports,
    /// No value.
    Flag,
    Int {
        min: i64,
        max: i64,
    },
    /// One value, or a comma-separated list when `multi`.
    Enum {
        values: &'static [&'static str],
        multi: bool,
    },
    /// Fake payload: a resource from the `bin` directory, a hex literal
    /// `0x...`, or (`allow_bang`) the winws built-in default `!`.
    Fake {
        allow_bang: bool,
    },
    /// Domain/IP list resource from the `lists` directory.
    List,
    /// Comma-separated domain names.
    Domains,
    /// Comma-separated IPv4/IPv6 addresses or CIDR blocks.
    Cidrs,
    /// Comma-separated modifiers: `key` or `key=value`.
    Mods {
        keys: &'static [&'static str],
    },
    /// Comma-separated split markers: `1`, `-2`, `midsld`, `sniext+1`, `host+2`.
    Markers,
    /// Cutoff/start: `n3`, `d5`, `s100`.
    Cutoff,
    /// AutoTTL syntax `delta[:min-max]`.
    AutoTtl,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptionSpec {
    pub name: &'static str,
    pub scope: Scope,
    pub category: Category,
    pub kind: Kind,
    /// May appear more than once in the same rule.
    pub repeatable: bool,
    pub title_ru: &'static str,
    pub help_ru: &'static str,
    pub title_en: &'static str,
    pub help_en: &'static str,
}

const DESYNC_MODES: &[&str] = &[
    "fake",
    "fakeknown",
    "rst",
    "rstack",
    "synack",
    "syndata",
    "split",
    "split2",
    "disorder",
    "disorder2",
    "multisplit",
    "multidisorder",
    "fakedsplit",
    "fakeddisorder",
    "hostfakesplit",
    "ipfrag1",
    "ipfrag2",
    "udplen",
    "tamper",
    "hopbyhop",
    "destopt",
];
const FOOLING: &[&str] = &[
    "none",
    "md5sig",
    "badseq",
    "badsum",
    "datanoack",
    "ts",
    "hopbyhop",
    "hopbyhop2",
];
const L7: &[&str] = &[
    "http",
    "tls",
    "quic",
    "wireguard",
    "dht",
    "discord",
    "stun",
    "unknown",
];
const L3: &[&str] = &["ipv4", "ipv6"];
const IPID: &[&str] = &["seq", "seqgroup", "rnd", "zero"];

macro_rules! opt {
    ($name:literal, $scope:ident, $cat:ident, $kind:expr, $rep:literal, $tru:literal, $hru:literal, $ten:literal, $hen:literal) => {
        OptionSpec {
            name: $name,
            scope: Scope::$scope,
            category: Category::$cat,
            kind: $kind,
            repeatable: $rep,
            title_ru: $tru,
            help_ru: $hru,
            title_en: $ten,
            help_en: $hen,
        }
    };
}

fn fake(bang: bool) -> Kind {
    Kind::Fake { allow_bang: bang }
}

pub fn option_specs() -> Vec<OptionSpec> {
    vec![
        // ---- Capture (global) ----
        opt!("--wf-tcp", Global, Capture, Kind::Ports, false,
            "Перехват TCP-портов", "Какие исходящие TCP-порты WinDivert передаёт движку. Пакеты на другие порты не затрагиваются.",
            "Captured TCP ports", "Outgoing TCP ports WinDivert hands to the engine. Other ports are untouched."),
        opt!("--wf-udp", Global, Capture, Kind::Ports, false,
            "Перехват UDP-портов", "Какие исходящие UDP-порты перехватываются. Для голоса Discord Flowseal использует 19294-19344 и 50000-50100.",
            "Captured UDP ports", "Outgoing UDP ports to capture. Flowseal uses 19294-19344 and 50000-50100 for Discord voice."),
        opt!("--wf-l3", Global, Capture, Kind::Enum { values: L3, multi: true }, false,
            "Версии IP", "Ограничить перехват IPv4 и/или IPv6.",
            "IP versions", "Restrict capture to IPv4 and/or IPv6."),
        // ---- Filters ----
        opt!("--filter-tcp", Rule, Filter, Kind::Ports, false,
            "TCP-порты правила", "Правило применяется только к TCP-соединениям на эти порты.",
            "Rule TCP ports", "The rule only applies to TCP connections to these ports."),
        opt!("--filter-udp", Rule, Filter, Kind::Ports, false,
            "UDP-порты правила", "Правило применяется только к UDP-пакетам на эти порты.",
            "Rule UDP ports", "The rule only applies to UDP packets to these ports."),
        opt!("--filter-l3", Rule, Filter, Kind::Enum { values: L3, multi: true }, false,
            "IP-версия правила", "Применять правило только к IPv4 или IPv6.",
            "Rule IP version", "Apply the rule only to IPv4 or IPv6."),
        opt!("--filter-l7", Rule, Filter, Kind::Enum { values: L7, multi: true }, false,
            "Протоколы L7", "Распознаваемые протоколы прикладного уровня. discord и stun — голосовой трафик Discord.",
            "L7 protocols", "Application protocols to match. discord and stun are Discord voice traffic."),
        // ---- Lists ----
        opt!("--hostlist", Rule, Lists, Kind::List, true,
            "Список доменов", "Правило срабатывает только для доменов из списка.",
            "Domain list", "The rule only matches domains from this list."),
        opt!("--hostlist-exclude", Rule, Lists, Kind::List, true,
            "Исключённые домены", "Домены из списка не обрабатываются этим правилом.",
            "Excluded domains", "Domains from this list are skipped by this rule."),
        opt!("--hostlist-domains", Rule, Lists, Kind::Domains, true,
            "Домены (вручную)", "Список доменов через запятую вместо файла.",
            "Domains (inline)", "Comma-separated domains instead of a file."),
        opt!("--hostlist-exclude-domains", Rule, Lists, Kind::Domains, true,
            "Исключённые домены (вручную)", "Домены через запятую, которые правило не трогает.",
            "Excluded domains (inline)", "Comma-separated domains this rule skips."),
        opt!("--ipset", Rule, Lists, Kind::List, true,
            "Набор IP", "Правило срабатывает только для адресов из списка.",
            "IP set", "The rule only matches addresses from this list."),
        opt!("--ipset-exclude", Rule, Lists, Kind::List, true,
            "Исключённые IP", "Адреса из списка не обрабатываются.",
            "Excluded IPs", "Addresses from this list are skipped."),
        opt!("--ipset-ip", Rule, Lists, Kind::Cidrs, true,
            "IP (вручную)", "Адреса или подсети через запятую.",
            "IPs (inline)", "Comma-separated addresses or subnets."),
        opt!("--ipset-exclude-ip", Rule, Lists, Kind::Cidrs, true,
            "Исключённые IP (вручную)", "Адреса или подсети через запятую, которые правило не трогает.",
            "Excluded IPs (inline)", "Comma-separated addresses or subnets to skip."),
        // ---- Desync ----
        opt!("--dpi-desync", Rule, Desync, Kind::Enum { values: DESYNC_MODES, multi: true }, false,
            "Метод десинхронизации", "Основной приём обхода DPI. Можно комбинировать до трёх фаз через запятую, например fake,multisplit.",
            "Desync method", "Main DPI evasion technique. Up to three phases can be combined, e.g. fake,multisplit."),
        opt!("--dpi-desync-repeats", Rule, Desync, Kind::Int { min: 1, max: 1000 }, false,
            "Повторы фейков", "Сколько раз отправлять каждый фейковый пакет.",
            "Fake repeats", "How many times each fake packet is sent."),
        opt!("--dpi-desync-fooling", Rule, Desync, Kind::Enum { values: FOOLING, multi: true }, false,
            "Способ «обмана»", "Как сделать фейк невалидным для сервера. ts требует включённых TCP timestamps в Windows.",
            "Fooling", "How fakes are made invalid for the server. ts requires TCP timestamps enabled in Windows."),
        opt!("--dpi-desync-ttl", Rule, Desync, Kind::Int { min: 0, max: 255 }, false,
            "TTL фейков (IPv4)", "TTL фейковых пакетов, чтобы они не доходили до сервера.",
            "Fake TTL (IPv4)", "TTL of fake packets so they do not reach the server."),
        opt!("--dpi-desync-ttl6", Rule, Desync, Kind::Int { min: 0, max: 255 }, false,
            "Hop limit фейков (IPv6)", "То же для IPv6.", "Fake hop limit (IPv6)", "Same for IPv6."),
        opt!("--dpi-desync-autottl", Rule, Desync, Kind::AutoTtl, false,
            "Авто-TTL", "Автоматический TTL: дельта[:мин-макс], например 2 или -1:3-20.",
            "Auto TTL", "Automatic TTL: delta[:min-max], e.g. 2 or -1:3-20."),
        opt!("--dpi-desync-badseq-increment", Rule, Desync, Kind::Int { min: -2147483648, max: 4294967295 }, false,
            "Смещение seq для badseq", "Насколько сдвигать номер последовательности при fooling=badseq.",
            "badseq increment", "Sequence offset used by fooling=badseq."),
        opt!("--dpi-desync-badack-increment", Rule, Desync, Kind::Int { min: -2147483648, max: 4294967295 }, false,
            "Смещение ack для badseq", "Сдвиг номера подтверждения.", "badack increment", "Acknowledgement offset."),
        opt!("--dpi-desync-ts-increment", Rule, Desync, Kind::Int { min: -2147483648, max: 4294967295 }, false,
            "Смещение timestamp", "Сдвиг TCP timestamp при fooling=ts.", "ts increment", "TCP timestamp offset for fooling=ts."),
        opt!("--dpi-desync-any-protocol", Rule, Desync, Kind::Int { min: 0, max: 1 }, false,
            "Любой протокол", "1 — применять десинхронизацию даже к нераспознанному трафику.",
            "Any protocol", "1 — desync even unrecognised payloads."),
        opt!("--dpi-desync-cutoff", Rule, Desync, Kind::Cutoff, false,
            "Отсечка", "После какого пакета прекратить обработку соединения: n3 — 3-й пакет, d5 — 5-й пакет с данными, s100 — 100 байт.",
            "Cutoff", "Stop processing a connection after n3 (packets), d5 (data packets) or s100 (bytes)."),
        opt!("--dpi-desync-start", Rule, Desync, Kind::Cutoff, false,
            "Начало обработки", "С какого пакета начинать десинхронизацию (синтаксис как у отсечки).",
            "Start", "Packet from which desync starts (same syntax as cutoff)."),
        opt!("--dpi-desync-skip-nosni", Rule, Desync, Kind::Int { min: 0, max: 1 }, false,
            "Пропускать TLS без SNI", "1 — не трогать TLS ClientHello без SNI.",
            "Skip TLS without SNI", "1 — leave ClientHello without SNI alone."),
        opt!("--ip-id", Rule, Desync, Kind::Enum { values: IPID, multi: false }, false,
            "IP ID", "Как заполнять поле IP ID исходящих пакетов.", "IP ID", "How to fill the IP ID field."),
        // ---- Split ----
        opt!("--dpi-desync-split-pos", Rule, Split, Kind::Markers, false,
            "Позиции разреза", "Где резать пакет: число, midsld, sniext+1, host+2 и т.п. через запятую.",
            "Split positions", "Where to split: number, midsld, sniext+1, host+2, comma-separated."),
        opt!("--dpi-desync-split-seqovl", Rule, Split, Kind::Markers, false,
            "Перекрытие seq", "Размер перекрытия последовательности для split/multisplit.",
            "seq overlap", "Sequence overlap size for split/multisplit."),
        opt!("--dpi-desync-split-seqovl-pattern", Rule, Split, fake(false), false,
            "Шаблон перекрытия", "Чем заполнять перекрывающую часть: файл-фейк или hex 0x...",
            "Overlap pattern", "Filler of the overlapping part: fake file or hex 0x..."),
        opt!("--dpi-desync-fakedsplit-pattern", Rule, Split, fake(false), false,
            "Шаблон fakedsplit", "Заполнитель фейковых частей для fakedsplit.",
            "fakedsplit pattern", "Filler of fake parts for fakedsplit."),
        opt!("--dpi-desync-fakedsplit-mod", Rule, Split, Kind::Mods { keys: &["altorder"] }, false,
            "Модификаторы fakedsplit", "Например altorder=1.", "fakedsplit modifiers", "For example altorder=1."),
        opt!("--dpi-desync-hostfakesplit-midhost", Rule, Split, Kind::Markers, false,
            "Разрез внутри host", "Дополнительный разрез внутри имени хоста для hostfakesplit.",
            "Mid-host split", "Extra split inside the host name for hostfakesplit."),
        opt!("--dpi-desync-hostfakesplit-mod", Rule, Split, Kind::Mods { keys: &["host", "altorder"] }, false,
            "Модификаторы hostfakesplit", "host=домен для фейка, altorder=1 — другой порядок частей.",
            "hostfakesplit modifiers", "host=domain for the fake, altorder=1 for alternative order."),
        opt!("--dpi-desync-ipfrag-pos-tcp", Rule, Split, Kind::Int { min: 8, max: 65535 }, false,
            "Позиция IP-фрагмента (TCP)", "Смещение фрагментации для ipfrag2.", "IP frag offset (TCP)", "Fragment offset for ipfrag2."),
        opt!("--dpi-desync-ipfrag-pos-udp", Rule, Split, Kind::Int { min: 8, max: 65535 }, false,
            "Позиция IP-фрагмента (UDP)", "Смещение фрагментации для ipfrag2.", "IP frag offset (UDP)", "Fragment offset for ipfrag2."),
        opt!("--dpi-desync-udplen-increment", Rule, Split, Kind::Int { min: -65535, max: 65535 }, false,
            "Изменение длины UDP", "На сколько байт менять длину UDP для udplen.", "UDP length delta", "UDP length change for udplen."),
        opt!("--dpi-desync-udplen-pattern", Rule, Split, fake(false), false,
            "Шаблон udplen", "Заполнитель добавленных байт.", "udplen pattern", "Filler for added bytes."),
        // ---- Fakes ----
        opt!("--dpi-desync-fake-http", Rule, Fake, fake(false), true,
            "Фейк HTTP", "Фейковый HTTP-запрос.", "HTTP fake", "Fake HTTP request."),
        opt!("--dpi-desync-fake-tls", Rule, Fake, fake(true), true,
            "Фейк TLS", "Фейковый TLS ClientHello. «!» — встроенный фейк winws.", "TLS fake", "Fake TLS ClientHello. \"!\" — winws built-in."),
        opt!("--dpi-desync-fake-tls-mod", Rule, Fake,
            Kind::Mods { keys: &["none", "rnd", "rndsni", "dupsid", "padencap", "sni"] }, true,
            "Модификаторы TLS-фейка", "rnd, rndsni, dupsid, padencap, sni=домен или none.",
            "TLS fake modifiers", "rnd, rndsni, dupsid, padencap, sni=domain or none."),
        opt!("--dpi-desync-fake-quic", Rule, Fake, fake(false), true,
            "Фейк QUIC", "Фейковый QUIC Initial.", "QUIC fake", "Fake QUIC Initial."),
        opt!("--dpi-desync-fake-discord", Rule, Fake, fake(false), true,
            "Фейк Discord (голос)", "Фейк для протокола голосовых серверов Discord.", "Discord voice fake", "Fake for the Discord voice protocol."),
        opt!("--dpi-desync-fake-stun", Rule, Fake, fake(false), true,
            "Фейк STUN", "Фейк для STUN (используется голосом).", "STUN fake", "Fake for STUN (used by voice)."),
        opt!("--dpi-desync-fake-wireguard", Rule, Fake, fake(false), true,
            "Фейк WireGuard", "Фейк для WireGuard.", "WireGuard fake", "Fake for WireGuard."),
        opt!("--dpi-desync-fake-dht", Rule, Fake, fake(false), true,
            "Фейк DHT", "Фейк для DHT.", "DHT fake", "Fake for DHT."),
        opt!("--dpi-desync-fake-unknown", Rule, Fake, fake(false), true,
            "Фейк для неизвестного TCP", "Фейк для нераспознанного TCP-трафика.", "Unknown TCP fake", "Fake for unrecognised TCP."),
        opt!("--dpi-desync-fake-unknown-udp", Rule, Fake, fake(false), true,
            "Фейк для неизвестного UDP", "Фейк для нераспознанного UDP-трафика.", "Unknown UDP fake", "Fake for unrecognised UDP."),
        opt!("--dpi-desync-fake-syndata", Rule, Fake, fake(false), true,
            "Фейк syndata", "Данные в SYN для режима syndata.", "syndata fake", "SYN payload for syndata."),
        // ---- Misc ----
        opt!("--dup", Rule, Misc, Kind::Int { min: 0, max: 1000 }, false,
            "Дубликаты", "Сколько дубликатов оригинального пакета отправлять.", "Duplicates", "How many duplicates of the original packet to send."),
        opt!("--dup-cutoff", Rule, Misc, Kind::Cutoff, false,
            "Отсечка дубликатов", "Когда прекратить отправку дубликатов.", "Duplicate cutoff", "When to stop sending duplicates."),
        opt!("--dup-fooling", Rule, Misc, Kind::Enum { values: FOOLING, multi: true }, false,
            "Fooling дубликатов", "Как испортить дубликаты.", "Duplicate fooling", "How duplicates are spoiled."),
    ]
}

pub fn find(name: &str) -> Option<OptionSpec> {
    option_specs().into_iter().find(|o| o.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn names_unique_and_well_formed() {
        let mut seen = HashSet::new();
        for o in option_specs() {
            assert!(o.name.starts_with("--"), "{}", o.name);
            assert!(seen.insert(o.name), "duplicate {}", o.name);
            assert!(!o.title_ru.is_empty() && !o.help_ru.is_empty());
        }
    }
}
