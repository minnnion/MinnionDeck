//! Plugins: folders in <app data>\plugins\<id>\ holding manifest.json, actions.json, icons and a DLL.
//!
//! Every plugin runs in the plugin host (pluginhost.rs), a helper copy of Minniondeck.exe that
//! this module starts and watches. If the host crashes, plugins show as starting, the host is
//! started again and the plugins loaded again; after 3 crashes in a minute it stays stopped until
//! the user reloads plugins in Settings.
//!
//! The host loads a copy of each DLL (in <app data>\plugin-run\), so a plugin's own DLL is never
//! locked and can be rebuilt while Minniondeck runs; reloading picks up the new build at once.
//!
//! Button kinds for plugin actions are "plug:<id>:<action>". A plugin's own settings are kept in
//! <app data>\plugin-data\<id>\settings.json, apart from its folder, so reinstalling keeps them.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};

use crate::pluginhost::API_VERSION;
use crate::signing::{self, AuthorKey, Trust};

/// Largest icon file read for the editor or a button
const MAX_ICON_BYTES: u64 = 512 * 1024;
/// How long a press may take before the button reports that the plugin didn't answer
const PRESS_TIMEOUT: Duration = Duration::from_secs(15);
const CRASHES_ALLOWED: usize = 3;

static SUP: OnceLock<Arc<Supervisor>> = OnceLock::new();

/// The button an event is about
#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct ButtonCtx {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub state: usize,
}

#[derive(Deserialize)]
pub(crate) struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub api: u32,
    pub dll: String,
    #[serde(default = "default_actions")]
    pub actions: String,
}
fn default_actions() -> String {
    "actions.json".into()
}

#[derive(Deserialize)]
struct ActionDef {
    id: String,
    label: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    fields: Value,
    /// Named button states the plugin switches between, e.g. ["Offline", "Live"]
    #[serde(default)]
    states: Vec<String>,
    /// A knob: turned with the mouse wheel on the PC or a drag on a phone ("turn" events), as
    /// well as pressed
    #[serde(default)]
    knob: bool,
}

#[derive(Serialize)]
pub struct PluginAction {
    id: String,
    label: String,
    group: String,
    /// The icon as a data: URL, or empty when the plugin has none
    icon: String,
    /// Where the icon is inside the plugin folder, shown in the editor
    #[serde(rename = "iconFile")]
    icon_file: String,
    fields: Value,
    states: Vec<String>,
    knob: bool,
}

#[derive(Serialize)]
pub struct PluginInfo {
    id: String,
    name: String,
    version: String,
    actions: Vec<PluginAction>,
    /// "ready", "waking" (starting), "dizzy" (stopped) or "off" (turned off)
    state: String,
    /// Why this plugin can't be used, when it can't
    error: Option<String>,
    /// Who signed it and whether this PC trusts them
    signature: Trust,
    /// Whether its manifest lists settings for Settings › Plugins
    #[serde(rename = "hasSettings")]
    has_settings: bool,
    /// Its icons (manifest "icons"), for the icon picker
    icons: Vec<PluginMedia>,
    /// Its GIFs (manifest "gifs"), for the GIF lists
    gifs: Vec<PluginMedia>,
}

/// A picture that comes with a plugin
#[derive(Serialize)]
pub struct PluginMedia {
    /// Where it is in the plugin folder, e.g. "icons/live.png"
    file: String,
    /// Its file name without the extension
    name: String,
    /// The picture as a data: URL
    url: String,
}

/// Largest GIF a plugin can offer
const MAX_GIF_BYTES: u64 = 2 * 1024 * 1024;

/// The files a manifest lists under `key` ("icons" or "gifs") that are there and small enough
fn manifest_media(folder: &Path, key: &str) -> Vec<PluginMedia> {
    let Ok(text) = fs::read_to_string(folder.join("manifest.json")) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    let mut seen = std::collections::HashSet::new();
    v.get(key)
        .and_then(|l| l.as_array())
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str())
        .filter(|f| safe_rel(f) && seen.insert(f.to_string()))
        .take(200)
        .filter_map(|f| {
            let url = if key == "gifs" { data_url(folder, f, MAX_GIF_BYTES) } else { icon_url(folder, f) };
            let name = Path::new(f).file_stem().and_then(|s| s.to_str()).unwrap_or(f).replace(['_', '-'], " ");
            (!url.is_empty()).then(|| PluginMedia { file: f.replace('\\', "/"), name, url })
        })
        .collect()
}

/// What the user decided about plugins, in <app data>\plugins.json
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginConfig {
    /// Run plugins that aren't signed by a trusted author
    #[serde(default)]
    pub dev_mode: bool,
    /// Plugins turned off
    #[serde(default)]
    pub disabled: Vec<String>,
    /// Authors the user trusts, besides the built-in ones
    #[serde(default)]
    pub trusted: Vec<AuthorKey>,
}

