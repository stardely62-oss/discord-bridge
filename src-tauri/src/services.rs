//! Service cards (read-only): catalog facts, the original Flowseal lists of
//! the active version and the running configuration, side by side.
//!
//! The list files are read from the installed resources (the built-in
//! version) or, for versions installed as updates (a directory only SYSTEM
//! and administrators can read), from the helper — in both cases checked
//! against the bundle manifest's SHA-256. Nothing is downloaded here.

use crate::state::AppState;
use base64::Engine as _;
use bridge_core::ipc::{EngineState, Request, Response};
use bridge_core::manifest::{sha256_hex, BundleManifest};
use bridge_core::packs::PackState;
use bridge_core::services::{
    self as core, CardDetails, CardInput, CardKind, CardSummary, Context, ListPage, Running,
    SourceFile,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use tauri::State;

type R<T> = Result<T, String>;

/// The list files of one Flowseal version, verified.
#[derive(Default)]
pub struct ListCache {
    pub version: String,
    pub lists: Arc<BTreeMap<String, String>>,
    /// Files that exist in the manifest but could not be read or verified.
    pub errors: Vec<String>,
}

/// Names read for the cards (the strategy files reference these).
const NAMES: &[&str] = &[
    "list-general.txt",
    "list-google.txt",
    "list-exclude.txt",
    "ipset-all.txt",
    "ipset-exclude.txt",
    "ipset-all.txt.backup",
];

fn from_resources(
    state: &AppState,
    version: &str,
) -> Option<(BTreeMap<String, String>, Vec<String>)> {
    let dir = state.resource_dir.join("flowseal").join(version);
    let text = std::fs::read_to_string(dir.join("bundle-manifest.json")).ok()?;
    let m: BundleManifest = serde_json::from_str(&text).ok()?;
    if m.flowseal_version != version {
        return None;
    }
    let mut lists = BTreeMap::new();
    let mut errors = Vec::new();
    for f in m.files.iter().filter(|f| f.path.starts_with("lists/")) {
        let name = &f.path["lists/".len()..];
        if name.contains('/') || f.size as usize > core::MAX_LIST_BYTES {
            continue;
        }
        let ok = std::fs::read(dir.join("lists").join(name))
            .map_err(|e| e.to_string())
            .and_then(|d| {
                m.verify_file(&f.path, &d)
                    .map(|_| d)
                    .map_err(|e| e.to_string())
            });
        match ok {
            Ok(d) => {
                lists.insert(name.to_string(), String::from_utf8_lossy(&d).into_owned());
            }
            Err(e) => errors.push(format!("{}: {e}", f.path)),
        }
    }
    Some((lists, errors))
}

async fn from_helper(state: &AppState, version: &str) -> (BTreeMap<String, String>, Vec<String>) {
    let mut lists = BTreeMap::new();
    let mut errors = Vec::new();
    'files: for name in NAMES {
        let mut data = Vec::new();
        let want_sha = loop {
            let req = Request::ReadBundleList {
                version: version.into(),
                name: name.to_string(),
                offset: data.len() as u64,
            };
            match state.helper.request(req).await {
                Ok(Response::BundleList {
                    sha256,
                    size,
                    data: chunk,
                    ..
                }) => {
                    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(chunk) else {
                        errors.push(format!("{name}: повреждённый ответ службы"));
                        continue 'files;
                    };
                    let done = bytes.is_empty();
                    data.extend_from_slice(&bytes);
                    if data.len() as u64 >= size || done || data.len() > core::MAX_LIST_BYTES {
                        break sha256;
                    }
                }
                Ok(Response::Error { message, .. }) => {
                    // A file this version does not ship is simply absent.
                    if !message.contains("нет файла") {
                        errors.push(format!("{name}: {message}"));
                    }
                    continue 'files;
                }
                Ok(_) => {
                    errors.push(format!(
                        "{name}: служба не поддерживает чтение списков (обновите Bridge)"
                    ));
                    break 'files;
                }
                Err(e) => {
                    errors.push(format!("{name}: {e}"));
                    break 'files;
                }
            }
        };
        if sha256_hex(&data) != want_sha {
            errors.push(format!("{name}: SHA-256 не совпадает с манифестом"));
            continue;
        }
        lists.insert(
            name.to_string(),
            String::from_utf8_lossy(&data).into_owned(),
        );
    }
    (lists, errors)
}

/// The verified lists of the active Flowseal version (cached per version).
pub async fn active_lists(
    state: &AppState,
) -> (Option<String>, Arc<BTreeMap<String, String>>, Vec<String>) {
    let Some(version) = state.bundle.lock().expect("lock").version.clone() else {
        return (
            None,
            Arc::default(),
            vec!["активная версия Flowseal неизвестна".into()],
        );
    };
    if let Some(c) = state.list_cache.lock().expect("lock").as_ref() {
        if c.version == version {
            return (Some(version), c.lists.clone(), c.errors.clone());
        }
    }
    let (lists, errors) = match from_resources(state, &version) {
        Some(x) => x,
        None => from_helper(state, &version).await,
    };
    let lists = Arc::new(lists);
    *state.list_cache.lock().expect("lock") = Some(ListCache {
        version: version.clone(),
        lists: lists.clone(),
        errors: errors.clone(),
    });
    (Some(version), lists, errors)
}

