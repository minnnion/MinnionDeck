// Where Minniondeck keeps its data, and the safe way to write it.
//
// %APPDATA%\com.minnion.deck\
//   library\library.json   profiles, folders, pages, buttons, designs, GIFs, crew
//   library\media\         pictures and GIF files, one file each
//   settings.json          the Settings page
//   devices.json           paired phones (written by phones/hub.rs)
//   backups\               up to 5 encrypted backups of the library
//
// Each JSON file is { "schema_version": 1, "saved_at": <unix ms>, "data": <what the UI sent> }.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

pub const SCHEMA_VERSION: u64 = 1;
/// Largest picture or GIF file the app accepts
pub const MAX_MEDIA_BYTES: usize = 25 * 1024 * 1024;

pub struct Paths {
    pub root: PathBuf,
    pub library: PathBuf,
    pub media: PathBuf,
    pub backups: PathBuf,
}

impl Paths {
    fn new(app: &AppHandle) -> Result<Self, String> {
        let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let library = root.join("library");
        let paths = Paths { media: library.join("media"), backups: root.join("backups"), library, root };
        for dir in [&paths.root, &paths.library, &paths.media, &paths.backups] {
            fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        }
        Ok(paths)
    }
    pub fn library_file(&self) -> PathBuf {
        self.library.join("library.json")
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn devices_file(&self) -> PathBuf {
        self.root.join("devices.json")
    }
}

pub struct Store {
    pub paths: Paths,
    /// Hash of what was last written for each file, so unchanged parts are never rewritten
    written: Mutex<HashMap<&'static str, [u8; 32]>>,
    /// One writer at a time: saves, backups and restores all take this
    pub lock: Mutex<()>,
    /// Set while a backup is restored. The app restarts and nothing may write until then.
    pub frozen: AtomicBool,
}

impl Store {
    pub fn new(app: &AppHandle) -> Result<Self, String> {
        Ok(Store { paths: Paths::new(app)?, written: Mutex::new(HashMap::new()), lock: Mutex::new(()), frozen: AtomicBool::new(false) })
    }

    pub fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Write whichever parts the UI sent. Each part is JSON text.
    pub fn save(&self, library: Option<String>, settings: Option<String>) -> Result<(), String> {
        if self.frozen.load(Ordering::SeqCst) {
            return Ok(());
        }
        let _guard = self.guard();
        let parts = [
            ("library", self.paths.library_file(), library),
            ("settings", self.paths.settings_file(), settings),
        ];
        for (key, path, data) in parts {
            let Some(data) = data else { continue };
            // Refuse anything that isn't a JSON object, so a bug in the UI can't wipe a file
            let value: Value = serde_json::from_str(&data).map_err(|e| format!("The {key} data is not valid: {e}"))?;
            if !value.is_object() {
                return Err(format!("The {key} data is not valid."));
            }
            let hash: [u8; 32] = Sha256::digest(data.as_bytes()).into();
            let mut written = self.written.lock().unwrap_or_else(|e| e.into_inner());
            if written.get(key) == Some(&hash) {
                continue;
            }
            let body = format!("{{\"schema_version\":{SCHEMA_VERSION},\"saved_at\":{},\"data\":{data}}}", now_ms());
            write_atomic(&path, body.as_bytes())?;
            // The first write of each part in a run goes in the log: a run with none (after
            // changes) means the UI never saved, which once went unseen for a day
            if !written.contains_key(key) {
                crate::applog::line(&format!("Saved {key} ({} KB)", body.len() / 1024));
            }
            written.insert(key, hash);
        }
        Ok(())
    }
}

/// Write to a temporary file, flush it to disk, then swap it in. A crash mid-write
/// leaves the previous file untouched.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let fail = |e: std::io::Error| format!("Could not save {}: {e}", path.display());
    {
        let mut f = fs::File::create(&tmp).map_err(fail)?;
        f.write_all(bytes).map_err(fail)?;
        f.sync_all().map_err(fail)?;
    }
    fs::rename(&tmp, path).map_err(fail)
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// A plain file name with no folders in it, safe to join onto a directory
pub fn safe_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok.then_some(name)
}

