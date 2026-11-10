//! The Settings page, kept in <data>\settings.json in the Minniondeck app's format
//! ({ "schema_version": 1, "saved_at": <ms>, "data": { "theme": "light", "startWin": true, ... } }),
//! so the file moves between the two apps. Keys this window doesn't show are kept as they were read.
//!
//! The window holds a setting as a number (a switch is 0 or 1, a choice is its position); this
//! file maps each to its key and words in settings.json.
use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use super::paths::Paths;
use super::util::now_ms;

/// Saved this long after the last change.
const SAVE_AFTER: f64 = 0.6;

enum Kind {
    /// A switch: 0 or 1 here, true or false in the file
    Switch,
    /// A choice: its position here, its word in the file
    Choice(&'static [&'static str]),
}

/// (the window's key, the key in settings.json, what it holds)
const MAP: &[(&str, &str, Kind)] = &[
    ("startWin", "startWin", Kind::Switch),
    ("tray", "tray", Kind::Switch),
    ("close", "close", Kind::Choice(&["tray", "quit"])),
    ("theme", "theme", Kind::Choice(&["system", "light", "dark"])),
    ("anim", "anim", Kind::Choice(&["full", "reduced", "off"])),
    ("tileSize", "tileSize", Kind::Choice(&["s", "m", "l"])),
    ("names", "names", Kind::Switch),
    ("crew", "crew", Kind::Switch),
    ("autoSwitch", "autoSwitch", Kind::Switch),
    ("back", "backBtn", Kind::Switch),
    ("conn", "conn", Kind::Choice(&["wifi", "usb", "both"])),
    ("match", "pin", Kind::Switch),
    ("secure", "securePhoneLink", Kind::Switch),
    ("follow", "followPc", Kind::Switch),
    ("awake", "awake", Kind::Switch),
    ("vibe", "haptics", Kind::Switch),
    ("plugUpd", "plugUpd", Kind::Switch),
    ("backup", "autoBackup", Kind::Switch),
    ("updDaily", "appUpd", Kind::Switch),
    ("chan", "appChannel", Kind::Choice(&["stable", "beta"])),
    ("phoneUpd", "bundlePhone", Kind::Switch),
];

pub struct SettingsFile {
    path: PathBuf,
    /// The file's "data", with every key it had
    data: Map<String, Value>,
    /// False in screenshot mode: nothing is read into the window or written
    live: bool,
    seen: u64,
    changed_at: f64,
    /// The Profiles page's switch and the Settings page's, kept as one
    pub prev_auto: bool,
}

fn value_of(kind: &Kind, n: usize) -> Value {
    match kind {
        Kind::Switch => json!(n == 1),
        Kind::Choice(words) => json!(words.get(n).copied().unwrap_or(words[0])),
    }
}

fn number_of(kind: &Kind, v: &Value) -> Option<usize> {
    match kind {
        Kind::Switch => v.as_bool().map(|b| b as usize),
        Kind::Choice(words) => v.as_str().and_then(|s| words.iter().position(|w| *w == s)),
    }
}

impl SettingsFile {
    /// Reads settings.json (a missing or unreadable one is the defaults). `live` false makes it inert.
    pub fn open(paths: &Paths, live: bool) -> SettingsFile {
        let path = paths.settings_file();
        let data = read_data(paths).and_then(|d| d.as_object().cloned()).unwrap_or_default();
        SettingsFile { path, data, live, seen: 0, changed_at: 0.0, prev_auto: true }
    }

    /// Puts what the file holds into the window's settings.
    pub fn apply(&self, st: &mut HashMap<&'static str, usize>) {
        if !self.live {
            return;
        }
        for (key, file_key, kind) in MAP {
            if let Some(n) = self.data.get(*file_key).and_then(|v| number_of(kind, v)) {
                st.insert(key, n);
            }
        }
    }

    /// Every frame: writes the settings a moment after one changed. Returns a message when saving failed.
    pub fn tick(&mut self, st: &HashMap<&'static str, usize>, now: f64) -> Option<String> {
        if !self.live {
            return None;
        }
        // what the window holds, as the file would
        let mut changes: Vec<(&str, Value)> = vec![];
        for (key, file_key, kind) in MAP {
            if let Some(n) = st.get(key) {
                let v = value_of(kind, *n);
                if self.data.get(*file_key) != Some(&v) {
                    changes.push((file_key, v));
                }
            }
        }
        let sig = changes.len() as u64 * 1_000_003 + changes.iter().map(|(k, v)| (k.len() as u64) * 31 + v.to_string().len() as u64).sum::<u64>();
        if changes.is_empty() {
            self.seen = 0;
            return None;
        }
        if sig != self.seen {
            self.seen = sig;
            self.changed_at = now;
        }
        if now - self.changed_at < SAVE_AFTER {
            return None;
        }
        for (k, v) in changes {
            self.data.insert(k.to_string(), v);
        }
        self.seen = 0;
        self.write().err()
    }

    /// A value this window keeps that isn't a setting on the page (when updates were last checked).
    pub fn get_u64(&self, key: &str) -> u64 {
        self.data.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
    }

