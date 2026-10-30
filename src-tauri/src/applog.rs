// The app's log, its version and folders (Settings > About, Plugins, Backup).
//
// The log is a plain text file, logs\minniondeck.log in the app data folder: one line per
// error or important event (backups, imports, phones pairing and connecting, actions that
// failed). When it passes 1 MB it is renamed to minniondeck.old.log and a new one starts.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::storage::{Store, now_ms};

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
const MAX_BYTES: u64 = 1024 * 1024;

pub fn init(root: &Path) {
    let dir = root.join("logs");
    let _ = fs::create_dir_all(&dir);
    let _ = LOG_DIR.set(dir);
}

/// Write one line to the log (and to the console when run from a terminal)
pub fn line(message: &str) {
    eprintln!("{message}");
    let Some(dir) = LOG_DIR.get() else { return };
    let file = dir.join("minniondeck.log");
    if fs::metadata(&file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = fs::rename(&file, dir.join("minniondeck.old.log"));
    }
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&file) {
        let _ = writeln!(f, "{}  {}", crate::backups::stamp(now_ms()), message.replace(['\r', '\n'], " "));
    }
}

/// A line from the UI: what it couldn't do that nobody may have seen (auto-save off, a failed
/// load or save, an error at start), so the log can say why
#[tauri::command]
pub fn ui_log(text: String) {
    line(&format!("UI: {}", text.chars().take(600).collect::<String>()));
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub backups_dir: String,
    pub logs_dir: String,
    pub plugins_dir: String,
}

fn plugins_dir(store: &Store) -> PathBuf {
    store.paths.root.join("plugins")
}

/// The real version and folders, for the Settings page
#[tauri::command]
pub fn app_info(app: AppHandle, store: State<Store>) -> AppInfo {
    let p = &store.paths;
    let _ = fs::create_dir_all(plugins_dir(&store));
    AppInfo {
        version: app.package_info().version.to_string(),
        data_dir: p.root.display().to_string(),
        backups_dir: p.backups.display().to_string(),
        logs_dir: p.root.join("logs").display().to_string(),
        plugins_dir: plugins_dir(&store).display().to_string(),
    }
}

/// Open one of the app's own folders in Explorer. Only these folders can be opened.
#[tauri::command]
pub fn open_app_folder(store: State<Store>, which: String) -> Result<(), String> {
    let p = &store.paths;
    let dir = match which.as_str() {
        "data" => p.root.clone(),
        "backups" => p.backups.clone(),
        "logs" => p.root.join("logs"),
        "plugins" => plugins_dir(&store),
        _ => return Err("That folder can't be opened.".into()),
    };
    fs::create_dir_all(&dir).map_err(|e| format!("Could not open {}: {e}", dir.display()))?;
    std::process::Command::new("explorer").arg(&dir).spawn().map_err(|e| format!("Could not open {}: {e}", dir.display()))?;
    Ok(())
}
