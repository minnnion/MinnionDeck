//! Plugins: folders in <data>\plugins\<id>\ holding manifest.json, actions.json, icons and a DLL.
//! Ported from the Minniondeck app's plugins.rs, with the window talking to it directly instead
//! of through Tauri commands and events.
//!
//! Every plugin runs in the plugin host (pluginhost.rs), a helper copy of this exe started with
//! `--plugin-host` and watched from here. If the host crashes, plugins show as starting, the host
//! is started again and the plugins loaded again; after 3 crashes in a minute it stays stopped
//! until the user reloads plugins in Settings.
//!
//! The host loads a copy of each DLL (in <data>\plugin-run\), so a plugin's own DLL is never
//! locked and can be rebuilt while the app runs; reloading picks up the new build at once.
//!
//! Button kinds for plugin actions are "plug:<id>:<action>". A plugin's own settings are kept in
//! <data>\plugin-data\<id>\settings.mdsave (sealed), apart from its folder, so reinstalling keeps them.
//!
//! What a plugin sends back (a button's look, title, icon, a toast, a changed list of choices)
//! comes to the window as a `Msg` on a channel; every message also asks the window to repaint,
//! since the window sleeps while nothing moves.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::actions::Step;
use super::pluginhost::API_VERSION;
use super::package::Kind;
use super::signing::{self, AuthorKey, Trust};
use super::{util, vault};

/// Largest icon file a plugin can put on a button
const MAX_ICON_BYTES: u64 = 512 * 1024;
/// How long a press may take before the button reports that the plugin didn't answer
const PRESS_TIMEOUT: Duration = Duration::from_secs(15);
const CRASHES_ALLOWED: usize = 3;
/// The plugin that needs the speech model, which this app doesn't have yet: it isn't copied over
const SPEECH_PLUGIN: &str = "minnion_speech";

static SUP: OnceLock<Arc<Supervisor>> = OnceLock::new();
static LOG: OnceLock<PathBuf> = OnceLock::new();

/// What a plugin tells the window.
pub enum Msg {
    /// The list of plugins or one's state changed: look at `list()` again
    Changed,
    /// A plugin changed one of its buttons. `state` is a look; `title` and `icon` are live
    /// text and picture (`Some(None)` clears them). An icon is its name for the window and its PNG bytes.
    Button { button: String, state: Option<usize>, title: Option<Option<String>>, icon: Option<Option<(String, Vec<u8>)>> },
    Toast { plugin: String, text: String },
    /// Live data a button draws itself (a chat): what changed, to merge into what it has
    Data { button: String, data: Value },
    /// The choices of a dynamic dropdown changed
    OptionsChanged,
}

/// The button an event is about
#[derive(Deserialize, Serialize, Clone, Default, Debug)]
pub struct ButtonCtx {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub state: usize,
}

#[derive(Deserialize, Debug)]
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

/// A plugin as Settings shows it
#[derive(Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub actions: usize,
    /// "ready", "waking" (starting), "dizzy" (stopped) or "off" (turned off)
    pub state: String,
    /// Why this plugin can't be used, when it can't
    pub error: Option<String>,
    /// Who signed it and whether this PC trusts them
    pub signature: Trust,
    /// Whether its manifest lists settings for Settings › Plugins
    pub has_settings: bool,
}

/// What the user decided about plugins, in <data>\plugins.mdsave
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

/// The plugin choices, sealed in <data>\plugins.mdsave (an earlier version's plain plugins.json is
/// sealed when it is first read).
pub(crate) fn read_config(root: &Path) -> PluginConfig {
    let (plain, sealed) = (root.join("plugins.json"), root.join("plugins.mdsave"));
    let _ = vault::migrate(&plain, &sealed, Kind::Save);
    vault::read_json(&sealed, Kind::Save).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

fn write_config(root: &Path, cfg: &PluginConfig) -> Result<(), String> {
    vault::write_json(&root.join("plugins.mdsave"), Kind::Save, &serde_json::to_value(cfg).map_err(|e| e.to_string())?)
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
    tx: Mutex<mpsc::Sender<Msg>>,
    repaint: Box<dyn Fn() + Send + Sync>,
    /// The data folder (plugins.json is here)
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
    /// Dynamic dropdown choices already asked for
    options: Mutex<HashMap<String, Dyn>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A line in <data>\logs\plugins.log
pub fn log(text: &str) {
    if let Some(dir) = LOG.get() {
        let _ = fs::create_dir_all(dir);
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join("plugins.log")) {
            let _ = writeln!(f, "{} {text}", util::now_ms());
        }
    }
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

/// A picture in a plugin folder as PNG bytes, or None if it isn't a small PNG inside the folder
pub(crate) fn read_icon(folder: &Path, rel: &str) -> Option<Vec<u8>> {
    if !safe_rel(rel) || !rel.to_ascii_lowercase().ends_with(".png") {
        return None;
    }
    let path = folder.join(rel);
    match fs::metadata(&path) {
        Ok(m) if m.len() <= MAX_ICON_BYTES => fs::read(&path).ok(),
        _ => None,
    }
}

/// A plugin folder as Settings sees it (state is filled in by the caller)
fn describe(folder: &Path) -> PluginInfo {
    let fallback = folder.file_name().and_then(|n| n.to_str()).unwrap_or("plugin").to_string();
    let bad = |e: String| PluginInfo { id: fallback.clone(), name: fallback.clone(), version: String::new(), actions: 0, state: "dizzy".into(), error: Some(e), signature: Trust::Unsigned, has_settings: false };
    let m = match read_manifest(folder) {
        Ok(m) => m,
        Err(e) => return bad(e),
    };
    let mut info = PluginInfo { id: m.id.clone(), name: m.name.clone(), version: m.version.clone(), actions: 0, state: "waking".into(), error: None, signature: Trust::Unsigned, has_settings: !super::plugin_settings::schema_of(folder).is_empty() };
    if !folder.join(&m.dll).is_file() {
        info.state = "dizzy".into();
        info.error = Some(format!("{} is missing.", m.dll));
        return info;
    }
    match fs::read_to_string(folder.join(&m.actions)).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Vec<Value>>(&t).map_err(|e| e.to_string())) {
        Ok(d) => info.actions = d.len(),
        Err(e) => {
            info.state = "dizzy".into();
            info.error = Some(format!("{} is not valid: {e}", m.actions));
        }
    }
    info
}

/// "plug:<id>:<action>" → (id, action)
pub fn split_kind(kind: &str) -> Option<(&str, &str)> {
    let mut parts = kind.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("plug"), Some(id), Some(a)) if safe_name(id) && safe_name(a) => Some((id, a)),
        _ => None,
    }
}