    /// Saves such a value now.
    pub fn set_extra(&mut self, key: &str, value: Value) {
        if self.live {
            self.data.insert(key.to_string(), value);
            let _ = self.write();
        }
    }

    /// True while a change is waiting to be written (the window should look again soon).
    pub fn pending(&self) -> bool {
        self.seen != 0
    }

    /// Writes everything now (when the window closes).
    pub fn flush(&mut self, st: &HashMap<&'static str, usize>) {
        if self.live {
            self.changed_at = f64::NEG_INFINITY;
            let _ = self.tick(st, 0.0);
        }
    }

    fn write(&self) -> Result<(), String> {
        let text = serde_json::to_string(&json!({ "schema_version": 1, "saved_at": now_ms(), "data": self.data })).map_err(|e| e.to_string())?;
        super::vault::write(&self.path, super::package::Kind::Save, text.as_bytes()).map_err(|e| format!("Couldn't save your settings: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("md-settings-file-{tag}-{}", now_ms()));
        let p = Paths { root: dir.clone(), library: dir.join("library"), media: dir.join("library").join("media"), backups: dir.join("backups") };
        p.make_folders().unwrap();
        p
    }

    #[test]
    fn settings_come_in_and_go_out_in_the_apps_words() {
        let paths = temp_paths("words");
        // an earlier version's plain settings.json is sealed when read
        std::fs::write(paths.legacy_settings_file(), r#"{"schema_version":1,"saved_at":1,"data":{"theme":"dark","tray":true,"anim":"reduced","autoSwitch":false,"pluginsPath":"C:\\x","port":"47800"}}"#).unwrap();
        let mut f = SettingsFile::open(&paths, true);
        let mut st = HashMap::new();
        f.apply(&mut st);
        assert_eq!(st["theme"], 2);
        assert_eq!(st["tray"], 1);
        assert_eq!(st["anim"], 1);
        assert_eq!(st["autoSwitch"], 0);
        assert!(!st.contains_key("names"), "what the file doesn't say stays the window's default");

        // nothing changed: nothing is written
        assert!(f.tick(&st, 1.0).is_none() && !f.pending());
        // a change waits its moment, then is written; keys the window doesn't show are kept
        st.insert("theme", 1);
        st.insert("close", 1);
        assert!(f.tick(&st, 10.0).is_none() && f.pending());
        assert!(f.tick(&st, 10.2).is_none());
        let r = f.tick(&st, 10.7);
        assert!(r.is_none() && !f.pending(), "{r:?}");
        assert!(!paths.legacy_settings_file().exists(), "the plain file is gone");
        assert!(!String::from_utf8_lossy(&std::fs::read(paths.settings_file()).unwrap()).contains("pluginsPath"), "nothing readable on disk");
        let saved: Value = super::super::vault::read_json(&paths.settings_file(), super::super::package::Kind::Save).unwrap();
        assert_eq!(saved["schema_version"], 1);
        assert_eq!(saved["data"]["theme"], "light");
        assert_eq!(saved["data"]["close"], "quit");
        assert_eq!(saved["data"]["pluginsPath"], "C:\\x");
        assert_eq!(saved["data"]["port"], "47800");
        // read back by a fresh start
        let mut st2 = HashMap::new();
        SettingsFile::open(&paths, true).apply(&mut st2);
        assert_eq!((st2["theme"], st2["close"]), (1, 1));
        let _ = std::fs::remove_dir_all(&paths.root);
    }

    #[test]
    fn a_screenshot_never_reads_or_writes_settings() {
        let paths = temp_paths("shot");
        let mut f = SettingsFile::open(&paths, false);
        let mut st = HashMap::new();
        st.insert("theme", 2);
        assert!(f.tick(&st, 5.0).is_none() && f.tick(&st, 9.0).is_none());
        assert!(!paths.settings_file().exists());
        let _ = std::fs::remove_dir_all(&paths.root);
    }
}

/// "Start minimised to the tray", read before the window is made.
pub fn start_in_tray_setting() -> bool {
    read_data(&Paths::find()).and_then(|d| d.get("tray").and_then(|b| b.as_bool())).unwrap_or(false)
}

/// The settings file's "data": from the sealed file, or the plain one an earlier version left
/// (which is sealed now).
fn read_data(paths: &Paths) -> Option<Value> {
    let _ = super::vault::migrate(&paths.legacy_settings_file(), &paths.settings_file(), super::package::Kind::Save);
    let v = super::vault::read_json(&paths.settings_file(), super::package::Kind::Save).ok()?;
    v.get("data").filter(|d| d.is_object()).cloned()
}

/// The port of the phone page (Settings › Phones), 47800 unless settings.json says another.
pub fn port_setting() -> u16 {
    read_data(&Paths::find())
        .and_then(|d| d.get("port").cloned())
        .and_then(|p| p.as_u64().or_else(|| p.as_str().and_then(|s| s.trim().parse().ok())))
        .filter(|p| (1024..65535 - 10).contains(p))
        .map(|p| p as u16)
        .unwrap_or(super::phoneweb::DEFAULT_PORT)
}