struct Owned {
    id: String,
    kind: CardKind,
    flowseal_list: Option<String>,
    domains: Vec<String>,
    ips: Vec<String>,
}

fn owned_cards(state: &AppState) -> Vec<Owned> {
    let installed = state.settings().installed_packs;
    let view = state.catalog.view(&installed);
    let mut out: Vec<Owned> = view
        .packs
        .iter()
        .map(|v| match (&v.installed, v.state) {
            (_, PackState::Flowseal) => Owned {
                id: v.pack.id.clone(),
                kind: CardKind::Flowseal,
                flowseal_list: v.pack.flowseal_list.clone(),
                domains: v.pack.domains.clone(),
                ips: v.pack.ips.clone(),
            },
            (Some(i), _) => Owned {
                id: i.id.clone(),
                kind: CardKind::Installed,
                flowseal_list: None,
                domains: i.domains.clone(),
                ips: i.ips.clone(),
            },
            (None, _) => Owned {
                id: v.pack.id.clone(),
                kind: CardKind::Available,
                flowseal_list: None,
                domains: v.pack.domains.clone(),
                ips: v.pack.ips.clone(),
            },
        })
        .collect();
    out.extend(view.orphans.iter().map(|o| Owned {
        id: o.id.clone(),
        kind: CardKind::Installed,
        flowseal_list: None,
        domains: o.domains.clone(),
        ips: o.ips.clone(),
    }));
    out
}

fn inputs(o: &[Owned]) -> Vec<CardInput<'_>> {
    o.iter()
        .map(|c| CardInput {
            id: &c.id,
            kind: c.kind,
            flowseal_list: c.flowseal_list.as_deref(),
            domains: &c.domains,
            ips: &c.ips,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicesOverview {
    pub bundle_version: Option<String>,
    pub sources: Vec<SourceFile>,
    pub errors: Vec<String>,
    pub cards: Vec<CardSummary>,
}

/// Runs `f` with the context of the current settings, strategy, engine.
async fn with_context<T>(
    state: &AppState,
    f: impl FnOnce(&[CardInput], &Context, &[String]) -> T,
) -> T {
    let (version, lists, errors) = active_lists(state).await;
    let s = state.settings();
    let engine = crate::zapret::engine_status(state).await.ok();
    let pending = crate::commands::pending_apply(state, engine.as_ref());
    let running = engine
        .filter(|e| e.state == EngineState::Running)
        .map(|e| Running {
            strategy_id: e.strategy_id,
            bundle_version: e.bundle_version,
            pending_apply: pending,
        });
    let selected = s
        .active_strategy_id
        .as_deref()
        .and_then(|id| state.strategies.lock().expect("lock").get(id).cloned());
    let effective = bridge_core::lists::effective_lists(&s.user_lists, &s.installed_packs);
    let owned = owned_cards(state);
    let cards = inputs(&owned);
    let ctx = Context {
        bundle_version: version.as_deref(),
        lists: &lists,
        selected: selected.as_ref(),
        game_filter: s.game_filter.ports(),
        ipset_mode: s.ipset_mode,
        effective: &effective,
        running,
    };
    f(&cards, &ctx, &errors)
}

#[tauri::command]
pub async fn services_overview(state: State<'_, AppState>) -> R<ServicesOverview> {
    Ok(with_context(&state, |cards, ctx, errors| ServicesOverview {
        bundle_version: ctx.bundle_version.map(str::to_string),
        sources: core::source_files(ctx.lists),
        errors: errors.to_vec(),
        cards: cards
            .iter()
            .map(|c| core::details(c, cards, ctx).summary)
            .collect(),
    })
    .await)
}

#[tauri::command]
pub async fn service_details(state: State<'_, AppState>, id: String) -> R<CardDetails> {
    with_context(&state, |cards, ctx, _| {
        cards
            .iter()
            .find(|c| c.id == id)
            .map(|c| core::details(c, cards, ctx))
            .ok_or_else(|| "такой карточки нет в каталоге".to_string())
    })
    .await
}

fn list_text(lists: &BTreeMap<String, String>, name: &str) -> R<String> {
    lists.get(name).cloned().ok_or_else(|| {
        format!("файла {name} нет в активной версии Flowseal или его не удалось проверить")
    })
}

/// A page of an original Flowseal file (searchable, for very long lists).
#[tauri::command]
pub async fn flowseal_list_page(
    state: State<'_, AppState>,
    name: String,
    query: String,
    offset: usize,
    limit: usize,
) -> R<ListPage> {
    let (_, lists, _) = active_lists(&state).await;
    let t = list_text(&lists, &name)?;
    Ok(core::page(&name, &t, &query, offset, limit))
}

/// The whole original file (for copying), unchanged.
#[tauri::command]
pub async fn flowseal_list_text(state: State<'_, AppState>, name: String) -> R<String> {
    let (_, lists, _) = active_lists(&state).await;
    list_text(&lists, &name)
}
