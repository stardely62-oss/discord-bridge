//! Built-in Flowseal strategy catalog.
//!
//! Strategies are produced at runtime by running the *safe* BAT importer over
//! the original files of the bundled Flowseal release. Real names come from the
//! actual file set, nothing is hard-coded.

use crate::batparse::{parse_bat, BatError, BatOptions};
use crate::lists::RUNTIME_LISTS;
use crate::strategy::validate::ResourceIndex;
use crate::strategy::{slugify, Source, Strategy};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogError {
    pub file: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub flowseal_version: String,
    pub strategies: Vec<Strategy>,
    pub errors: Vec<CatalogError>,
}

/// Natural sort key used by Flowseal's own menu (numbers padded to 8 digits).
pub fn natural_key(s: &str) -> String {
    let mut out = String::new();
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push_str(&format!("{num:0>8}"));
                num.clear();
            }
            out.push(c);
        }
    }
    if !num.is_empty() {
        out.push_str(&format!("{num:0>8}"));
    }
    out.to_lowercase()
}

/// Is this file a Flowseal strategy (not `service.bat`)?
pub fn is_strategy_file(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".bat") && !l.starts_with("service")
}

pub fn builtin_id(file_stem: &str) -> String {
    format!("flowseal-{}", slugify(file_stem))
}

fn describe(stem: &str) -> String {
    let u = stem.to_ascii_uppercase();
    let base = if u == "GENERAL" {
        "Базовая стратегия Flowseal: multisplit с перекрытием seq для TCP и фейки для QUIC и голоса Discord."
    } else if u.contains("FAKE TLS AUTO") {
        "Семейство FAKE TLS AUTO: фейковые TLS ClientHello с автоматическими модификациями (rnd, dupsid, sni)."
    } else if u.contains("SIMPLE FAKE") {
        "Семейство SIMPLE FAKE: простые фейковые пакеты без сложной нарезки."
    } else if u.contains("EXP") {
        "Экспериментальная стратегия Flowseal."
    } else if u.contains("ALT") {
        "Альтернативный вариант Flowseal с другими методами десинхронизации и фейками."
    } else {
        "Стратегия из набора Flowseal."
    };
    format!("{base} Исходный файл: {stem}.bat.")
}

/// Build the catalog from `(file name, contents)` pairs.
pub fn build_catalog(version: &str, files: &[(String, String)]) -> Catalog {
    let mut files: Vec<&(String, String)> =
        files.iter().filter(|(n, _)| is_strategy_file(n)).collect();
    files.sort_by_key(|(n, _)| natural_key(n));
    let mut cat = Catalog {
        flowseal_version: version.into(),
        ..Default::default()
    };
    for (name, body) in files {
        let stem = name.trim_end_matches(".bat").trim_end_matches(".BAT");
        let opts = BatOptions {
            name: stem.to_string(),
            id: Some(builtin_id(stem)),
            source: Some(Source::Flowseal),
            flowseal_version: Some(version.into()),
            file: Some(name.clone()),
        };
        match parse_bat(body, &opts) {
            Ok(imp) if imp.unsupported.is_empty() => {
                let mut s = imp.strategy;
                s.description = describe(stem);
                s.metadata.remove("importedFrom");
                cat.strategies.push(s);
            }
            Ok(imp) => cat.errors.push(CatalogError {
                file: name.clone(),
                error: format!(
                    "неподдерживаемые строки: {}",
                    imp.unsupported
                        .iter()
                        .map(|u| u.line.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }),
            Err(e) => cat.errors.push(CatalogError {
                file: name.clone(),
                error: e.to_string(),
            }),
        }
    }
    cat
}

/// Resource index from file names in the Flowseal `bin` and `lists` dirs plus
/// the lists the helper generates at runtime.
pub fn resource_index<'a>(
    bin: impl IntoIterator<Item = &'a str>,
    lists: impl IntoIterator<Item = &'a str>,
) -> ResourceIndex {
    let mut idx = ResourceIndex::default();
    for f in bin {
        if f.to_ascii_lowercase().ends_with(".bin") && crate::is_safe_resource_name(f) {
            idx.fakes.insert(f.to_string());
        }
    }
    for f in lists {
        if f.to_ascii_lowercase().ends_with(".txt") && crate::is_safe_resource_name(f) {
            idx.lists.insert(f.to_string());
        }
    }
    for f in RUNTIME_LISTS {
        idx.lists.insert(f.to_string());
    }
    idx
}

pub fn parse_error_to_string(e: &BatError) -> String {
    e.to_string()
}
