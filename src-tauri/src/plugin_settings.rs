//! A plugin's own settings: the fields its manifest.json lists under "settings", shown in
//! Settings › Plugins and kept in <app data>\plugin-data\<id>\settings.json (apart from the
//! plugin's folder, so reinstalling keeps them). Values the plugin saves itself (Host::set_setting)
//! live in the same file.
//!
//! Password fields are encrypted with Windows (DPAPI, locked to this Windows account) before they
//! are saved, stored as {"$secret":"<base64>"}, and never sent back to the window: it only learns
//! whether one is set. The plugin gets them decrypted.
//!
//! manifest.json:
//!   "settings": [
//!     { "key": "host", "label": "Address", "kind": "text", "def": "localhost", "placeholder": "…", "help": "…", "required": true },
//!     { "key": "port", "label": "Port", "kind": "number", "min": 1, "max": 65535, "def": 4455 },
//!     { "key": "password", "label": "Password", "kind": "password" },
//!     { "key": "auto", "label": "Connect when Minniondeck starts", "kind": "switch", "def": true },
//!     { "key": "quality", "label": "Quality", "kind": "select", "options": ["Low", "High"], "def": "High" },
//!     { "key": "test", "label": "Test the connection", "kind": "button" },
//!     { "key": "token", "label": "Login", "kind": "password", "hidden": true }
//!   ]
//! A "button" isn't saved: pressing it calls the plugin's settings_button(key) and shows its answer.
//! A "hidden" password is the plugin's own (a login it got by signing in): kept and encrypted the
//! same, but not in the form, and only the plugin changes it.

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

