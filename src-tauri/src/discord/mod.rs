//! DiscordManager: finds and launches the official, unmodified Discord
//! client. It never touches Discord files, tokens, messages or updates.

use crate::platform::processes;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DOWNLOAD_URL: &str = "https://discord.com/download";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordInstall {
    pub channel: String,
    pub root: String,
    pub exe: String,
    pub update_exe: Option<String>,
    pub version: Option<String>,
    pub custom: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordStatus {
    pub installed: bool,
    pub install: Option<DiscordInstall>,
    pub running: bool,
    pub running_pids: Vec<u32>,
    pub custom_path_error: Option<String>,
}

const CHANNELS: &[(&str, &str, &str)] = &[
    ("Stable", "Discord", "Discord.exe"),
    ("PTB", "DiscordPTB", "DiscordPTB.exe"),
    ("Canary", "DiscordCanary", "DiscordCanary.exe"),
];

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Squirrel layout: `<root>\Update.exe`, `<root>\app-1.0.9xxx\Discord.exe`.
pub fn inspect_root(root: &Path, channel: &str, exe_name: &str) -> Option<DiscordInstall> {
    let mut best: Option<(String, PathBuf)> = None;
    for e in std::fs::read_dir(root).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if let Some(ver) = name.strip_prefix("app-") {
            let exe = e.path().join(exe_name);
            if exe.is_file()
                && best
                    .as_ref()
                    .is_none_or(|(b, _)| version_key(ver) > version_key(b))
            {
                best = Some((ver.to_string(), exe));
            }
        }
    }
    let (ver, exe) = best?;
    let update = root.join("Update.exe");
    Some(DiscordInstall {
        channel: channel.into(),
        root: root.to_string_lossy().into(),
        exe: exe.to_string_lossy().into(),
        update_exe: update.is_file().then(|| update.to_string_lossy().into()),
        version: Some(ver),
        custom: false,
    })
}

pub fn detect_default(local_app_data: &Path) -> Option<DiscordInstall> {
    CHANNELS
        .iter()
        .find_map(|(ch, dir, exe)| inspect_root(&local_app_data.join(dir), ch, exe))
}

/// Validates a user-chosen path: must be an existing Discord executable or
/// its Squirrel `Update.exe`.
pub fn from_custom(path: &str) -> Result<DiscordInstall, String> {
    let p = PathBuf::from(path);
    if !p.is_file() {
        return Err("файл не найден".into());
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match name.as_str() {
        "update.exe" => {
            let root = p.parent().ok_or("некорректный путь")?;
            CHANNELS
                .iter()
                .find_map(|(ch, _, exe)| inspect_root(root, ch, exe))
                .map(|mut i| {
                    i.custom = true;
                    i
                })
                .ok_or_else(|| "рядом с Update.exe не найдена установка Discord".into())
        }
        "discord.exe" | "discordptb.exe" | "discordcanary.exe" => {
            let app_dir = p.parent().ok_or("некорректный путь")?;
            let root = app_dir.parent().unwrap_or(app_dir);
            let version = app_dir
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("app-"))
                .map(str::to_string);
            let update = root.join("Update.exe");
            Ok(DiscordInstall {
                channel: "Custom".into(),
                root: root.to_string_lossy().into(),
                exe: p.to_string_lossy().into(),
                update_exe: update.is_file().then(|| update.to_string_lossy().into()),
                version,
                custom: true,
            })
        }
        _ => Err("ожидается Discord.exe, DiscordPTB.exe, DiscordCanary.exe или Update.exe".into()),
    }
}

/// Does a process belong to this Discord installation? The executable name
/// must match the channel (`Discord.exe` / `DiscordPTB.exe` / `DiscordCanary.exe`)
/// and, when the image path is readable, it must lie inside the install root.
pub fn matches_install(name: &str, image: Option<&str>, install: &DiscordInstall) -> bool {
    let want = Path::new(&install.exe)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.to_lowercase() != want {
        return false;
    }
    match image {
        Some(path) => {
            let norm = |p: &str| p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
            let root = norm(&install.root);
            let path = norm(path);
            path.starts_with(&(root + "\\"))
        }
        None => true,
    }
}

pub fn running_pids(install: &DiscordInstall) -> Vec<u32> {
    processes()
        .into_iter()
        .filter(|p| {
            matches_install(
                &p.name,
                crate::platform::process_path(p.pid).as_deref(),
                install,
            )
        })
        .map(|p| p.pid)
        .collect()
}