/// The "data" of a saved JSON file, if it can be read
pub fn read_data(path: &Path) -> Option<Value> {
    let v: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    v.get("data").filter(|d| d.is_object()).cloned()
}

/// One value from the saved Settings page, such as "port" or "keep"
pub fn setting_value(store: &Store, key: &str) -> Option<Value> {
    let text = fs::read_to_string(store.paths.settings_file()).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v["data"].get(key).cloned()
}

/// Read one saved file and return its "data" as JSON text. A damaged file is moved
/// aside (never overwritten) and reported, and the app starts with its defaults.
fn read_part(path: &Path, warnings: &mut Vec<String>) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let parsed: Result<Value, _> = serde_json::from_str(&text);
    match parsed {
        Ok(v) if v["schema_version"].as_u64().unwrap_or(0) > SCHEMA_VERSION => {
            warnings.push(format!("{} was saved by a newer version of Minniondeck, so it was not loaded.", file_label(path)));
            None
        }
        Ok(v) if v["data"].is_object() => Some(v["data"].to_string()),
        _ => {
            let mut aside = path.as_os_str().to_owned();
            aside.push(format!(".damaged-{}", now_ms()));
            let _ = fs::rename(path, PathBuf::from(&aside));
            warnings.push(format!("{} was damaged. It was kept as {} and the defaults were loaded.", file_label(path), PathBuf::from(aside).display()));
            None
        }
    }
}

fn file_label(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loaded {
    pub library: Option<String>,
    pub settings: Option<String>,
    /// Folder holding pictures and GIF files, so the UI can show them
    pub media_dir: String,
    pub warnings: Vec<String>,
}

/// Everything the UI needs on start. Missing parts are None and the UI keeps its defaults.
#[tauri::command]
pub async fn load_state(app: AppHandle) -> Result<Loaded, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let _guard = store.guard();
        let mut warnings = Vec::new();
        let p = &store.paths;
        Ok(Loaded {
            library: read_part(&p.library_file(), &mut warnings),
            settings: read_part(&p.settings_file(), &mut warnings),
            media_dir: p.media.display().to_string(),
            warnings,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Auto-save: the UI sends only the parts that changed, as JSON text. Connected phones are
/// brought up to date from the same save.
#[tauri::command]
pub async fn save_state(app: AppHandle, library: Option<String>, settings: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = app.state::<Store>().save(library.clone(), settings.clone()) {
            crate::applog::line(&format!("Could not save: {e}"));
            return Err(e);
        }
        if let Some(hub) = app.try_state::<std::sync::Arc<crate::phones::hub::Hub>>() {
            if let Some(s) = &settings {
                hub.set_settings(s);
            }
            if let Some(l) = &library {
                hub.set_library(l);
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Store an uploaded picture or GIF file once, as its own file. Returns its file name.
#[tauri::command]
pub async fn save_media(app: AppHandle, id: String, data_url: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (head, data) = data_url.split_once(',').ok_or("That picture could not be read.")?;
        let ext = match head {
            "data:image/png;base64" => "png",
            "data:image/jpeg;base64" => "jpg",
            "data:image/gif;base64" => "gif",
            "data:image/webp;base64" => "webp",
            _ => return Err("Only PNG, JPEG, GIF and WebP pictures can be saved.".to_string()),
        };
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| "That picture could not be read.")?;
        if bytes.len() > MAX_MEDIA_BYTES {
            return Err("That picture is larger than 25 MB.".to_string());
        }
        let stem: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).take(40).collect();
        let name = format!("{stem}-{}.{ext}", &sha256_hex(&bytes)[..12]);
        let store = app.state::<Store>();
        let path = store.paths.media.join(&name);
        if !path.exists() {
            write_atomic(&path, &bytes)?;
        }
        Ok(name)
    })
    .await
    .map_err(|e| e.to_string())?
}