pub(crate) fn read_config(root: &Path) -> PluginConfig {
    fs::read_to_string(root.join("plugins.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn write_config(root: &Path, cfg: &PluginConfig) -> Result<(), String> {
    crate::storage::write_atomic(&root.join("plugins.json"), serde_json::to_string_pretty(cfg).unwrap_or_default().as_bytes())
}

/// Whether a plugin may run, and why not
pub(crate) fn may_run(trust: &Trust, cfg: &PluginConfig) -> Result<(), String> {
    match trust {
        Trust::Bad { why } => Err(format!("{why} It won't run. Install it again from its author.")),
        t if t.trusted() || cfg.dev_mode => Ok(()),
        Trust::Untrusted { author } => Err(format!("Signed by {author}, who this PC doesn't trust. Add their key under Trusted authors, or turn on Developer mode.")),
        _ => Err("Not signed. It runs only with Developer mode on.".into()),
    }
}

#[derive(Clone)]
struct Status {
    state: &'static str,
    error: Option<String>,
}

struct HostConn {
    generation: u64,
    stdin: Option<ChildStdin>,
    child: Option<Child>,
}

pub struct Supervisor {
    app: AppHandle,
    /// The app data folder (plugins.json is here)
    root: PathBuf,
    dir: PathBuf,
    data: PathBuf,
    run: PathBuf,
    host: Mutex<HostConn>,
    pending: Mutex<HashMap<u64, mpsc::Sender<Value>>>,
    next_req: AtomicU64,
    status: Mutex<HashMap<String, Status>>,
    /// The DLL copy each plugin runs from, removed once a newer copy replaces it
    shadows: Mutex<HashMap<String, PathBuf>>,
    crashes: Mutex<Vec<Instant>>,
    quitting: AtomicBool,
    /// Loading or reloading, one at a time
    loading: Mutex<()>,
    /// Starting the host, one at a time (start-up and a reload can race)
    spawning: Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ids and file names come from plugin files, so they may only name things inside the plugin's folder
pub(crate) fn safe_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}
pub(crate) fn safe_rel(s: &str) -> bool {
    !s.is_empty() && s.split(['/', '\\']).all(safe_name)
}

/// A plugin's manifest.json, checked: its files stay inside its folder and it speaks this interface
pub(crate) fn check_manifest(text: &str) -> Result<Manifest, String> {
    let m: Manifest = serde_json::from_str(text).map_err(|e| format!("manifest.json is not valid: {e}"))?;
    if !safe_name(&m.id) || !safe_rel(&m.dll) || !safe_rel(&m.actions) {
        return Err("manifest.json names a file outside the plugin folder.".into());
    }
    if m.api != API_VERSION {
        return Err(format!("Built for plugin interface {}, but Minniondeck uses {API_VERSION}. Rebuild it with the current plugin kit.", m.api));
    }
    Ok(m)
}

fn read_manifest(folder: &Path) -> Result<Manifest, String> {
    let text = fs::read_to_string(folder.join("manifest.json")).map_err(|_| "manifest.json is missing.".to_string())?;
    let m = check_manifest(&text)?;
    if folder.file_name().and_then(|n| n.to_str()) != Some(m.id.as_str()) {
        return Err(format!("The folder must be named \"{}\", like the id in manifest.json.", m.id));
    }
    Ok(m)
}

fn icon_url(folder: &Path, rel: &str) -> String {
    data_url(folder, rel, MAX_ICON_BYTES)
}

/// A picture in a plugin folder as a data: URL, or "" if it isn't a picture or is too big
fn data_url(folder: &Path, rel: &str, max: u64) -> String {
    if !safe_rel(rel) {
        return String::new();
    }
    let path = folder.join(rel);
    let mime = match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => return String::new(),
    };
    match fs::metadata(&path) {
        Ok(m) if m.len() <= max => {}
        _ => return String::new(),
    }
    fs::read(&path).map(|b| format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(b))).unwrap_or_default()
}

/// A plugin folder as the UI sees it (state is filled in by the caller)
fn describe(folder: &Path) -> PluginInfo {
    let fallback = folder.file_name().and_then(|n| n.to_str()).unwrap_or("plugin").to_string();
    let bad = |e: String| PluginInfo { id: fallback.clone(), name: fallback.clone(), version: String::new(), actions: vec![], state: "dizzy".into(), error: Some(e), signature: Trust::Unsigned, has_settings: false, icons: vec![], gifs: vec![] };
    let m = match read_manifest(folder) {
        Ok(m) => m,
        Err(e) => return bad(e),
    };
    let mut info = PluginInfo { id: m.id.clone(), name: m.name.clone(), version: m.version.clone(), actions: vec![], state: "waking".into(), error: None, signature: Trust::Unsigned, has_settings: !crate::plugin_settings::schema_of(folder).is_empty(), icons: manifest_media(folder, "icons"), gifs: manifest_media(folder, "gifs") };
    if !folder.join(&m.dll).is_file() {
        info.state = "dizzy".into();
        info.error = Some(format!("{} is missing.", m.dll));
        return info;
    }
    let defs: Vec<ActionDef> = match fs::read_to_string(folder.join(&m.actions)).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string())) {
        Ok(d) => d,
        Err(e) => {
            info.state = "dizzy".into();
            info.error = Some(format!("{} is not valid: {e}", m.actions));
            return info;
        }
    };
    info.actions = defs
        .into_iter()
        .filter(|a| safe_name(&a.id))
        .map(|a| PluginAction {
            icon: icon_url(folder, &a.icon),
            icon_file: a.icon.clone(),
            group: if a.group.is_empty() { m.name.clone() } else { a.group },
            id: a.id,
            label: a.label,
            fields: if a.fields.is_array() { a.fields } else { Value::Array(vec![]) },
            states: a.states.into_iter().take(8).map(|s| s.chars().take(24).collect()).collect(),
            knob: a.knob,
        })
        .collect();
    info
}

/// "plug:<id>:<action>" → (id, action)
fn split_kind(kind: &str) -> Option<(&str, &str)> {
    let mut parts = kind.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("plug"), Some(id), Some(a)) if safe_name(id) && safe_name(a) => Some((id, a)),
        _ => None,
    }
}

impl Supervisor {
    fn emit_changed(&self) {
        let _ = self.app.emit("plugins_changed", ());
    }

    fn set_status(&self, id: &str, state: &'static str, error: Option<String>) {
        lock(&self.status).insert(id.to_string(), Status { state, error });
    }

    /// The settings a plugin gets: saved values with secrets decrypted, and defaults
    fn read_settings(&self, id: &str) -> Value {
        crate::plugin_settings::for_plugin(&self.data, id, &crate::plugin_settings::schema_of(&self.dir.join(id)))
    }