impl Supervisor {
    fn emit(&self, msg: Msg) {
        let _ = lock(&self.tx).send(msg);
        (self.repaint)();
    }

    fn emit_changed(&self) {
        lock(&self.options).clear();
        self.emit(Msg::Changed);
    }

    fn set_status(&self, id: &str, state: &'static str, error: Option<String>) {
        lock(&self.status).insert(id.to_string(), Status { state, error });
    }

    /// The settings a plugin gets: saved values with secrets decrypted, and defaults
    fn read_settings(&self, id: &str) -> Value {
        super::plugin_settings::for_plugin(&self.data, id, &super::plugin_settings::schema_of(&self.dir.join(id)))
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
        cmd.arg(super::pluginhost::ARG).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
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
                let state = msg.get("state").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                self.emit(Msg::Button { button, state: Some(state), title: None, icon: None });
            }
            Some("set_title") => {
                let title = s("title").map(|t| clip(t, 40));
                self.emit(Msg::Button { button, state: None, title: Some(title), icon: None });
            }
            Some("set_icon") => {
                let icon = s("icon").and_then(|rel| read_icon(&self.dir.join(id), &rel).map(|b| (format!("live/{id}/{rel}"), b)));
                self.emit(Msg::Button { button, state: None, title: None, icon: Some(icon) });
            }
            // live data a button draws itself (a chat); only what changed comes after the first
            Some("set_data") => {
                if let Some(data) = msg.get("data").filter(|d| d.is_object() && d.to_string().len() <= 256 * 1024) {
                    self.emit(Msg::Data { button, data: data.clone() });
                }
            }
            Some("log") => log(&format!("Plugin {id}: {}", clip(s("text").unwrap_or_default(), 500))),
            Some("toast") => self.emit(Msg::Toast { plugin: id.to_string(), text: clip(s("text").unwrap_or_default(), 200) }),
            Some("set_setting") => {
                if let Some(key) = s("key") {
                    let fields = super::plugin_settings::schema_of(&self.dir.join(id));
                    let value = msg.get("value").cloned().unwrap_or(Value::Null);
                    if let Err(e) = super::plugin_settings::set_from_plugin(&self.data, id, &fields, &key, value) {
                        log(&format!("Plugin {id}: could not save the setting {key}: {e}"));
                    }
                }
            }
            Some("refresh_options") => {
                lock(&self.options).clear();
                self.emit(Msg::OptionsChanged);
            }
            // The speech model isn't part of this app yet
            Some("speak") => self.emit(Msg::Toast { plugin: id.to_string(), text: "Speech isn't in this app yet.".into() }),
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
            log("The plugin host crashed 3 times in a minute; plugins stay stopped until they are reloaded.");
            for id in &ids {
                self.set_status(id, "dizzy", Some("Plugins stopped after crashing 3 times in a minute. Use Reload plugins to try again.".into()));
            }
            self.emit_changed();
            return;
        }
        log("The plugin host stopped; starting it again.");
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
                log(&e);
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
        let stop = |state: &'static str, error: Option<String>| {
            let _ = self.request(json!({ "t": "unload", "id": id }), Duration::from_secs(5));
            if let Some(e) = &error {
                log(&format!("Plugin {id} not started: {e}"));
            }
            self.set_status(id, state, error);
        };
        // Turned off: make sure it isn't running
        if cfg.disabled.iter().any(|d| d == id) {
            return stop("off", None);
        }
        // The DLL is copied first and the copy is what runs, so it is the copy that is checked
        // against the signature: a file swapped in the folder after the check can't slip through.
        let shadow = match prepare_copy(&folder, &self.run, id, &cfg) {
            Ok(s) => s,
            Err(e) => return stop("dizzy", Some(e)),
        };
        let result = (|| -> Result<(), String> {
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
                log(&format!("Plugin {id} started"));
            }
            Err(e) => {
                log(&format!("Plugin {id} didn't start: {e}"));
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

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)?.flatten() {
        let p = e.path();
        let dest = to.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &dest)?;
        } else {
            fs::copy(&p, &dest)?;
        }
    }
    Ok(())
}

