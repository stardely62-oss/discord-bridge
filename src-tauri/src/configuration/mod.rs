//! ConfigurationManager: typed, versioned settings with atomic writes,
//! corruption recovery and backups before migration.

use bridge_core::atomic::{backup_before_migration, read_with_recovery, write_atomic};
use bridge_core::lists::{InstalledPack, IpsetMode, UserLists};
use bridge_core::ports::GameFilter;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const SETTINGS_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CloseBehavior {
    /// Hide the window, keep running in the tray.
    #[default]
    Tray,
    /// Quit the application.
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfirmation {
    /// Unix seconds.
    pub at: u64,
    pub works: bool,
    /// Strategy the confirmation was made with (informational; the map key
    /// is the hash of the effective configuration).
    #[serde(default)]
    pub strategy_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    pub language: String,
    /// `system`, `dark` or `light`.
    pub theme: String,
    pub onboarding_done: bool,
    pub launch_at_startup: bool,
    pub start_minimized: bool,
    pub auto_enable_bypass: bool,
    pub auto_launch_discord: bool,
    pub close_behavior: CloseBehavior,
    /// Stop our engine when the app fully exits.
    pub stop_bypass_on_exit: bool,
    pub tray_notifications: bool,
    pub discord_path: Option<String>,
    pub active_strategy_id: Option<String>,
    pub last_working_strategy_id: Option<String>,
    pub bypass_enabled: bool,
    pub game_filter: GameFilter,
    pub ipset_mode: IpsetMode,
    /// The user's own entries only; catalog packs live in `installed_packs`.
    pub user_lists: UserLists,
    /// Catalog packs, merged with `user_lists` only when the engine
    /// configuration is built (`bridge_core::lists::effective_lists`).
    pub installed_packs: Vec<InstalledPack>,
    pub keep_logs: bool,
    pub check_updates_on_start: bool,
    /// Install a newer Flowseal at startup without asking, when the project
    /// has verified it (signed manifest) and no strategy of the user would
    /// break. Off by default. Missing in older files: off.
    pub auto_install_flowseal: bool,
    /// Strategy ids the Auto mode may try, in order. Empty = all built-ins.
    pub auto_candidates: Vec<String>,
    pub auto_max_attempts: u32,
    /// User confirmations of voice chat, per strategy id.
    /// Voice confirmations keyed by the effective configuration hash
    /// (`bridge_core::strategy::config_hash`): any relevant change — rules,
    /// Flowseal version, lists, game filter — makes an old confirmation stale.
    pub voice_confirmations: BTreeMap<String, VoiceConfirmation>,
    /// What «Запустить Discord» does by default.
    pub launch_mode: LaunchMode,
    /// Telegram WS Proxy (independent of the bypass; the secret is stored
    /// separately, never here). Missing in older files: defaults.
    pub telegram_proxy: bridge_telegram::TelegramProxySettings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LaunchMode {
    /// Make sure the bypass runs, then launch Discord.
    #[default]
    WithBypass,
    /// Launch Discord as is, never touching the bypass.
    WithoutBypass,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema_version: SETTINGS_VERSION,
            language: "ru".into(),
            theme: "system".into(),
            onboarding_done: false,
            launch_at_startup: false,
            start_minimized: false,
            auto_enable_bypass: false,
            auto_launch_discord: false,
            close_behavior: CloseBehavior::Tray,
            stop_bypass_on_exit: true,
            tray_notifications: true,
            discord_path: None,
            active_strategy_id: Some("flowseal-general".into()),
            last_working_strategy_id: None,
            bypass_enabled: false,
            game_filter: GameFilter::default(),
            ipset_mode: IpsetMode::None,
            user_lists: UserLists::default(),
            installed_packs: Vec::new(),
            keep_logs: true,
            check_updates_on_start: true,
            auto_install_flowseal: false,
            auto_candidates: vec![],
            auto_max_attempts: 8,
            voice_confirmations: BTreeMap::new(),
            launch_mode: LaunchMode::WithBypass,
            telegram_proxy: Default::default(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.language.as_str(), "ru" | "en") {
            return Err("неподдерживаемый язык".into());
        }
        if !matches!(self.theme.as_str(), "system" | "dark" | "light") {
            return Err("неподдерживаемая тема".into());
        }
        self.game_filter
            .validate()
            .map_err(|e| format!("игровой фильтр: {e}"))?;
        self.user_lists.validate().map_err(|e| e.to_string())?;
        bridge_core::lists::validate_packs(&self.user_lists, &self.installed_packs)
            .map_err(|e| e.to_string())?;
        if self.auto_max_attempts == 0 || self.auto_max_attempts > 50 {
            return Err("число попыток Auto должно быть от 1 до 50".into());
        }
        if let Some(p) = &self.discord_path {
            if p.len() > 1024 {
                return Err("слишком длинный путь Discord".into());
            }
        }
        self.telegram_proxy.validate()?;
        Ok(())
    }
}

pub struct ConfigurationManager {
    path: PathBuf,
    pub settings: Settings,
    /// Set when the settings file was corrupted and restored from backup.
    pub recovered: bool,
}

fn parse(bytes: &[u8]) -> Result<(Settings, Option<u64>), String> {
    let mut v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let ver = v.get("schemaVersion").and_then(|x| x.as_u64()).unwrap_or(0);
    if ver > SETTINGS_VERSION as u64 {
        return Err(format!("настройки созданы более новой версией ({ver})"));
    }
    // v0 → v1: `closeToTray: bool` became `closeBehavior`.
    if ver == 0 {
        if let Some(obj) = v.as_object_mut() {
            if let Some(b) = obj.remove("closeToTray").and_then(|b| b.as_bool()) {
                obj.insert(
                    "closeBehavior".into(),
                    if b { "tray" } else { "exit" }.into(),
                );
            }
        }
    }
    // v1 → v2: voice confirmations were keyed by strategy id and cannot be
    // mapped to a configuration hash — drop them (they become "not checked").
    if ver <= 1 {
        if let Some(obj) = v.as_object_mut() {
            obj.remove("voiceConfirmations");
            obj.insert("schemaVersion".into(), SETTINGS_VERSION.into());
        }
    }
    let s: Settings = serde_json::from_value(v).map_err(|e| e.to_string())?;
    s.validate()?;
    Ok((s, (ver != SETTINGS_VERSION as u64).then_some(ver)))
}

impl ConfigurationManager {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("settings.json");
        let loaded = read_with_recovery(&path, parse).ok().flatten();
        match loaded {
            Some(((settings, migrated), recovered)) => {
                if let Some(from) = migrated {
                    let _ = backup_before_migration(&path, from);
                }
                let mgr = ConfigurationManager {
                    path,
                    settings,
                    recovered,
                };
                if migrated.is_some() {
                    let _ = mgr.save();
                }
                mgr
            }
            None => ConfigurationManager {
                path,
                settings: Settings::default(),
                recovered: false,
            },
        }
    }

    pub fn save(&self) -> Result<(), String> {
        self.settings.validate()?;
        let data = serde_json::to_vec_pretty(&self.settings).map_err(|e| e.to_string())?;
        write_atomic(&self.path, &data).map_err(|e| e.to_string())
    }

    /// Validate → write atomically → only then make it the in-memory state.
    /// On any error the previous settings stay active, matching the disk.
    pub fn update(&mut self, new: Settings) -> Result<(), String> {
        new.validate()?;
        let data = serde_json::to_vec_pretty(&new).map_err(|e| e.to_string())?;
        write_atomic(&self.path, &data)
            .map_err(|e| format!("не удалось сохранить настройки: {e}"))?;
        self.settings = new;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip_and_recovery() {
        let d = tempfile::tempdir().unwrap();
        let mut m = ConfigurationManager::load(d.path().to_path_buf());
        assert_eq!(m.settings.language, "ru");
        m.settings.auto_launch_discord = true;
        m.save().unwrap();
        m.settings.start_minimized = true;
        m.save().unwrap();
        std::fs::write(d.path().join("settings.json"), b"{broken").unwrap();
        let m2 = ConfigurationManager::load(d.path().to_path_buf());
        assert!(m2.recovered);
        assert!(m2.settings.auto_launch_discord);
    }

    #[test]
    fn migrates_v0_with_backup() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("settings.json"),
            br#"{"closeToTray":false,"language":"en"}"#,
        )
        .unwrap();
        let m = ConfigurationManager::load(d.path().to_path_buf());
        assert_eq!(m.settings.close_behavior, CloseBehavior::Exit);
        assert_eq!(m.settings.language, "en");
        assert!(d.path().join("settings.json.v0.bak").exists());
    }

    #[test]
    fn failed_write_keeps_previous_state_in_memory() {
        let d = tempfile::tempdir().unwrap();
        let mut m = ConfigurationManager::load(d.path().to_path_buf());
        m.save().unwrap();
        // Make the target unwritable by replacing it with a directory.
        let p = d.path().join("settings.json");
        std::fs::remove_file(&p).unwrap();
        std::fs::create_dir(&p).unwrap();
        let mut next = m.settings.clone();
        next.auto_launch_discord = true;
        assert!(m.update(next).is_err());
        assert!(
            !m.settings.auto_launch_discord,
            "memory must not diverge from disk"
        );
        // invalid candidates are rejected before writing
        let bad = Settings {
            language: "xx".into(),
            ..Default::default()
        };
        assert!(m.update(bad).is_err());
        assert_eq!(m.settings.language, "ru");
    }

    #[test]
    fn migrates_v1_dropping_id_keyed_voice_confirmations() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("settings.json"),
            br#"{"schemaVersion":1,"language":"ru","voiceConfirmations":{"flowseal-general":{"at":1,"works":true}}}"#,
        )
        .unwrap();
        let m = ConfigurationManager::load(d.path().to_path_buf());
        assert!(m.settings.voice_confirmations.is_empty());
        assert_eq!(m.settings.schema_version, SETTINGS_VERSION);
        assert_eq!(m.settings.launch_mode, LaunchMode::WithBypass);
        assert!(d.path().join("settings.json.v1.bak").exists());
    }

    #[test]
    fn rejects_invalid() {
        let s = Settings {
            language: "xx".into(),
            ..Default::default()
        };
        assert!(s.validate().is_err());
        let t = Settings {
            theme: "neon".into(),
            ..Default::default()
        };
        assert!(t.validate().is_err());
        assert_eq!(Settings::default().theme, "system");
        let dup = Settings {
            installed_packs: vec![
                InstalledPack {
                    id: "a".into(),
                    version: None,
                    domains: vec!["x.com".into()],
                    ips: vec![],
                    title: None,
                    previous: None,
                },
                InstalledPack {
                    id: "a".into(),
                    version: None,
                    domains: vec!["y.com".into()],
                    ips: vec![],
                    title: None,
                    previous: None,
                },
            ],
            ..Default::default()
        };
        assert!(dup.validate().is_err());
    }

    #[test]
    fn settings_without_packs_load_with_an_empty_list() {
        let s: Settings =
            serde_json::from_str(r#"{"schemaVersion":2,"userLists":{"general":["a.com"]}}"#)
                .unwrap();
        assert!(s.installed_packs.is_empty());
        assert_eq!(s.user_lists.general, vec!["a.com"]);
    }

    #[test]
    fn automatic_update_checks_default_on_and_persist_when_turned_off() {
        let d = tempfile::tempdir().unwrap();
        let mut m = ConfigurationManager::load(d.path().to_path_buf());
        assert!(m.settings.check_updates_on_start);
        let mut s = m.settings.clone();
        s.check_updates_on_start = false;
        m.update(s).unwrap();
        let again = ConfigurationManager::load(d.path().to_path_buf());
        assert!(!again.settings.check_updates_on_start);
        // Older settings files without the field keep the default (on).
        let s: Settings = serde_json::from_str(r#"{"schemaVersion":2}"#).unwrap();
        assert!(s.check_updates_on_start);
    }
}