    fn write_line(&self, msg: &Value) -> Result<(), String> {
        let mut h = lock(&self.host);
        let stdin = h.stdin.as_mut().ok_or("Plugins are restarting. Try again in a moment.")?;
        writeln!(stdin, "{msg}").and_then(|_| stdin.flush()).map_err(|_| "Plugins are restarting. Try again in a moment.".to_string())
    }

    /// Send a message to the host and wait for its reply
    fn request(&self, mut msg: Value, timeout: Duration) -> Result<Value, String> {
        let req = self.next_req.fetch_add(1, Ordering::Relaxed) + 1;
        msg["req"] = json!(req);
        let (tx, rx) = mpsc::channel();
        lock(&self.pending).insert(req, tx);
        if let Err(e) = self.write_line(&msg) {
            lock(&self.pending).remove(&req);
            return Err(e);
        }
        match rx.recv_timeout(timeout) {
            Ok(v) => Ok(v),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                lock(&self.pending).remove(&req);
                Err("The plugin took too long to answer.".into())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("Plugins restarted while this was running.".into()),
        }
    }

    /// Start the host process and read what it says
    fn spawn_host(self: &Arc<Self>) -> Result<(), String> {
        let _one_at_a_time = lock(&self.spawning);
        if lock(&self.host).stdin.is_some() {
            return Ok(());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut cmd = Command::new(exe);
        cmd.arg(crate::pluginhost::ARG).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| format!("Could not start the plugin host: {e}"))?;
        let stdout = child.stdout.take().ok_or("The plugin host has no output.")?;
        let generation = {
            let mut h = lock(&self.host);
            h.generation += 1;
            h.stdin = child.stdin.take();
            h.child = Some(child);
            h.generation
        };
        let sup = self.clone();
        std::thread::Builder::new()
            .name("plugin host reader".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(msg) = serde_json::from_str::<Value>(&line) {
                        sup.handle(msg);
                    }
                }
                sup.host_ended(generation);
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn handle(&self, msg: Value) {
        match msg.get("t").and_then(|v| v.as_str()) {
            Some("reply") => {
                let req = msg.get("req").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(tx) = lock(&self.pending).remove(&req) {
                    let _ = tx.send(msg.get("body").cloned().unwrap_or_default());
                }
            }
            Some("plugin") => {
                let id = msg.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if safe_name(&id) {
                    self.from_plugin(&id, msg.get("msg").cloned().unwrap_or_default());
                }
            }
            _ => {}
        }
    }

    /// Something a plugin sent through its Host
    fn from_plugin(&self, id: &str, msg: Value) {
        let s = |k: &str| msg.get(k).and_then(|v| v.as_str()).map(|t| t.to_string());
        let clip = |t: String, n: usize| t.chars().take(n).collect::<String>();
        let button = s("button").unwrap_or_default();
        match s("t").as_deref() {
            Some("set_state") => {
                let state = msg.get("state").and_then(|v| v.as_u64()).unwrap_or(0);
                let _ = self.app.emit("plugin_button", json!({ "plugin": id, "button": button, "state": state }));
                if let Some(hub) = self.app.try_state::<std::sync::Arc<crate::phones::hub::Hub>>() {
                    hub.set_live_state(&button, state);
                }
            }
            Some("set_title") => {
                let title = s("title").map(|t| clip(t, 40));
                let _ = self.app.emit("plugin_button", json!({ "plugin": id, "button": button, "title": title }));
                if let Some(hub) = self.app.try_state::<Arc<crate::phones::hub::Hub>>() {
                    hub.set_live_title(&button, title);
                }
            }
            Some("set_icon") => {
                let icon = s("icon").map(|rel| icon_url(&self.dir.join(id), &rel)).filter(|u| !u.is_empty());
                let _ = self.app.emit("plugin_button", json!({ "plugin": id, "button": button, "icon": icon }));
                if let Some(hub) = self.app.try_state::<Arc<crate::phones::hub::Hub>>() {
                    hub.set_live_icon(&button, icon);
                }
            }
            // Live data a button draws itself (a map): an object whose keys replace the ones the
            // button had, or null to clear it. Kept to 256 KB, so a plugin can't flood the app.
            Some("set_data") => {
                let data = msg.get("data").cloned().unwrap_or(Value::Null);
                if !(data.is_object() || data.is_null()) || data.to_string().len() > 256 * 1024 {
                    crate::applog::line(&format!("Plugin {id}: set_data needs a JSON object under 256 KB"));
                    return;
                }
                let _ = self.app.emit("plugin_button", json!({ "plugin": id, "button": button, "data": data }));
                if let Some(hub) = self.app.try_state::<Arc<crate::phones::hub::Hub>>() {
                    hub.set_live_data(&button, data);
                }
            }
            Some("log") => crate::applog::line(&format!("Plugin {id}: {}", clip(s("text").unwrap_or_default(), 500))),
            Some("toast") => {
                let _ = self.app.emit("plugin_toast", json!({ "plugin": id, "text": clip(s("text").unwrap_or_default(), 200) }));
            }
            Some("set_setting") => {
                if let Some(key) = s("key") {
                    let fields = crate::plugin_settings::schema_of(&self.dir.join(id));
                    let value = msg.get("value").cloned().unwrap_or(Value::Null);
                    if let Err(e) = crate::plugin_settings::set_from_plugin(&self.data, id, &fields, &key, value) {
                        crate::applog::line(&format!("Plugin {id}: could not save the setting {key}: {e}"));
                    }
                }
            }
            Some("refresh_options") => {
                let _ = self.app.emit("plugin_options_changed", json!({ "plugin": id, "action": s("action") }));
            }
            // A plugin asks the app to say something; the app runs the speech model (speech.rs)
            Some("speak") => {
                if let Err(e) = crate::speech::say(&s("text").unwrap_or_default(), &s("voice").unwrap_or_default(), id) {
                    crate::applog::line(&format!("Plugin {id}: could not speak: {e}"));
                    let _ = self.app.emit("plugin_toast", json!({ "plugin": id, "text": clip(e, 200) }));
                }
            }
            _ => {}
        }
    }

    /// The host process ended: restart it, unless it keeps crashing or the app is closing
    fn host_ended(self: &Arc<Self>, generation: u64) {
        {
            let mut h = lock(&self.host);
            if h.generation != generation {
                return;
            }
            h.stdin = None;
            if let Some(mut c) = h.child.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
        // Anyone waiting for a reply gets "plugins restarted"
        lock(&self.pending).clear();
        if self.quitting.load(Ordering::Relaxed) {
            return;
        }
        let too_many = {
            let mut c = lock(&self.crashes);
            c.push(Instant::now());
            c.retain(|t| t.elapsed() < Duration::from_secs(60));
            c.len() >= CRASHES_ALLOWED
        };
        let ids: Vec<String> = lock(&self.status).keys().cloned().collect();
        if too_many {
            crate::applog::line("The plugin host crashed 3 times in a minute; plugins stay stopped until they are reloaded.");
            for id in &ids {
                self.set_status(id, "dizzy", Some("Plugins stopped after crashing 3 times in a minute. Use Reload plugins to try again.".into()));
            }
            self.emit_changed();
            return;
        }
        crate::applog::line("The plugin host stopped; starting it again.");
        for id in &ids {
            if lock(&self.status).get(id).is_some_and(|s| s.state == "ready") {
                self.set_status(id, "waking", None);
            }
        }
        self.emit_changed();
        let sup = self.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(1));
            sup.start_and_load();
        });
    }

    fn start_and_load(self: &Arc<Self>) {
        if lock(&self.host).stdin.is_none() {
            if let Err(e) = self.spawn_host() {
                crate::applog::line(&e);
                let ids: Vec<String> = lock(&self.status).keys().cloned().collect();
                for id in ids {
                    self.set_status(&id, "dizzy", Some(e.clone()));
                }
                self.emit_changed();
                return;
            }
        }
        self.load_all();
    }

    fn folders(&self) -> Vec<PathBuf> {
        plugin_folders(&self.dir)
    }

    /// Load (or reload) every plugin in the plugins folder
    fn load_all(self: &Arc<Self>) {
        let _one_at_a_time = lock(&self.loading);
        let folders = self.folders();
        let present: Vec<String> = folders.iter().filter_map(|f| f.file_name().and_then(|n| n.to_str()).map(String::from)).collect();
        lock(&self.status).retain(|id, _| present.contains(id));
        for folder in folders {
            let info = describe(&folder);
            if let Some(e) = info.error {
                self.set_status(&info.id, "dizzy", Some(e));
                continue;
            }
            self.set_status(&info.id, "waking", None);
            self.emit_changed();
            self.load_one(&info.id);
        }
        self.emit_changed();
    }

    fn load_one(&self, id: &str) {
        let folder = self.dir.join(id);
        let cfg = read_config(&self.root);
        // Turned off, or not allowed to run: make sure it isn't running
        let gate = if cfg.disabled.iter().any(|d| d == id) {
            Some(("off", None))
        } else {
            may_run(&signing::verify_folder(&folder, &cfg.trusted), &cfg).err().map(|e| ("dizzy", Some(e)))
        };
        if let Some((state, error)) = gate {
            let _ = self.request(json!({ "t": "unload", "id": id }), Duration::from_secs(5));
            if let Some(e) = &error {
                crate::applog::line(&format!("Plugin {id} not started: {e}"));
            }
            self.set_status(id, state, error);
            return;
        }
        let result = (|| -> Result<(), String> {
            let m = read_manifest(&folder)?;
            let dll = folder.join(&m.dll);
            fs::create_dir_all(&self.run).map_err(|e| e.to_string())?;
            let shadow = self.run.join(format!("{id}-{}.dll", crate::storage::now_ms()));
            fs::copy(&dll, &shadow).map_err(|e| format!("Could not copy {}: {e}", m.dll))?;
            let body = self.request(
                json!({ "t": "load", "id": id, "dll": shadow.to_string_lossy(), "dir": folder.to_string_lossy(), "settings": self.read_settings(id) }),
                Duration::from_secs(20),
            );
            let old = lock(&self.shadows).insert(id.to_string(), shadow.clone());
            if let Some(old) = old.filter(|o| *o != shadow) {
                let _ = fs::remove_file(old);
            }
            let body = body?;
            if body.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                Ok(())
            } else {
                Err(body.get("error").and_then(|v| v.as_str()).unwrap_or("The plugin didn't start.").to_string())
            }
        })();
        match result {
            Ok(()) => {
                self.set_status(id, "ready", None);
                crate::applog::line(&format!("Plugin {id} started"));
            }
            Err(e) => {
                crate::applog::line(&format!("Plugin {id} didn't start: {e}"));
                self.set_status(id, "dizzy", Some(e));
            }
        }
    }

    fn state_of(&self, id: &str) -> Option<Status> {
        lock(&self.status).get(id).cloned()
    }

    /// Send an event to a plugin. `wait` returns the plugin's answer; otherwise it is sent and forgotten.
    fn event(&self, kind: &str, event: &str, fields: &Value, button: &ButtonCtx, wait: bool, data: Option<&Value>) -> Result<String, String> {
        let (id, action) = split_kind(kind).ok_or_else(|| format!("\"{kind}\" is not a plugin action."))?;
        match self.state_of(id) {
            Some(s) if s.state == "ready" => {}
            Some(s) if s.state == "waking" => return Err(format!("Plugin \"{id}\" is still starting.")),
            Some(s) if s.state == "off" => return Err(format!("Plugin \"{id}\" is turned off in Settings › Plugins.")),
            Some(s) => return Err(s.error.unwrap_or_else(|| format!("Plugin \"{id}\" is stopped."))),
            None => return Err(format!("Plugin \"{id}\" is not installed.")),
        }
        let mut msg = json!({ "t": "call", "id": id, "msg": { "t": event, "action": action, "button": button.id, "state": button.state, "fields": fields } });
        // Only small objects go with an event
        if let Some(d) = data.filter(|d| d.is_object() && d.to_string().len() <= 4096) {
            msg["msg"]["data"] = d.clone();
        }
        if !wait {
            self.write_line(&msg)?;
            return Ok(String::new());
        }
        let v = self.request(msg, PRESS_TIMEOUT)?;
        let message = v.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
        if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false) {
            Ok(if message.is_empty() { "Done".into() } else { message })
        } else {
            Err(if message.is_empty() { format!("Plugin \"{id}\" failed.") } else { message })
        }
    }

    fn shutdown(&self) {
        self.quitting.store(true, Ordering::Relaxed);
        let _ = self.write_line(&json!({ "t": "quit" }));
        let child = lock(&self.host).child.take();
        if let Some(mut c) = child {
            let until = Instant::now() + Duration::from_secs(3);
            while Instant::now() < until {
                if let Ok(Some(_)) = c.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Plugin folders; names starting with "." are installs in progress
fn plugin_folders(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && !p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.'))).collect())
        .unwrap_or_default()
}

/// The argument `Minniondeck.exe plugin install` starts the app with, so the running copy
/// (which gets it through the single-instance hand-over) loads the plugin at once
pub const RELOAD_ARG: &str = "--reload-plugin";

/// Called once at start with the app data folder: starts the plugin host and loads plugins
pub fn init(app: &AppHandle, root: &Path) {
    let dir = root.join("plugins");
    let data = root.join("plugin-data");
    let run = root.join("plugin-run");
    for d in [&dir, &data, &run] {
        let _ = fs::create_dir_all(d);
    }
    // Installs that stopped half-way
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with('.')) {
            let _ = fs::remove_dir_all(e.path());
        }
    }
    // DLL copies from last time; any still in use stay until next start
    if let Ok(rd) = fs::read_dir(&run) {
        for e in rd.flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
    let sup = Arc::new(Supervisor {
        app: app.clone(),
        root: root.to_path_buf(),
        dir,
        data,
        run,
        host: Mutex::new(HostConn { generation: 0, stdin: None, child: None }),
        pending: Mutex::new(HashMap::new()),
        next_req: AtomicU64::new(0),
        status: Mutex::new(HashMap::new()),
        shadows: Mutex::new(HashMap::new()),
        crashes: Mutex::new(Vec::new()),
        quitting: AtomicBool::new(false),
        loading: Mutex::new(()),
        spawning: Mutex::new(()),
    });
    if SUP.set(sup.clone()).is_err() {
        return;
    }
    // Every plugin starts as "starting", so the UI knows about them before the host answers
    for folder in sup.folders() {
        let info = describe(&folder);
        sup.set_status(&info.id, if info.error.is_some() { "dizzy" } else { "waking" }, info.error);
    }
    std::thread::spawn(move || sup.start_and_load());
}

/// Stop the plugin host (the app is closing)
pub fn shutdown() {
    if let Some(sup) = SUP.get() {
        sup.shutdown();
    }
}

fn sup() -> Result<&'static Arc<Supervisor>, String> {
    SUP.get().ok_or_else(|| "Plugins aren't ready yet.".into())
}

/// The app data folder that holds plugins\ (for downloads from the marketplace, see market.rs)
pub(crate) fn data_root() -> Result<PathBuf, String> {
    Ok(sup()?.root.clone())
}

/// A button with a plugin action was pressed (called from actions.rs)
pub fn press(kind: &str, fields: &Value, button: &ButtonCtx) -> Result<String, String> {
    sup()?.event(kind, "press", fields, button, true, None)
}

/// Installed plugins, their actions and whether each is running
#[tauri::command]
pub fn list_plugins() -> Result<Vec<PluginInfo>, String> {
    let sup = sup()?;
    let cfg = read_config(&sup.root);
    let mut out: Vec<PluginInfo> = sup
        .folders()
        .iter()
        .map(|f| {
            let mut info = describe(f);
            info.signature = signing::verify_folder(f, &cfg.trusted);
            if info.error.is_none() {
                match sup.state_of(&info.id) {
                    Some(s) => {
                        info.state = s.state.into();
                        info.error = s.error;
                    }
                    None => info.state = "waking".into(),
                }
            }
            info
        })
        .collect();
    out.sort_by_key(|p| p.name.to_lowercase());
    Ok(out)
}

/// Load plugins again: one (`id`) or all. Picks up new and rebuilt plugins without a restart.
#[tauri::command]
pub async fn reload_plugins(id: Option<String>) -> Result<Vec<PluginInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || reload_now(id)).await.map_err(|e| e.to_string())??;
    list_plugins()
}