/// First start: copy the plugins, their saved settings and plugins.json from the Minniondeck
/// app's folder (read only). A plugin folder is copied whole or not at all, since its signature
/// covers every file. Returns how many plugins were copied.
#[allow(dead_code)]
pub fn seed_from_old_app(root: &Path) -> usize {
    let target = root.join("plugins");
    if target.exists() {
        return 0;
    }
    let Some(old) = super::paths::old_app_root() else { return 0 };
    let from = old.join("plugins");
    if !from.is_dir() || fs::create_dir_all(&target).is_err() {
        return 0;
    }
    let mut n = 0;
    for p in plugin_folders(&from) {
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        if name == SPEECH_PLUGIN {
            continue;
        }
        let dest = target.join(&name);
        if copy_dir(&p, &dest).is_ok() {
            n += 1;
        } else {
            let _ = fs::remove_dir_all(&dest);
        }
    }
    let _ = copy_dir(&old.join("plugin-data"), &root.join("plugin-data"));
    // read from the old folder and sealed here: no plain copy is left
    if let Some(v) = fs::read_to_string(old.join("plugins.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        let _ = vault::write_json(&root.join("plugins.mdsave"), Kind::Save, &v);
    }
    n
}

/// Called once at start with the data folder: starts the plugin host and loads plugins. `tx` gets
/// what plugins tell the window; `repaint` wakes the window up.
pub fn init(root: &Path, tx: mpsc::Sender<Msg>, repaint: impl Fn() + Send + Sync + 'static) {
    let dir = root.join("plugins");
    let data = root.join("plugin-data");
    let run = root.join("plugin-run");
    let _ = LOG.set(root.join("logs"));
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
        tx: Mutex::new(tx),
        repaint: Box::new(repaint),
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
        options: Mutex::new(HashMap::new()),
    });
    if SUP.set(sup.clone()).is_err() {
        return;
    }
    // Every plugin starts as "starting", so the window knows about them before the host answers
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

/// A button with a plugin action was pressed. Blocks until the plugin answers.
pub fn press(kind: &str, fields: &Value, button: &ButtonCtx) -> Result<String, String> {
    sup()?.event(kind, "press", fields, button, true, None)
}

/// The state of a plugin: "ready", "waking", "dizzy" or "off", and why not when it isn't ready
pub fn state_of(id: &str) -> Option<(&'static str, Option<String>)> {
    sup().ok()?.state_of(id).map(|s| (s.state, s.error))
}

/// What a plugin folder's signature check found, kept while none of its files changed (their
/// names, sizes and times) and the trusted authors are the same. Install, remove and reload clear it.
static TRUST_CACHE: Mutex<Option<HashMap<PathBuf, (u64, Trust)>>> = Mutex::new(None);

/// A cheap stamp of a folder's files and the trusted keys: what makes a signature check go stale.
fn folder_stamp(folder: &Path, trusted: &[AuthorKey]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut stack = vec![folder.to_path_buf()];
    let mut files = vec![];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                files.push((e.path(), m.len(), m.modified().ok()));
            }
        }
    }
    files.sort();
    files.hash(&mut h);
    trusted.iter().map(|a| &a.key).collect::<Vec<_>>().hash(&mut h);
    h.finish()
}

/// `signing::verify_folder`, from the cache when nothing changed.
fn cached_trust(folder: &Path, trusted: &[AuthorKey]) -> Trust {
    let stamp = folder_stamp(folder, trusted);
    let mut cache = TRUST_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = cache.get_or_insert_with(HashMap::new);
    if let Some((_, t)) = map.get(folder).filter(|(s, _)| *s == stamp) {
        return t.clone();
    }
    let t = signing::verify_folder(folder, trusted);
    map.insert(folder.to_path_buf(), (stamp, t.clone()));
    t
}