const KINDS: [&str; 6] = ["text", "password", "number", "switch", "select", "button"];
const MAX_FIELDS: usize = 40;
const MAX_TEXT: usize = 2000;
const SECRET: &str = "$secret";
const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Field {
    pub key: String,
    pub label: String,
    #[serde(default = "text_kind")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub placeholder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub def: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default)]
    pub required: bool,
    /// A password only the plugin sets (see the top of this file)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}
fn text_kind() -> String {
    "text".into()
}

fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 40 && !k.starts_with('$') && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The settings fields a manifest lists. Fields that can't be shown are left out.
pub fn schema(manifest_text: &str) -> Vec<Field> {
    let Ok(v) = serde_json::from_str::<Value>(manifest_text) else { return Vec::new() };
    let Some(list) = v.get("settings").and_then(|s| s.as_array()) else { return Vec::new() };
    let mut seen = std::collections::HashSet::new();
    list.iter()
        .filter_map(|f| serde_json::from_value::<Field>(f.clone()).ok())
        .filter(|f| valid_key(&f.key) && KINDS.contains(&f.kind.as_str()) && seen.insert(f.key.clone()))
        .filter(|f| f.kind != "select" || !f.options.is_empty())
        .map(|mut f| {
            f.label = f.label.chars().take(60).collect();
            f.help = f.help.chars().take(300).collect();
            f.placeholder = f.placeholder.chars().take(80).collect();
            f.options = f.options.into_iter().take(100).map(|o| o.chars().take(80).collect()).collect();
            // A password's default would sit in plain text in manifest.json; it isn't used
            if f.kind == "password" || f.kind == "button" {
                f.def = None;
            }
            f.hidden &= f.kind == "password";
            f
        })
        .take(MAX_FIELDS)
        .collect()
}

/// The settings fields of an installed plugin
pub fn schema_of(folder: &Path) -> Vec<Field> {
    fs::read_to_string(folder.join("manifest.json")).map(|t| schema(&t)).unwrap_or_default()
}

fn file(data: &Path, id: &str) -> PathBuf {
    data.join(id).join("settings.json")
}

fn stored(data: &Path, id: &str) -> Map<String, Value> {
    fs::read_to_string(file(data, id)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

fn write(data: &Path, id: &str, all: &Map<String, Value>) -> Result<(), String> {
    let path = file(data, id);
    fs::create_dir_all(path.parent().unwrap_or(data)).map_err(|e| e.to_string())?;
    crate::storage::write_atomic(&path, serde_json::to_string_pretty(&Value::Object(all.clone())).unwrap_or_default().as_bytes())
}

fn seal(v: &Value) -> Result<Value, String> {
    let sealed = crate::signing::protect(v.to_string().as_bytes())?;
    Ok(json!({ SECRET: B64.encode(sealed) }))
}

/// A stored value as the plugin should see it (secrets decrypted). None if it can't be read,
/// e.g. a secret saved under another Windows account.
fn open(v: &Value) -> Option<Value> {
    match v.get(SECRET).and_then(|s| s.as_str()) {
        Some(b64) => {
            let bytes = crate::signing::unprotect(&B64.decode(b64).ok()?).ok()?;
            serde_json::from_slice(&bytes).ok()
        }
        None => Some(v.clone()),
    }
}

fn is_sealed(v: &Value) -> bool {
    v.get(SECRET).is_some()
}

/// Everything the plugin gets: saved values (secrets decrypted), and defaults for unset fields
pub fn for_plugin(data: &Path, id: &str, fields: &[Field]) -> Value {
    let mut out = Map::new();
    for (k, v) in stored(data, id) {
        if let Some(v) = open(&v) {
            out.insert(k, v);
        }
    }
    for f in fields {
        if let (false, Some(def)) = (out.contains_key(&f.key), &f.def) {
            out.insert(f.key.clone(), def.clone());
        }
    }
    Value::Object(out)
}

/// What the window may see: the fields, and their values with passwords only marked set or not
pub fn for_window(data: &Path, id: &str, fields: &[Field]) -> Value {
    let all = stored(data, id);
    let fields: Vec<&Field> = fields.iter().filter(|f| !f.hidden).collect();
    let mut values = Map::new();
    for f in &fields {
        let v = all.get(&f.key);
        let shown = match f.kind.as_str() {
            "button" => continue,
            "password" => json!({ "set": v.is_some_and(|v| !v.is_null()) }),
            _ => v.filter(|v| !is_sealed(v)).cloned().or_else(|| f.def.clone()).unwrap_or(Value::Null),
        };
        values.insert(f.key.clone(), shown);
    }
    json!({ "fields": fields, "values": values })
}

/// Check and save what the user changed. A key that isn't in `changes` keeps its value;
/// null clears it (a password is removed, other fields go back to their default).
pub fn save(data: &Path, id: &str, fields: &[Field], changes: &Map<String, Value>) -> Result<(), String> {
    let mut all = stored(data, id);
    for f in fields.iter().filter(|f| f.kind != "button" && !f.hidden) {
        let Some(v) = changes.get(&f.key) else { continue };
        if v.is_null() {
            all.remove(&f.key);
            continue;
        }
        let label = &f.label;
        let value = match f.kind.as_str() {
            "password" => {
                let s = v.as_str().ok_or_else(|| format!("{label} must be text."))?;
                if s.is_empty() {
                    continue;
                }
                if s.chars().count() > MAX_TEXT {
                    return Err(format!("{label} is too long."));
                }
                seal(&json!(s))?
            }
            "number" => {
                let n = v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok())).ok_or_else(|| format!("{label} must be a number."))?;
                if !n.is_finite() || f.min.is_some_and(|m| n < m) || f.max.is_some_and(|m| n > m) {
                    let range = match (f.min, f.max) {
                        (Some(a), Some(b)) => format!(" from {a} to {b}"),
                        (Some(a), None) => format!(" of at least {a}"),
                        (None, Some(b)) => format!(" of at most {b}"),
                        _ => String::new(),
                    };
                    return Err(format!("{label} must be a number{range}."));
                }
                if n.fract() == 0.0 && n.abs() < 9.0e15 { json!(n as i64) } else { json!(n) }
            }
            "switch" => json!(v.as_bool().ok_or_else(|| format!("{label} must be on or off."))?),
            "select" => {
                let s = v.as_str().unwrap_or("");
                if !f.options.iter().any(|o| o == s) {
                    return Err(format!("Choose one of the {label} options."));
                }
                json!(s)
            }
            _ => {
                let s = v.as_str().ok_or_else(|| format!("{label} must be text."))?;
                if s.chars().count() > MAX_TEXT {
                    return Err(format!("{label} is too long."));
                }
                json!(s)
            }
        };
        all.insert(f.key.clone(), value);
    }
    // Required fields must end up with something
    for f in fields.iter().filter(|f| f.required && f.kind != "button") {
        let empty = match all.get(&f.key) {
            None => f.def.as_ref().is_none_or(|d| d.is_null() || d.as_str() == Some("")),
            Some(v) => v.is_null() || v.as_str() == Some(""),
        };
        if empty {
            return Err(format!("{} is needed.", f.label));
        }
    }
    write(data, id, &all)
}

/// The plugin saved a value itself (Host::set_setting); a password field is encrypted
pub fn set_from_plugin(data: &Path, id: &str, fields: &[Field], key: &str, value: Value) -> Result<(), String> {
    if !valid_key(key) {
        return Err("Setting names may use letters, digits, - and _.".into());
    }
    let mut all = stored(data, id);
    let secret = fields.iter().any(|f| f.key == key && f.kind == "password");
    if value.is_null() {
        all.remove(key);
    } else {
        all.insert(key.to_string(), if secret { seal(&value)? } else { value });
    }
    write(data, id, &all)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"{ "settings": [
        { "key": "host", "label": "Address", "def": "localhost", "required": true },
        { "key": "port", "label": "Port", "kind": "number", "min": 1, "max": 65535, "def": 4455 },
        { "key": "password", "label": "Password", "kind": "password", "def": "leaked" },
        { "key": "auto", "label": "Auto", "kind": "switch", "def": true },
        { "key": "quality", "label": "Quality", "kind": "select", "options": ["Low", "High"] },
        { "key": "broken", "label": "No options", "kind": "select" },
        { "key": "$bad", "label": "Bad key" },
        { "key": "test", "label": "Test", "kind": "button" },
        { "key": "token", "label": "Login", "kind": "password", "hidden": true },
        { "key": "shy", "label": "Not a password", "hidden": true }
    ] }"#;

    #[test]
    fn fields_values_and_secrets() {
        let data = std::env::temp_dir().join(format!("md-settings-test-{}", crate::storage::now_ms()));
        let fields = schema(MANIFEST);
        assert_eq!(fields.iter().map(|f| f.key.as_str()).collect::<Vec<_>>(), ["host", "port", "password", "auto", "quality", "test", "token", "shy"]);
        assert!(fields[6].hidden && !fields[7].hidden, "only a password can be hidden");
        assert!(fields[2].def.is_none(), "a password never has a default");

        // Defaults before anything is saved
        let p = for_plugin(&data, "obs", &fields);
        assert_eq!(p["host"], "localhost");
        assert_eq!(p["port"], 4455);
        assert!(p.get("password").is_none());

        let changes = |v: Value| v.as_object().unwrap().clone();
        save(&data, "obs", &fields, &changes(json!({ "port": "4456", "password": "s3cret", "quality": "High", "auto": false }))).unwrap();
        // The secret is encrypted on disk and never shown to the window
        let raw = fs::read_to_string(file(&data, "obs")).unwrap();
        assert!(!raw.contains("s3cret") && raw.contains(SECRET));
        let w = for_window(&data, "obs", &fields);
        assert_eq!(w["values"]["password"], json!({ "set": true }));
        assert_eq!(w["values"]["port"], 4456);
        assert!(w["values"].get("test").is_none());
        // The plugin gets it decrypted
        let p = for_plugin(&data, "obs", &fields);
        assert_eq!(p["password"], "s3cret");
        assert_eq!(p["auto"], false);

        // An empty password keeps the saved one; null clears it
        save(&data, "obs", &fields, &changes(json!({ "password": "" }))).unwrap();
        assert_eq!(for_plugin(&data, "obs", &fields)["password"], "s3cret");
        save(&data, "obs", &fields, &changes(json!({ "password": null }))).unwrap();
        assert!(for_plugin(&data, "obs", &fields).get("password").is_none());

        // Wrong values are refused with the field's name
        assert!(save(&data, "obs", &fields, &changes(json!({ "port": 70000 }))).unwrap_err().contains("Port"));
        assert!(save(&data, "obs", &fields, &changes(json!({ "quality": "Ultra" }))).unwrap_err().contains("Quality"));
        assert!(save(&data, "obs", &fields, &changes(json!({ "host": "" }))).unwrap_err().contains("Address is needed"));

        // A plugin saving into a password field is encrypted too
        set_from_plugin(&data, "obs", &fields, "password", json!("token2")).unwrap();
        assert!(!fs::read_to_string(file(&data, "obs")).unwrap().contains("token2"));
        assert_eq!(for_plugin(&data, "obs", &fields)["password"], "token2");

        // A hidden login: the plugin sets it, encrypted; the window neither sees nor changes it
        set_from_plugin(&data, "obs", &fields, "token", json!("refresh-1")).unwrap();
        assert!(!fs::read_to_string(file(&data, "obs")).unwrap().contains("refresh-1"));
        let w = for_window(&data, "obs", &fields);
        assert!(w["values"].get("token").is_none());
        assert!(w["fields"].as_array().unwrap().iter().all(|f| f["key"] != "token"));
        save(&data, "obs", &fields, &changes(json!({ "token": null }))).unwrap();
        assert_eq!(for_plugin(&data, "obs", &fields)["token"], "refresh-1");
        let _ = fs::remove_dir_all(&data);
    }
}