/// A plugin was just installed from the command line: load it and say so in the window
pub fn installed(id: String) {
    std::thread::spawn(move || {
        let result = reload_now(Some(id.clone()));
        if let Ok(sup) = sup() {
            let name = sup.state_of(&id).map(|s| if s.state == "ready" { "Installed and running".to_string() } else { format!("Installed, but it didn't start: {}", s.error.unwrap_or_default()) });
            let text = match result {
                Ok(()) => name.unwrap_or_else(|| "Installed".into()),
                Err(e) => format!("Installed, but it didn't load: {e}"),
            };
            let _ = sup.app.emit("plugin_toast", json!({ "plugin": id, "text": text }));
        }
    });
}

fn reload_now(id: Option<String>) -> Result<(), String> {
    // The speech model lives in minnion_speech's folder: read it again after a reload or install
    if id.as_deref().is_none_or(|i| i == crate::speech::PLUGIN_ID) {
        crate::speech::forget();
    }
    let sup = sup()?.clone();
    {
        lock(&sup.crashes).clear();
        if lock(&sup.host).stdin.is_none() {
            if let Err(e) = sup.spawn_host() {
                return Err(e);
            }
        }
        match id.filter(|i| safe_name(i)) {
            Some(id) => {
                let _one_at_a_time = lock(&sup.loading);
                if sup.dir.join(&id).is_dir() {
                    sup.set_status(&id, "waking", None);
                    sup.emit_changed();
                    sup.load_one(&id);
                } else {
                    let _ = sup.request(json!({ "t": "unload", "id": id }), Duration::from_secs(5));
                    lock(&sup.status).remove(&id);
                }
                sup.emit_changed();
            }
            None => sup.load_all(),
        }
        Ok(())
    }
}