fn forget_trust() {
    *TRUST_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Installed plugins, their actions and whether each is running
pub fn list() -> Vec<PluginInfo> {
    let Ok(sup) = sup() else { return vec![] };
    let cfg = read_config(&sup.root);
    let mut out: Vec<PluginInfo> = sup
        .folders()
        .iter()
        .map(|f| {
            let mut info = describe(f);
            info.signature = cached_trust(f, &cfg.trusted);
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
    out
}

/// Load plugins again: one (`id`) or all. Picks up new and rebuilt plugins without a restart.
/// Blocks; call it from a thread.
pub fn reload(id: Option<String>) -> Result<(), String> {
    forget_trust();
    let sup = sup()?.clone();
    lock(&sup.crashes).clear();
    if lock(&sup.host).stdin.is_none() {
        sup.spawn_host()?;
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

/// A button event other than a press: "release", "long_press", "shown", "hidden", "view" or
/// "turn", for each plugin step of the button. A long press waits for and returns the result,
/// like a press. "turn" turns a knob button (`data` {"by": clicks, + for clockwise}). Blocks only
/// for a long press; call it from a thread anyway.
pub fn button_event(event: &str, button: &ButtonCtx, steps: &[Step], data: Option<&Value>) -> Result<String, String> {
    if !matches!(event, "release" | "long_press" | "shown" | "hidden" | "view" | "turn") {
        return Err("Unknown button event.".into());
    }
    let sup = sup()?;
    let wait = event == "long_press";
    let mut last = String::new();
    for s in steps.iter().filter(|s| s.kind.starts_with("plug:")) {
        match sup.event(&s.kind, event, &s.fields, button, wait, data) {
            Ok(m) => last = m,
            Err(e) if wait => return Err(e),
            Err(_) => {}
        }
    }
    Ok(last)
}

/// A choice for a plugin action's dynamic dropdown
#[derive(Clone, Debug)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Clone)]
pub enum Dyn {
    Loading,
    Ready(Vec<Choice>),
    /// The plugin isn't running, or didn't answer
    Unavailable,
}

/// The choices of a dynamic dropdown. The first call asks the plugin on another thread and says
/// `Loading`; the window is repainted when the answer is in. Plugins say when the list changes.
pub fn dynamic_options(kind: &str, field: &str, fields: &Value) -> Dyn {
    let Ok(sup) = sup() else { return Dyn::Unavailable };
    // the other fields' values are part of it: a list can follow another field (Source after Scene)
    let mut others: Vec<(String, String)> = fields.as_object().map(|m| m.iter().filter(|(k, _)| k.as_str() != field).map(|(k, v)| (k.clone(), v.to_string())).collect()).unwrap_or_default();
    others.sort();
    let key = format!("{kind}|{field}|{}", others.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&"));
    if let Some(d) = lock(&sup.options).get(&key) {
        return d.clone();
    }
    let Some((id, _)) = split_kind(kind) else { return Dyn::Unavailable };
    if sup.state_of(id).is_none_or(|s| s.state != "ready") {
        return Dyn::Unavailable;
    }
    lock(&sup.options).insert(key.clone(), Dyn::Loading);
    let (sup, kind, field, fields) = (sup.clone(), kind.to_string(), field.to_string(), fields.clone());
    std::thread::spawn(move || {
        let got = fetch_options(&sup, &kind, &field, &fields).map(Dyn::Ready).unwrap_or(Dyn::Unavailable);
        lock(&sup.options).insert(key, got);
        (sup.repaint)();
    });
    Dyn::Loading
}

fn fetch_options(sup: &Supervisor, kind: &str, field: &str, fields: &Value) -> Result<Vec<Choice>, String> {
    let (id, action) = split_kind(kind).ok_or("Not a plugin action.")?;
    let v = sup.request(json!({ "t": "call", "id": id, "msg": { "t": "options", "action": action, "field": field, "fields": fields } }), Duration::from_secs(5))?;
    Ok(v.get("options")
        .and_then(|o| o.as_array())
        .map(|list| {
            list.iter()
                .take(500)
                .filter_map(|c| {
                    let value = c.get("value").and_then(|v| v.as_str())?.chars().take(200).collect::<String>();
                    let label = c.get("label").and_then(|v| v.as_str()).unwrap_or(&value).chars().take(80).collect::<String>();
                    Some(Choice { value, label })
                })
                .collect()
        })
        .unwrap_or_default())
}

// ===== Turning plugins off, Developer mode, removing =====

/// Developer mode on or off
pub fn dev_mode() -> bool {
    sup().map(|s| read_config(&s.root).dev_mode).unwrap_or(false)
}

fn change_config(change: impl FnOnce(&mut PluginConfig)) -> Result<(), String> {
    let sup = sup()?;
    let mut cfg = read_config(&sup.root);
    change(&mut cfg);
    write_config(&sup.root, &cfg)
}

/// Developer mode on or off; every plugin is checked again. Blocks; call it from a thread.
pub fn set_dev_mode(on: bool) -> Result<(), String> {
    change_config(|c| c.dev_mode = on)?;
    log(if on { "Developer mode turned on" } else { "Developer mode turned off" });
    reload(None)
}

/// Turn a plugin on or off (off: it stops, and its buttons say so). Blocks; call it from a thread.
pub fn set_enabled(id: &str, on: bool) -> Result<(), String> {
    if !safe_name(id) {
        return Err("Unknown plugin.".into());
    }
    let key = id.to_string();
    change_config(move |c| {
        c.disabled.retain(|d| *d != key);
        if !on {
            c.disabled.push(key);
        }
    })?;
    reload(Some(id.to_string()))
}

/// Remove a plugin: it stops and its folder is deleted. Its saved settings stay, for a reinstall.
pub fn remove(id: &str) -> Result<(), String> {
    if !safe_name(id) {
        return Err("Unknown plugin.".into());
    }
    let sup = sup()?.clone();
    let _one_at_a_time = lock(&sup.loading);
    let _ = sup.request(json!({ "t": "unload", "id": id }), Duration::from_secs(5));
    let folder = sup.dir.join(id);
    // Renamed first so a half-deleted folder never looks like a plugin
    let gone = sup.dir.join(format!(".removed-{id}-{}", util::now_ms()));
    fs::rename(&folder, &gone).map_err(|e| format!("Could not remove it: {e}"))?;
    let _ = fs::remove_dir_all(&gone);
    lock(&sup.status).remove(id);
    forget_trust();
    let key = id.to_string();
    let _ = change_config(move |c| c.disabled.retain(|d| *d != key));
    log(&format!("Plugin {id} removed"));
    sup.emit_changed();
    Ok(())
}

// ===== A plugin's own settings (see plugin_settings.rs) =====

/// A plugin's settings fields and their values (passwords only say whether one is saved)
pub fn settings_view(id: &str) -> Result<Value, String> {
    if !safe_name(id) {
        return Err("Unknown plugin.".into());
    }
    let sup = sup()?;
    let fields = super::plugin_settings::schema_of(&sup.dir.join(id));
    Ok(super::plugin_settings::for_window(&sup.data, id, &fields))
}

/// Save what the user changed, then hand the plugin its new settings if it is running. Blocks.
pub fn save_settings(id: &str, values: &serde_json::Map<String, Value>) -> Result<String, String> {
    if !safe_name(id) {
        return Err("Unknown plugin.".into());
    }
    let sup = sup()?;
    let fields = super::plugin_settings::schema_of(&sup.dir.join(id));
    super::plugin_settings::save(&sup.data, id, &fields, values)?;
    if sup.state_of(id).is_none_or(|s| s.state != "ready") {
        return Ok("Saved. The plugin uses them when it starts.".to_string());
    }
    let settings = sup.read_settings(id);
    match sup.request(json!({ "t": "call", "id": id, "msg": { "t": "settings", "settings": settings } }), Duration::from_secs(5)) {
        Ok(_) => Ok("Saved".into()),
        Err(e) => Ok(format!("Saved, but the plugin didn't take them yet: {e}")),
    }
}

/// A button in a plugin's settings was pressed: the plugin's settings_button answers. Blocks.
pub fn settings_button(id: &str, key: &str) -> Result<String, String> {
    if !safe_name(id) {
        return Err("Unknown plugin.".into());
    }
    let sup = sup()?;
    if sup.state_of(id).is_none_or(|s| s.state != "ready") {
        return Err("Start the plugin first: it isn't running.".to_string());
    }
    let v = sup.request(json!({ "t": "call", "id": id, "msg": { "t": "settings_button", "key": key } }), PRESS_TIMEOUT)?;
    let message = v.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string();
    if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false) { Ok(if message.is_empty() { "Done".into() } else { message }) } else { Err(if message.is_empty() { "It didn't work.".into() } else { message }) }
}

// ===== Installing a .mdplugin =====

/// The data folder that holds plugins\ (for downloads from the marketplace, see market.rs)
pub fn data_root() -> Result<PathBuf, String> {
    Ok(sup()?.root.clone())
}

/// The biggest .mdplugin read (the file itself; unpacked it may be up to MAX_INSTALL_BYTES)
const MAX_PACK_BYTES: u64 = 200 * 1024 * 1024;

/// Reads a .mdplugin whole, once: what is checked is what is installed.
fn read_pack(file: &Path) -> Result<Vec<u8>, String> {
    let len = fs::metadata(file).map_err(|e| format!("Could not open {}: {e}", file.display()))?.len();
    if len > MAX_PACK_BYTES {
        return Err(format!("{} is bigger than 200 MB.", file.display()));
    }
    fs::read(file).map_err(|e| format!("Could not open {}: {e}", file.display()))
}

/// Unpack a .mdplugin (its bytes) into `to`, checking every name stays inside, and return its manifest
pub(crate) fn unpack(pack: &[u8], to: &Path) -> Result<Manifest, String> {
    use std::io::Read;
    const MAX_INSTALL_BYTES: u64 = 200 * 1024 * 1024;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(pack)).map_err(|_| "That isn't a .mdplugin file.".to_string())?;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(|_| "The pack has a file name that can't be read.".to_string())?.replace('\\', "/");
        if !safe_rel(&name) {
            return Err(format!("The pack has a file outside the plugin folder: {name}"));
        }
        if entry.is_symlink() {
            return Err(format!("The pack has a link instead of a file: {name}"));
        }
        let path = to.join(&name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // what really unpacks is counted, not what the zip claims, so a lying size can't fill the disk
        let room = MAX_INSTALL_BYTES - total;
        let mut bytes = Vec::with_capacity(entry.size().min(room) as usize);
        (&mut entry).take(room + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        total += bytes.len() as u64;
        if total > MAX_INSTALL_BYTES {
            return Err("The pack is bigger than 200 MB unpacked.".into());
        }
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    let text = fs::read_to_string(to.join("manifest.json")).map_err(|_| "The pack has no manifest.json.".to_string())?;
    let m = check_manifest(&text)?;
    if !to.join(&m.dll).is_file() {
        return Err(format!("The pack has no {}.", m.dll));
    }
    Ok(m)
}

/// Install a .mdplugin into <root>\plugins\<id>\, replacing an older copy. Damaged or changed
/// packs are refused; unsigned ones install but only run in Developer mode.
pub(crate) fn install_file(root: &Path, file: &Path) -> Result<(Manifest, PathBuf, Trust), String> {
    install_bytes(root, &read_pack(file)?)
}

/// `install_file` for a pack already read.
pub(crate) fn install_bytes(root: &Path, file: &[u8]) -> Result<(Manifest, PathBuf, Trust), String> {
    forget_trust();
    let plugins = root.join("plugins");
    fs::create_dir_all(&plugins).map_err(|e| e.to_string())?;
    let stamp = util::now_ms();
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

/// What a .mdplugin is, before installing it
#[derive(Clone, Debug)]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub actions: usize,
    pub signature: Trust,
    /// The version already installed, if any
    pub installed: Option<String>,
    /// Whether it will run once installed (signed by a trusted author, or Developer mode is on)
    pub runs: bool,
    pub why: Option<String>,
    /// The SHA-256 of the file as it was looked at: Install refuses a file that changed since
    pub sha: String,
    /// The fingerprint of the key it is signed with, to compare with the author's own page
    pub key: Option<String>,
}

impl PackInfo {
    /// Installing this would put an older version over a newer one
    pub fn downgrade(&self) -> bool {
        self.installed.as_deref().is_some_and(|have| super::updates::is_newer(have, &self.version))
    }
}

fn inspect_in(root: &Path, file: &Path) -> Result<PackInfo, String> {
    let temp = root.join("plugins").join(format!(".inspect-{}", util::now_ms()));
    let result = (|| {
        let bytes = read_pack(file)?;
        let sha = util::sha256_hex(&bytes);
        let m = unpack(&bytes, &temp)?;
        let cfg = read_config(root);
        let signature = signing::verify_folder(&temp, &cfg.trusted);
        let actions = fs::read_to_string(temp.join(&m.actions)).ok().and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok()).map(|a| a.len()).unwrap_or(0);
        let installed = fs::read_to_string(root.join("plugins").join(&m.id).join("manifest.json")).ok().and_then(|t| check_manifest(&t).ok()).map(|o| o.version);
        let why = may_run(&signature, &cfg).err();
        let key = signing::signer_key(&temp).map(|k| signing::fingerprint(&k));
        Ok(PackInfo { id: m.id, name: m.name, version: m.version, actions, runs: why.is_none(), why, signature, installed, sha, key })
    })();
    let _ = fs::remove_dir_all(&temp);
    result
}

/// Copies plugin `id`'s DLL into `run` and checks the copy: it may run (signed by a trusted author,
/// or Developer mode), and for a signed plugin the copy and the manifest naming it hash to what the
/// signature vouches for. Returns the copy, which is what the host loads.
fn prepare_copy(folder: &Path, run: &Path, id: &str, cfg: &PluginConfig) -> Result<PathBuf, String> {
    let manifest = fs::read(folder.join("manifest.json")).map_err(|_| "manifest.json is missing.".to_string())?;
    let m = check_manifest(&String::from_utf8_lossy(&manifest))?;
    if m.id != id {
        return Err(format!("The folder must be named \"{}\", like the id in manifest.json.", m.id));
    }
    fs::create_dir_all(run).map_err(|e| e.to_string())?;
    let shadow = run.join(format!("{id}-{}.dll", util::now_ms()));
    fs::copy(folder.join(&m.dll), &shadow).map_err(|e| format!("Could not copy {}: {e}", m.dll))?;
    let checked = (|| {
        let (trust, signed) = signing::verify_folder_hashes(folder, &cfg.trusted);
        may_run(&trust, cfg)?;
        if let Some(files) = signed {
            let copy = fs::read(&shadow).map_err(|e| format!("Could not read the copy of {}: {e}", m.dll))?;
            let same = |name: &str, bytes: &[u8]| files.get(name).is_some_and(|h| h.eq_ignore_ascii_case(&util::sha256_hex(bytes)));
            if !same(&m.dll, &copy) || !same("manifest.json", &manifest) {
                return Err("Its files changed while it was starting. It won't run. Install it again from its author.".to_string());
            }
        }
        Ok(())
    })();
    match checked {
        Ok(()) => Ok(shadow),
        Err(e) => {
            let _ = fs::remove_file(&shadow);
            Err(e)
        }
    }
}

/// Looks inside a .mdplugin without installing it: who made it, whether it will run. Blocks.
pub fn inspect(file: &Path) -> Result<PackInfo, String> {
    inspect_in(&sup()?.root, file)
}

/// Installs a .mdplugin (replacing an older copy) and starts it. Returns its name. Blocks.
/// The .mdprofile beside a plugin file that belongs to plugin `name` and has choices: the
/// profile, and its file's SHA-256.
pub fn profile_beside(media: &Path, file: &Path, name: &str) -> Option<(Value, String)> {
    for e in fs::read_dir(file.parent()?).ok()?.flatten() {
        let path = e.path();
        if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("mdprofile")) {
            continue;
        }
        // only looked at: a profile that isn't the one wanted leaves nothing behind
        let Ok(look) = super::profile_io::inspect_from(&path) else { continue };
        let ours = look["plugins"].as_array().is_some_and(|p| p.iter().any(|n| n.as_str() == Some(name)));
        if ours && !super::library::choices_of(&look).is_empty() {
            let bytes = fs::read(&path).ok()?;
            let bundle = super::profile_io::import_from(media, &path).ok()?;
            return Some((bundle, util::sha256_hex(&bytes)));
        }
    }
    None
}

/// Installs the plugin file the install question looked at. `sha`: its SHA-256 then; a file that
/// changed since (another file put in its place) is refused.
pub fn install_from_file(file: &Path, sha: &str) -> Result<String, String> {
    let root = sup()?.root.clone();
    let (m, _, _) = install_checked(&root, file, sha)?;
    log(&format!("Plugin {} {} installed", m.id, m.version));
    reload(Some(m.id.clone()))?;
    Ok(m.name)
}

pub(crate) fn install_checked(root: &Path, file: &Path, sha: &str) -> Result<(Manifest, PathBuf, Trust), String> {
    let bytes = read_pack(file)?;
    if !util::sha256_hex(&bytes).eq_ignore_ascii_case(sha) {
        return Err("The file changed after it was looked at, so it wasn't installed. Open it again.".into());
    }
    install_bytes(root, &bytes)
}

/// A .mdkey file as the "Trust this author?" question shows it. Reading it trusts nothing.
#[derive(Clone, Debug)]
pub struct AuthorInfo {
    pub key: AuthorKey,
    /// The key shown short, to compare with what the author published
    pub fingerprint: String,
    /// Already trusted on this PC (built in, or added before)
    pub known: bool,
    /// Another key is already trusted under this same name: someone may be passing as them
    pub name_taken: bool,
}

pub(crate) fn author_info_in(root: &Path, file: &Path) -> Result<AuthorInfo, String> {
    let key = signing::read_key_file(file)?;
    let cfg = read_config(root);
    let same_name = |n: &str| n.trim().eq_ignore_ascii_case(&key.name);
    let known = signing::BUILT_IN.iter().any(|(_, k)| *k == key.key) || cfg.trusted.iter().any(|a| a.key == key.key);
    let name_taken = signing::BUILT_IN.iter().any(|(n, k)| same_name(n) && *k != key.key) || cfg.trusted.iter().any(|a| same_name(&a.name) && a.key != key.key);
    Ok(AuthorInfo { fingerprint: signing::fingerprint(&key.key), key, known, name_taken })
}

/// Reads a .mdkey file for the question. Nothing is trusted until `trust_author`.
pub fn author_info(file: &Path) -> Result<AuthorInfo, String> {
    author_info_in(&sup()?.root, file)
}

pub(crate) fn trust_in(root: &Path, key: AuthorKey) -> Result<(), String> {
    let mut cfg = read_config(root);
    cfg.trusted.retain(|a| a.key != key.key);
    cfg.trusted.push(key);
    write_config(root, &cfg)
}

/// Trust an author's key, the one the question showed (the file is not read again): their signed
/// plugins run without Developer mode. Returns their name. Blocks.
pub fn trust_author(key: AuthorKey) -> Result<String, String> {
    let name = key.name.clone();
    let print = signing::fingerprint(&key.key);
    trust_in(&sup()?.root, key)?;
    log(&format!("Trusted plugin author {name} (key {print})"));
    reload(None)?;
    Ok(name)
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
    fn a_live_icon_must_be_a_small_png_inside_the_plugin() {
        let dir = std::env::temp_dir().join(format!("md-icon-test-{}", util::now_ms()));
        fs::create_dir_all(dir.join("icons")).unwrap();
        fs::write(dir.join("icons/a.png"), b"png").unwrap();
        fs::write(dir.join("icons/b.gif"), b"gif").unwrap();
        assert_eq!(read_icon(&dir, "icons/a.png"), Some(b"png".to_vec()));
        assert_eq!(read_icon(&dir, "icons/b.gif"), None);
        assert_eq!(read_icon(&dir, "../a.png"), None);
        assert_eq!(read_icon(&dir, "icons/missing.png"), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_must_stay_in_its_folder_and_speak_this_interface() {
        assert!(check_manifest(r#"{"id":"a","name":"A","api":2,"dll":"a.dll"}"#).is_ok());
        assert!(check_manifest(r#"{"id":"a","name":"A","api":1,"dll":"a.dll"}"#).unwrap_err().contains("interface"));
        assert!(check_manifest(r#"{"id":"a","name":"A","api":2,"dll":"../a.dll"}"#).unwrap_err().contains("outside"));
    }

    // the signature check is kept while nothing in the folder changed
    #[test]
    fn a_folders_trust_is_kept_until_a_file_changes() {
        let dir = std::env::temp_dir().join(format!("md-trust-cache-{}", util::now_ms()));
        fs::create_dir_all(dir.join("icons")).unwrap();
        fs::write(dir.join("a.dll"), b"one").unwrap();
        let a = folder_stamp(&dir, &[]);
        assert_eq!(a, folder_stamp(&dir, &[]));
        assert_eq!(cached_trust(&dir, &[]), Trust::Unsigned);
        fs::write(dir.join("icons").join("x.png"), b"x").unwrap();
        assert_ne!(folder_stamp(&dir, &[]), a, "a new file");
        let b = folder_stamp(&dir, &[]);
        fs::write(dir.join("a.dll"), b"longer").unwrap();
        assert_ne!(folder_stamp(&dir, &[]), b, "a changed file");
        let key = AuthorKey { name: "x".into(), key: "k".into() };
        assert_ne!(folder_stamp(&dir, &[key]), folder_stamp(&dir, &[]), "other trusted authors");
        let _ = fs::remove_dir_all(&dir);
    }

    // the DLL that runs is the copy, and the copy is what is checked against the signature
    #[test]
    fn the_copy_that_runs_is_the_one_checked() {
        let root = std::env::temp_dir().join(format!("md-shadow-test-{}", util::now_ms()));
        let (folder, run) = (root.join("plugins").join("demo"), root.join("plugin-run"));
        fs::create_dir_all(&folder).unwrap();
        let files = vec![("manifest.json".to_string(), br#"{"id":"demo","name":"Demo","version":"1.0.0","api":2,"dll":"demo.dll"}"#.to_vec()), ("demo.dll".to_string(), b"good dll".to_vec())];
        for (n, b) in &files {
            fs::write(folder.join(n), b).unwrap();
        }
        let me = signing::read_key_file(&signing::make_key(&root, "Tester", &root).unwrap()).unwrap();
        fs::write(folder.join(signing::SIG_FILE), signing::sign(&root, &files).unwrap().unwrap().1).unwrap();
        let cfg = PluginConfig { trusted: vec![me], ..Default::default() };
        // as signed: the copy is made and kept
        let copy = prepare_copy(&folder, &run, "demo", &cfg).unwrap();
        assert_eq!(fs::read(&copy).unwrap(), b"good dll");
        // a DLL swapped in after signing: refused, and no copy is left to load
        fs::write(folder.join("demo.dll"), b"evil dll").unwrap();
        assert!(prepare_copy(&folder, &run, "demo", &cfg).is_err());
        assert_eq!(fs::read_dir(&run).unwrap().count(), 1, "only the good copy from before");
        // not trusted and no Developer mode: nothing runs
        fs::write(folder.join("demo.dll"), b"good dll").unwrap();
        assert!(prepare_copy(&folder, &run, "demo", &PluginConfig::default()).unwrap_err().contains("doesn't trust"));
        // unsigned in Developer mode: its copy runs as it is
        fs::remove_file(folder.join(signing::SIG_FILE)).unwrap();
        let dev = PluginConfig { dev_mode: true, ..Default::default() };
        assert!(prepare_copy(&folder, &run, "demo", &dev).is_ok());
        assert!(prepare_copy(&folder, &run, "other", &dev).unwrap_err().contains("named"));
        let _ = fs::remove_dir_all(&root);
    }

    // opening a .mdkey only reads it: the author is trusted when the question is answered
    #[test]
    fn a_key_file_is_only_read_until_the_question_is_answered() {
        let root = std::env::temp_dir().join(format!("md-author-ask-{}", util::now_ms()));
        fs::create_dir_all(&root).unwrap();
        let built_in = signing::BUILT_IN[1].1;
        let mine = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        let file = root.join("someone.mdkey");
        let write = |name: &str, key: &str| fs::write(&file, format!(r#"{{"v":1,"name":"{name}","key":"{key}"}}"#)).unwrap();
        write("Someone", mine);
        let info = author_info_in(&root, &file).unwrap();
        assert_eq!((info.key.name.as_str(), info.known, info.name_taken), ("Someone", false, false));
        assert_eq!(info.fingerprint, signing::fingerprint(mine));
        assert!(read_config(&root).trusted.is_empty(), "reading the file trusts nobody");
        trust_in(&root, info.key.clone()).unwrap();
        trust_in(&root, info.key).unwrap();
        assert_eq!(read_config(&root).trusted.len(), 1, "trusted once, however often it is added");
        assert!(author_info_in(&root, &file).unwrap().known);
        // a built-in key is known; a built-in name with another key is flagged
        write("x", built_in);
        assert!(author_info_in(&root, &file).unwrap().known);
        write(" Minnion ", mine);
        assert!(author_info_in(&root, &file).unwrap().name_taken);
        write("minnion", built_in);
        assert!(!author_info_in(&root, &file).unwrap().name_taken);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_pack_installs_inside_its_folder_only() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("md-install-test-{}", util::now_ms()));
        fs::create_dir_all(&root).unwrap();
        let pack = |name: &str, files: &[(&str, &[u8])]| {
            let path = root.join(name);
            let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
            for (n, b) in files {
                z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(b).unwrap();
            }
            z.finish().unwrap();
            path
        };
        let manifest: &[u8] = br#"{"id":"demo","name":"Demo","version":"1.0.0","api":2,"dll":"demo.dll"}"#;
        let good = pack("good.mdplugin", &[("manifest.json", manifest), ("demo.dll", b"dll"), ("icons/a.png", b"png")]);
        let (m, dest, trust) = install_file(&root, &good).unwrap();
        assert_eq!((m.id.as_str(), m.version.as_str()), ("demo", "1.0.0"));
        assert!(dest.join("demo.dll").is_file() && dest.join("icons/a.png").is_file());
        assert_eq!(trust, Trust::Unsigned);
        // installing again replaces it
        install_file(&root, &good).unwrap();
        assert!(!root.join("plugins").read_dir().unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with('.')));
        // a file outside the folder, a missing DLL and a wrong interface are refused, and nothing is left behind
        let escape = pack("escape.mdplugin", &[("manifest.json", manifest), ("../evil.dll", b"x")]);
        assert!(install_file(&root, &escape).unwrap_err().contains("outside"));
        let nodll = pack("nodll.mdplugin", &[("manifest.json", manifest)]);
        assert!(install_file(&root, &nodll).unwrap_err().contains("no demo.dll"));
        let old: &[u8] = br#"{"id":"demo","name":"Demo","version":"1.0.0","api":1,"dll":"demo.dll"}"#;
        let wrong = pack("wrong.mdplugin", &[("manifest.json", old), ("demo.dll", b"dll")]);
        assert!(install_file(&root, &wrong).unwrap_err().contains("interface"));
        assert!(!root.join("evil.dll").exists());
        assert!(root.join("plugins").join("demo").join("demo.dll").is_file(), "the installed one stays");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_pack_is_looked_into_before_it_is_installed() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("md-inspect-test-{}", util::now_ms()));
        fs::create_dir_all(&root).unwrap();
        let manifest = br#"{"id":"demo","name":"Demo","version":"1.2.0","api":2,"dll":"demo.dll"}"#;
        let path = root.join("demo.mdplugin");
        let mut z = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (n, b) in [("manifest.json", &manifest[..]), ("demo.dll", b"dll"), ("actions.json", br#"[{"id":"a"},{"id":"b"}]"#)] {
            z.start_file(n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap();
        // unsigned: it can be installed but won't run without Developer mode
        let info = inspect_in(&root, &path).unwrap();
        assert_eq!((info.id.as_str(), info.name.as_str(), info.version.as_str(), info.actions), ("demo", "Demo", "1.2.0", 2));
        assert_eq!(info.signature, Trust::Unsigned);
        assert!(!info.runs && info.why.as_deref().unwrap_or("").contains("Developer mode"));
        assert_eq!(info.installed, None);
        // nothing is left behind by looking
        assert!(root.join("plugins").read_dir().unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.')));
        // with Developer mode on it runs; once installed the version it replaces is shown
        // an earlier version's plain plugins.json is taken over (and sealed)
        fs::write(root.join("plugins.json"), r#"{"devMode":true}"#).unwrap();
        install_file(&root, &path).unwrap();
        assert!(!root.join("plugins.json").exists() && root.join("plugins.mdsave").is_file(), "sealed, the plain file gone");
        let again = inspect_in(&root, &path).unwrap();
        assert!(again.runs && again.why.is_none());
        assert_eq!(again.installed.as_deref(), Some("1.2.0"));
        assert!(!again.downgrade() && again.key.is_none(), "the same version again; unsigned");
        let older = PackInfo { version: "1.1.9".into(), ..again.clone() };
        assert!(older.downgrade());
        // a file that isn't a pack is explained
        fs::write(root.join("notes.mdplugin"), b"hello").unwrap();
        assert!(inspect_in(&root, &root.join("notes.mdplugin")).unwrap_err().contains("isn't a .mdplugin"));
        // Install takes the file that was looked at, not one put in its place since
        assert_eq!(again.sha, util::sha256_hex(&fs::read(&path).unwrap()));
        assert!(install_checked(&root, &path, &again.sha).is_ok());
        fs::write(&path, b"hello").unwrap();
        assert!(install_checked(&root, &path, &again.sha).unwrap_err().contains("changed after"));
        let _ = fs::remove_dir_all(&root);
    }
}