pub fn status(custom: Option<&str>) -> DiscordStatus {
    let mut custom_path_error = None;
    let install = match custom {
        Some(c) if !c.is_empty() => match from_custom(c) {
            Ok(i) => Some(i),
            Err(e) => {
                custom_path_error = Some(e);
                None
            }
        },
        _ => std::env::var_os("LOCALAPPDATA").and_then(|l| detect_default(Path::new(&l))),
    };
    let pids = install.as_ref().map(running_pids).unwrap_or_default();
    DiscordStatus {
        installed: install.is_some(),
        install,
        running: !pids.is_empty(),
        running_pids: pids,
        custom_path_error,
    }
}

/// Launch Discord the same way its own shortcut does
/// (`Update.exe --processStart Discord.exe`), or the exe directly.
pub fn launch(install: &DiscordInstall) -> Result<(), String> {
    let exe = PathBuf::from(&install.exe);
    let exe_name = exe
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("некорректный путь")?;
    let mut cmd = match &install.update_exe {
        Some(u) => {
            let mut c = Command::new(u);
            c.arg("--processStart").arg(&exe_name);
            c
        }
        None => Command::new(&exe),
    };
    cmd.current_dir(&install.root);
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("не удалось запустить Discord: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_newest_app_dir() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("Discord");
        for v in ["1.0.9150", "1.0.9200", "1.0.990"] {
            std::fs::create_dir_all(root.join(format!("app-{v}"))).unwrap();
            std::fs::write(root.join(format!("app-{v}")).join("Discord.exe"), b"").unwrap();
        }
        std::fs::write(root.join("Update.exe"), b"").unwrap();
        let i = detect_default(d.path()).unwrap();
        assert_eq!(i.version.as_deref(), Some("1.0.9200"));
        assert!(i.update_exe.is_some());
    }

    #[test]
    fn channels_are_not_confused() {
        let inst = |ch: &str, dir: &str, exe: &str| DiscordInstall {
            channel: ch.into(),
            root: format!("C:\\Users\\u\\AppData\\Local\\{dir}"),
            exe: format!("C:\\Users\\u\\AppData\\Local\\{dir}\\app-1.0.1\\{exe}"),
            update_exe: None,
            version: None,
            custom: false,
        };
        let stable = inst("Stable", "Discord", "Discord.exe");
        let ptb = inst("PTB", "DiscordPTB", "DiscordPTB.exe");
        let ptb_path = "C:\\Users\\u\\AppData\\Local\\DiscordPTB\\app-1.0.1\\DiscordPTB.exe";
        assert!(
            !matches_install("DiscordPTB.exe", Some(ptb_path), &stable),
            "PTB running is not Stable running"
        );
        assert!(matches_install("DiscordPTB.exe", Some(ptb_path), &ptb));
        assert!(
            matches_install("discord.exe", None, &stable),
            "unknown path: name decides"
        );
        // same exe name, different install root (portable copy)
        assert!(!matches_install(
            "Discord.exe",
            Some("D:\\Other\\Discord\\Discord.exe"),
            &stable
        ));
        // prefix trap: "Discord" must not match "DiscordPTB"
        let trap = "C:\\Users\\u\\AppData\\Local\\DiscordX\\Discord.exe";
        assert!(!matches_install("Discord.exe", Some(trap), &stable));
    }

    #[test]
    fn missing_and_invalid_paths() {
        let d = tempfile::tempdir().unwrap();
        assert!(detect_default(d.path()).is_none());
        assert!(from_custom(&d.path().join("nope.exe").to_string_lossy()).is_err());
        let other = d.path().join("calc.exe");
        std::fs::write(&other, b"").unwrap();
        assert!(from_custom(&other.to_string_lossy()).is_err());
        let s = status(Some(&other.to_string_lossy()));
        assert!(!s.installed && s.custom_path_error.is_some());
    }

    #[test]
    fn custom_exe_path() {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("app-1.0.9001");
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(app.join("Discord.exe"), b"").unwrap();
        let i = from_custom(&app.join("Discord.exe").to_string_lossy()).unwrap();
        assert_eq!(i.version.as_deref(), Some("1.0.9001"));
        assert!(i.custom);
    }
}