/// A button event other than a press: "release", "long_press", "shown", "hidden", "view" or
/// "turn", for each plugin step of the button. A long press waits for and returns the result,
/// like a press. "view" says what a live map on the button is looking at (`data`, a small object,
/// such as {"airport":"KATL"}), so the plugin can send what's there; "turn" turns a knob button
/// (`data` {"by": clicks, + for clockwise}).
#[tauri::command]
pub async fn plugin_event(event: String, button: ButtonCtx, steps: Vec<crate::actions::Step>, data: Option<Value>) -> Result<String, String> {
    if !matches!(event.as_str(), "release" | "long_press" | "shown" | "hidden" | "view" | "turn") {
        return Err("Unknown button event.".into());
    }
    let sup = sup()?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let wait = event == "long_press";
        let mut last = String::new();
        for s in steps.iter().filter(|s| s.kind.starts_with("plug:")) {
            match sup.event(&s.kind, &event, &s.fields, &button, wait, data.as_ref()) {
                Ok(m) => last = m,
                Err(e) if wait => return Err(e),
                Err(_) => {}
            }
        }
        Ok(last)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Choices for a plugin action's dynamic dropdown
#[tauri::command]
pub async fn plugin_options(kind: String, field: String, fields: Value) -> Result<Vec<Value>, String> {
    let sup = sup()?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (id, action) = split_kind(&kind).ok_or("Not a plugin action.")?;
        if sup.state_of(id).is_none_or(|s| s.state != "ready") {
            return Err(format!("Plugin \"{id}\" isn't running."));
        }
        let v = sup.request(json!({ "t": "call", "id": id, "msg": { "t": "options", "action": action, "field": field, "fields": fields } }), Duration::from_secs(5))?;
        Ok(v.get("options")
            .and_then(|o| o.as_array())
            .map(|list| {
                list.iter()
                    .take(500)
                    .filter_map(|c| {
                        let value = c.get("value").and_then(|v| v.as_str())?.chars().take(200).collect::<String>();
                        let label = c.get("label").and_then(|v| v.as_str()).unwrap_or(&value).chars().take(80).collect::<String>();
                        let mut o = json!({ "value": value, "label": label });
                        // States the button gets when this is chosen: a name, an icon in the
                        // plugin's folder and the text on it
                        let states: Vec<Value> = c
                            .get("states")
                            .and_then(|v| v.as_array())
                            .into_iter()
                            .flatten()
                            .take(8)
                            .map(|st| {
                                let s = |k: &str, n: usize| st.get(k).and_then(|v| v.as_str()).unwrap_or("").chars().take(n).collect::<String>();
                                let icon = s("icon", 200);
                                let ar = st.get("ar").and_then(|v| v.as_f64()).filter(|a| a.is_finite()).map(|a| a.clamp(0.2, 5.0)).unwrap_or(1.0);
                                json!({ "name": s("name", 24), "icon": if safe_rel(&icon) { icon } else { String::new() }, "text": s("text", 24), "ar": ar })
                            })
                            .collect();
                        if !states.is_empty() {
                            o["states"] = Value::Array(states);
                        }
                        Some(o)
                    })
                    .collect()
            })
            .unwrap_or_default())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_kinds_and_names_stay_inside_the_plugin() {
        assert_eq!(split_kind("plug:obs:scene"), Some(("obs", "scene")));
        assert_eq!(split_kind("plug:../x:scene"), None);
        assert_eq!(split_kind("key"), None);
        assert!(safe_rel("icons/live.png"));
        assert!(!safe_rel("../secret.png"));
        assert!(!safe_rel("icons/../../x.png"));
        assert!(!safe_name(".hidden"));
    }

    #[test]
    fn icons_and_gifs_come_from_the_manifest() {
        let dir = std::env::temp_dir().join(format!("md-media-test-{}", crate::storage::now_ms()));
        fs::create_dir_all(dir.join("icons")).unwrap();
        fs::create_dir_all(dir.join("gifs")).unwrap();
        fs::write(dir.join("icons/live_on.png"), b"png").unwrap();
        fs::write(dir.join("gifs/spark.gif"), b"gif").unwrap();
        fs::write(dir.join("manifest.json"), r#"{ "icons": ["icons/live_on.png", "icons/missing.png", "../x.png"], "gifs": ["gifs/spark.gif"] }"#).unwrap();
        let icons = manifest_media(&dir, "icons");
        assert_eq!(icons.len(), 1);
        assert_eq!((icons[0].file.as_str(), icons[0].name.as_str()), ("icons/live_on.png", "live on"));
        assert!(icons[0].url.starts_with("data:image/png;base64,"));
        assert!(manifest_media(&dir, "gifs")[0].url.starts_with("data:image/gif;base64,"));
        let _ = fs::remove_dir_all(&dir);
    }
}

// ===== Installing, removing, turning off, trust =====

/// Unpack a .mdplugin into `to`, checking every name stays inside, and return its manifest
pub(crate) fn unpack(file: &Path, to: &Path) -> Result<Manifest, String> {
    use std::io::Read;
    const MAX_INSTALL_BYTES: u64 = 200 * 1024 * 1024;
    let f = fs::File::open(file).map_err(|e| format!("Could not open {}: {e}", file.display()))?;
    let mut zip = zip::ZipArchive::new(f).map_err(|_| format!("{} isn't a .mdplugin file.", file.display()))?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().replace('\\', "/");
        if !safe_rel(&name) {
            return Err(format!("The pack has a file outside the plugin folder: {name}"));
        }
        total += entry.size();
        if total > MAX_INSTALL_BYTES {
            return Err("The pack is bigger than 200 MB unpacked.".into());
        }
        let path = to.join(&name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut bytes = Vec::with_capacity(entry.size().min(MAX_INSTALL_BYTES) as usize);
        entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    let text = fs::read_to_string(to.join("manifest.json")).map_err(|_| "The pack has no manifest.json.".to_string())?;
    let m = check_manifest(&text)?;
    if !to.join(&m.dll).is_file() {
        return Err(format!("The pack has no {}.", m.dll));
    }
    Ok(m)
}

/// What a .mdplugin is, before installing it
#[derive(Serialize)]
pub struct PackInfo {
    id: String,
    name: String,
    version: String,
    actions: usize,
    signature: Trust,
    /// The version already installed, if any
    installed: Option<String>,
    /// Whether it will run once installed (signed by a trusted author, or Developer mode is on)
    runs: bool,
    why: Option<String>,
}

fn inspect_file(root: &Path, file: &Path) -> Result<PackInfo, String> {
    let temp = root.join("plugins").join(format!(".inspect-{}", crate::storage::now_ms()));
    let result = (|| {
        let m = unpack(file, &temp)?;
        let cfg = read_config(root);
        let signature = signing::verify_folder(&temp, &cfg.trusted);
        let actions = fs::read_to_string(temp.join(&m.actions)).ok().and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok()).map(|a| a.len()).unwrap_or(0);
        let installed = fs::read_to_string(root.join("plugins").join(&m.id).join("manifest.json")).ok().and_then(|t| check_manifest(&t).ok()).map(|o| o.version);
        let why = may_run(&signature, &cfg).err();
        Ok(PackInfo { id: m.id, name: m.name, version: m.version, actions, runs: why.is_none(), why, signature, installed })
    })();
    let _ = fs::remove_dir_all(&temp);
    result
}

/// Install a .mdplugin into <root>\plugins\<id>\, replacing an older copy. Damaged or changed
/// packs are refused; unsigned ones install but only run in Developer mode.
pub(crate) fn install_file(root: &Path, file: &Path) -> Result<(Manifest, PathBuf, Trust), String> {
    let plugins = root.join("plugins");
    fs::create_dir_all(&plugins).map_err(|e| e.to_string())?;
    let stamp = crate::storage::now_ms();
    let temp = plugins.join(format!(".install-{stamp}"));
    let result = unpack(file, &temp).and_then(|m| {
        let trust = signing::verify_folder(&temp, &read_config(root).trusted);
        if let Trust::Bad { why } = &trust {
            return Err(format!("{why} It wasn't installed."));
        }
        let dest = plugins.join(&m.id);
        if dest.exists() {
            // Move the old version aside first, so a failed swap never leaves no plugin at all
            let old = plugins.join(format!(".old-{}-{stamp}", m.id));
            fs::rename(&dest, &old).map_err(|e| format!("Could not replace the installed {}: {e}", m.name))?;
            let _ = fs::remove_dir_all(&old);
        }
        fs::rename(&temp, &dest).map_err(|e| format!("Could not install {}: {e}", m.name))?;
        Ok((m, dest, trust))
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    result
}

/// Developer mode and the trusted authors, for Settings › Plugins
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigView {
    dev_mode: bool,
    authors: Vec<Value>,
    /// This PC's own author key, if it has one
    my_key: Option<Value>,
}

fn config_view(root: &Path) -> ConfigView {
    let cfg = read_config(root);
    let mut authors: Vec<Value> = signing::BUILT_IN.iter().map(|(n, k)| json!({ "name": n, "key": k, "fingerprint": signing::fingerprint(k), "builtIn": true })).collect();
    authors.extend(cfg.trusted.iter().map(|a| json!({ "name": a.name, "key": a.key, "fingerprint": signing::fingerprint(&a.key), "builtIn": false })));
    let my_key = signing::author(root).map(|a| json!({ "name": a.name, "fingerprint": signing::fingerprint(&a.key) }));
    ConfigView { dev_mode: cfg.dev_mode, authors, my_key }
}

fn change_config(change: impl FnOnce(&mut PluginConfig)) -> Result<(), String> {
    let sup = sup()?;
    let mut cfg = read_config(&sup.root);
    change(&mut cfg);
    write_config(&sup.root, &cfg)
}

#[tauri::command]
pub fn plugin_config() -> Result<ConfigView, String> {
    Ok(config_view(&sup()?.root))
}

/// Developer mode on or off; every plugin is checked again
#[tauri::command]
pub async fn set_dev_mode(on: bool) -> Result<Vec<PluginInfo>, String> {
    change_config(|c| c.dev_mode = on)?;
    crate::applog::line(if on { "Developer mode turned on" } else { "Developer mode turned off" });
    tauri::async_runtime::spawn_blocking(|| reload_now(None)).await.map_err(|e| e.to_string())??;
    list_plugins()
}

/// Turn a plugin on or off (off: it stops, and its buttons say so)
#[tauri::command]
pub async fn set_plugin_enabled(id: String, on: bool) -> Result<Vec<PluginInfo>, String> {
    if !safe_name(&id) {
        return Err("Unknown plugin.".into());
    }
    let key = id.clone();
    change_config(move |c| {
        c.disabled.retain(|d| *d != key);
        if !on {
            c.disabled.push(key);
        }
    })?;
    tauri::async_runtime::spawn_blocking(move || reload_now(Some(id))).await.map_err(|e| e.to_string())??;
    list_plugins()
}

/// Remove a plugin: it stops and its folder is deleted. Its saved settings stay, for a reinstall.
#[tauri::command]
pub async fn remove_plugin(id: String) -> Result<Vec<PluginInfo>, String> {
    if !safe_name(&id) {
        return Err("Unknown plugin.".into());
    }
    let sup = sup()?.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let _one_at_a_time = lock(&sup.loading);
        let _ = sup.request(json!({ "t": "unload", "id": id }), Duration::from_secs(5));
        if id == crate::speech::PLUGIN_ID {
            crate::speech::forget();
        }
        let folder = sup.dir.join(&id);
        // Renamed first so a half-deleted folder never looks like a plugin
        let gone = sup.dir.join(format!(".removed-{id}-{}", crate::storage::now_ms()));
        fs::rename(&folder, &gone).map_err(|e| format!("Could not remove it: {e}"))?;
        let _ = fs::remove_dir_all(&gone);
        lock(&sup.status).remove(&id);
        let key = id.clone();
        let _ = change_config(move |c| c.disabled.retain(|d| *d != key));
        crate::applog::line(&format!("Plugin {id} removed"));
        sup.emit_changed();
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    list_plugins()
}

/// What a .mdplugin file is: name, version, who signed it and whether it will run
#[tauri::command]
pub async fn inspect_plugin(path: String) -> Result<PackInfo, String> {
    let root = sup()?.root.clone();
    tauri::async_runtime::spawn_blocking(move || inspect_file(&root, Path::new(&path))).await.map_err(|e| e.to_string())?
}

/// Install a .mdplugin and start it
#[tauri::command]
pub async fn install_plugin(path: String) -> Result<Vec<PluginInfo>, String> {
    let root = sup()?.root.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let (m, _, _) = install_file(&root, Path::new(&path))?;
        crate::applog::line(&format!("Plugin {} {} installed", m.id, m.version));
        reload_now(Some(m.id))
    })
    .await
    .map_err(|e| e.to_string())??;
    list_plugins()
}

/// Trust an author's key (.mdkey); their signed plugins run without Developer mode
#[tauri::command]
pub async fn add_author(path: String) -> Result<ConfigView, String> {
    let key = signing::read_key_file(Path::new(&path))?;
    let name = key.name.clone();
    change_config(move |c| {
        c.trusted.retain(|a| a.key != key.key);
        c.trusted.push(key);
    })?;
    crate::applog::line(&format!("Trusted plugin author {name}"));
    tauri::async_runtime::spawn_blocking(|| reload_now(None)).await.map_err(|e| e.to_string())??;
    plugin_config()
}

/// Stop trusting an author the user added
#[tauri::command]
pub async fn remove_author(key: String) -> Result<ConfigView, String> {
    change_config(move |c| c.trusted.retain(|a| a.key != key))?;
    tauri::async_runtime::spawn_blocking(|| reload_now(None)).await.map_err(|e| e.to_string())??;
    plugin_config()
}

/// Choose a .mdplugin (kind "plugin") or .mdkey (kind "key") file
#[tauri::command]
pub async fn pick_plugin_file(app: AppHandle, kind: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (title, label, ext) = if kind == "key" { ("Add an author key", "Minniondeck author key", "mdkey") } else { ("Install a plugin", "Minniondeck plugin", "mdplugin") };
    let picked = app.dialog().file().set_title(title).add_filter(label, &[ext]).blocking_pick_file();
    match picked {
        Some(p) => Ok(Some(p.into_path().map_err(|e| e.to_string())?.display().to_string())),
        None => Ok(None),
    }
}

// ===== A plugin's own settings (see plugin_settings.rs) =====

/// A plugin's settings fields and their values (passwords only say whether one is saved)
#[tauri::command]
pub fn plugin_settings(id: String) -> Result<Value, String> {
    if !safe_name(&id) {
        return Err("Unknown plugin.".into());
    }
    let sup = sup()?;
    let fields = crate::plugin_settings::schema_of(&sup.dir.join(&id));
    Ok(crate::plugin_settings::for_window(&sup.data, &id, &fields))
}

/// Save what the user changed, then hand the plugin its new settings if it is running
#[tauri::command]
pub async fn save_plugin_settings(id: String, values: serde_json::Map<String, Value>) -> Result<String, String> {
    if !safe_name(&id) {
        return Err("Unknown plugin.".into());
    }
    let sup = sup()?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let fields = crate::plugin_settings::schema_of(&sup.dir.join(&id));
        crate::plugin_settings::save(&sup.data, &id, &fields, &values)?;
        if sup.state_of(&id).is_none_or(|s| s.state != "ready") {
            return Ok("Saved. The plugin uses them when it starts.".to_string());
        }
        let settings = sup.read_settings(&id);
        match sup.request(json!({ "t": "call", "id": id, "msg": { "t": "settings", "settings": settings } }), Duration::from_secs(5)) {
            Ok(_) => Ok("Saved".into()),
            Err(e) => Ok(format!("Saved, but the plugin didn't take them yet: {e}")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A button in a plugin's settings was pressed: the plugin's settings_button answers
#[tauri::command]
pub async fn plugin_settings_button(id: String, key: String) -> Result<String, String> {
    if !safe_name(&id) {
        return Err("Unknown plugin.".into());
    }
    let sup = sup()?.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if sup.state_of(&id).is_none_or(|s| s.state != "ready") {
            return Err("Start the plugin first: it isn't running.".to_string());
        }
        let v = sup.request(json!({ "t": "call", "id": id, "msg": { "t": "settings_button", "key": key } }), PRESS_TIMEOUT)?;
        let message = v.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
        if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false) { Ok(if message.is_empty() { "Done".into() } else { message }) } else { Err(if message.is_empty() { "It didn't work.".into() } else { message }) }
    })
    .await
    .map_err(|e| e.to_string())?
}
