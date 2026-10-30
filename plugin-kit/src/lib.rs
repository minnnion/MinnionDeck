//! # minniondeck-plugin
//!
//! Write a Minniondeck plugin in safe Rust. A plugin is a DLL (`crate-type = ["cdylib"]`) with
//! one type that implements [`Plugin`], exported with [`export!`]:
//!
//! ```ignore
//! use minniondeck_plugin::{export, ButtonEvent, Host, Outcome, Plugin};
//!
//! struct Hello { host: Host }
//!
//! impl Plugin for Hello {
//!     fn start(host: Host) -> Self { Hello { host } }
//!     fn press(&mut self, e: &ButtonEvent) -> Outcome {
//!         self.host.set_title(&e.button, "Pressed!");
//!         Ok("Said hello".into())
//!     }
//! }
//!
//! export!(Hello);
//! ```
//!
//! Minniondeck runs plugins in a helper process next to the app, so a crash restarts the plugins
//! and never takes the deck down. Each plugin gets its own thread: Minniondeck calls your methods
//! one at a time, in order. A method that takes long holds up that plugin's other events, so do
//! slow work (network, waiting) on your own threads and report back through [`Host`].
//!
//! [`Host`] can be cloned and used from any thread, at any time, until [`Plugin::stop`] returns.
//! `stop` must end any threads you started.
//!
//! ## Settings
//!
//! List settings in manifest.json under "settings" and Minniondeck shows them in Settings ›
//! Plugins (kinds: text, password, number, switch, select, button). Read them with
//! [`Host::settings`] / [`Host::setting`]; [`Plugin::settings_changed`] runs when the user saves.
//! Passwords are encrypted by Windows on disk and reach you as plain text. A "button" field calls
//! [`Plugin::settings_button`], e.g. to test a connection.
//!
//! ## The C interface (version 2)
//!
//! [`export!`] writes these for you. They are listed so the format is documented in one place:
//!
//! ```text
//! md_plugin_api() -> u32                                    2
//! md_plugin_start(host: *const HostApi, init_json) -> *mut c_void
//!     init_json: {"id":"…","dir":"<plugin folder>","settings":{…}}
//! md_plugin_call(instance, msg_json) -> *mut c_char        reply JSON, freed with md_plugin_free
//!     {"t":"press"|"long_press","action","button","state","fields"} -> {"ok":bool,"message":"…"}
//!     {"t":"release"|"shown"|"hidden", …same…}                         -> {"ok":true}
//!     {"t":"view", …same…, "data":{…}}                                  -> {"ok":true}
//!     {"t":"turn", …same…, "data":{"by":clicks}}                        -> {"ok":true}  (a knob)
//!     {"t":"options","action","field","fields"}                        -> {"options":[{"value","label","states"?}]}
//!         states (optional): [{"name","icon","text","ar"}], what the button becomes when it's chosen
//!     {"t":"settings","settings":{…}}                                  -> {"ok":true}
//!     {"t":"settings_button","key"}                                    -> {"ok":bool,"message":"…"}
//! md_plugin_free(text)
//! md_plugin_stop(instance)
//! ```
//!
//! Messages a plugin sends through `HostApi.send` (JSON):
//!
//! ```text
//! {"t":"set_state","button","state":1}       {"t":"set_title","button","title":"…"|null}
//! {"t":"set_icon","button","icon":"icons/x.png"|null}
//! {"t":"log","text"}   {"t":"toast","text"}   {"t":"set_setting","key","value"}
//! {"t":"refresh_options","action"}
//! {"t":"set_data","button","data":{…}}      live data a button draws itself (such as a map)
//! {"t":"speak","text","voice"}               Minniondeck says it aloud (needs minnion_speech)
//! ```

use std::ffi::{CStr, CString, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub use serde_json;
use serde_json::{Value, json};

/// The plugin interface version this kit speaks
pub const API_VERSION: u32 = 2;

/// What Minniondeck hands a plugin when it starts. Plugins use [`Host`] instead.
#[repr(C)]
pub struct HostApi {
    pub api: u32,
    pub ctx: *mut c_void,
    pub send: unsafe extern "C" fn(ctx: *mut c_void, msg: *const c_char),
}

/// A button on the deck (or a phone) that uses one of your actions
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Button {
    pub id: String,
}

/// A button event: pressed, released, long-pressed, shown or hidden
#[derive(Clone, Debug)]
pub struct ButtonEvent {
    /// Your action's id from actions.json
    pub action: String,
    pub button: Button,
    /// The button's current state (0 is the first)
    pub state: usize,
    /// The values the user set in the Edit button window
    pub fields: Value,
}

impl ButtonEvent {
    /// A field's text, trimmed; empty when it isn't set
    pub fn field(&self, key: &str) -> &str {
        self.fields.get(key).and_then(|v| v.as_str()).unwrap_or("").trim()
    }
}

/// The editor asking for a dropdown's choices (a field with `"kind":"select","dynamic":true`)
#[derive(Clone, Debug)]
pub struct OptionsQuery {
    pub action: String,
    pub field: String,
    /// The other fields' current values
    pub fields: Value,
}

/// One dropdown choice. It can bring the button's states with it: when it's chosen in the Edit
/// button window, the button gets these states (each with its icon, a file listed in your
/// manifest's "icons", and its text), and you switch between them with `Host::set_state`.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub states: Vec<ChoiceState>,
}

/// A state a choice gives the button: its name, icon (a file in your plugin folder, listed in the
/// manifest's "icons"), the text shown on it ("{live}" shows your live title there, see
/// `Host::set_title`) and the icon's width / height (`ar`, 1 for a square), so it keeps its shape
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ChoiceState {
    pub name: String,
    pub icon: String,
    pub text: String,
    pub ar: f64,
}

impl Choice {
    /// A choice shown as its value
    pub fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        Choice { label: value.clone(), value, states: Vec::new() }
    }
    /// A choice with a different label than its value
    pub fn labelled(value: impl Into<String>, label: impl Into<String>) -> Self {
        Choice { value: value.into(), label: label.into(), states: Vec::new() }
    }
    /// The states the button gets when this is chosen (up to 8)
    pub fn with_states(mut self, states: Vec<ChoiceState>) -> Self {
        self.states = states;
        self
    }
}

/// What a press did: `Ok` with a short message for the user, or `Err` saying what went wrong
pub type Outcome = Result<String, String>;

/// Your plugin. Only `start` and `press` are required.
pub trait Plugin: Send + 'static {
    /// Called once when Minniondeck loads the plugin
    fn start(host: Host) -> Self
    where
        Self: Sized;
    /// A button with one of your actions was pressed
    fn press(&mut self, event: &ButtonEvent) -> Outcome;
    /// The button was let go (after `press`)
    fn release(&mut self, _event: &ButtonEvent) {}
    /// The button was held down; by default this is a normal press
    fn long_press(&mut self, event: &ButtonEvent) -> Outcome {
        self.press(event)
    }
    /// The button came on screen (its page opened); update its look here
    fn shown(&mut self, _event: &ButtonEvent) {}
    /// The button left the screen
    fn hidden(&mut self, _event: &ButtonEvent) {}
    /// A live map on the button (see `Host::set_data`) is looking at something, such as
    /// {"airport":"KATL"} when it's zoomed in on an airport; send what's there with `set_data`
    fn view(&mut self, _event: &ButtonEvent, _data: &Value) {}
    /// A knob button (an action with "knob": true in actions.json) was turned: `by` clicks, more
    /// than 0 clockwise. The mouse wheel turns it on the PC, a drag on a phone.
    fn turn(&mut self, _event: &ButtonEvent, _by: i32) {}
    /// Choices for a dynamic dropdown in the Edit button window
    fn options(&mut self, _query: &OptionsQuery) -> Vec<Choice> {
        Vec::new()
    }
    /// The user changed your settings in Settings › Plugins
    fn settings_changed(&mut self, _settings: &Value) {}
    /// A "button" field in your settings was pressed; the message is shown next to it
    fn settings_button(&mut self, key: &str) -> Outcome {
        Err(format!("This plugin doesn't handle the \"{key}\" button."))
    }
    /// Minniondeck is unloading the plugin: end your threads and close connections
    fn stop(&mut self) {}
}

struct HostInner {
    ctx: usize,
    send: unsafe extern "C" fn(*mut c_void, *const c_char),
    id: String,
    dir: PathBuf,
    settings: Mutex<Value>,
}

/// Talk to Minniondeck. Cheap to clone; use it from any thread.
#[derive(Clone)]
pub struct Host {
    inner: Arc<HostInner>,
}

impl Host {
    fn send(&self, msg: Value) {
        if let Ok(text) = CString::new(msg.to_string()) {
            // The host's send function is thread-safe and copies the text before returning
            unsafe { (self.inner.send)(self.inner.ctx as *mut c_void, text.as_ptr()) }
        }
    }
    /// Your plugin's id from manifest.json
    pub fn id(&self) -> &str {
        &self.inner.id
    }
    /// Your plugin's installed folder (icons, GIFs and other files are here)
    pub fn folder(&self) -> &Path {
        &self.inner.dir
    }
    /// Switch a button to one of its states (0 is the first)
    pub fn set_state(&self, button: &Button, state: usize) {
        self.send(json!({ "t": "set_state", "button": button.id, "state": state }));
    }
    /// Show this text on a button instead of its own (until cleared or Minniondeck restarts)
    pub fn set_title(&self, button: &Button, title: impl Into<String>) {
        self.send(json!({ "t": "set_title", "button": button.id, "title": title.into() }));
    }
    /// Go back to the button's own text
    pub fn clear_title(&self, button: &Button) {
        self.send(json!({ "t": "set_title", "button": button.id, "title": null }));
    }
    /// Show a picture from your plugin folder on a button, e.g. `"icons/live.png"`
    pub fn set_icon(&self, button: &Button, file: &str) {
        self.send(json!({ "t": "set_icon", "button": button.id, "icon": file }));
    }
    /// Go back to the button's own icon
    pub fn clear_icon(&self, button: &Button) {
        self.send(json!({ "t": "set_icon", "button": button.id, "icon": null }));
    }
    /// Write a line to Minniondeck's log file
    pub fn log(&self, text: impl Into<String>) {
        self.send(json!({ "t": "log", "text": text.into() }));
    }
    /// Show a short message at the bottom of the Minniondeck window
    pub fn toast(&self, text: impl Into<String>) {
        self.send(json!({ "t": "toast", "text": text.into() }));
    }
    /// All your settings (what the user set in Settings › Plugins, plus what you saved)
    pub fn settings(&self) -> Value {
        self.inner.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }
    /// One setting
    pub fn setting(&self, key: &str) -> Option<Value> {
        self.inner.settings.lock().ok().and_then(|s| s.get(key).cloned())
    }
    /// Save a setting (kept across restarts), e.g. a token you refreshed
    pub fn set_setting(&self, key: &str, value: Value) {
        if let Ok(mut s) = self.inner.settings.lock() {
            if !s.is_object() {
                *s = json!({});
            }
            s[key] = value.clone();
        }
        self.send(json!({ "t": "set_setting", "key": key, "value": value }));
    }
    /// Ask the editor to fetch an action's dropdown choices again (e.g. OBS scenes changed)
    pub fn refresh_options(&self, action: &str) {
        self.send(json!({ "t": "refresh_options", "action": action }));
    }
    /// Send live data a button draws itself, as a JSON object. `{"kind":"map", …}` draws a map
    /// (see the MSFS 2024 plugin). Each call changes only the keys it sends; the others stay as
    /// they were, so send what changes often (a position) apart from what changes rarely (a route).
    /// `null` for the whole object clears it. Phones get it too.
    pub fn set_data(&self, button: &Button, data: Value) {
        self.send(json!({ "t": "set_data", "button": button.id, "data": data }));
    }
    /// Ask Minniondeck to say this aloud, in the speech model's own voice. Minniondeck runs the
    /// model (it comes with the minnion_speech plugin) and speaks lines one after another; if
    /// the model isn't installed it says so in a message instead. At most 500 characters.
    pub fn speak(&self, text: impl Into<String>) {
        self.speak_as(text, "");
    }
    /// Like [`Host::speak`], in one of the voices: its id or name, such as "Alfie"
    pub fn speak_as(&self, text: impl Into<String>, voice: &str) {
        self.send(json!({ "t": "speak", "text": text.into(), "voice": voice }));
    }
}

#[doc(hidden)]
pub mod __private {
    use super::*;

    pub struct Instance {
        plugin: Box<dyn Plugin>,
        host: Host,
    }

    fn text(p: *const c_char) -> String {
        if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
    }

    fn reply(v: Value) -> *mut c_char {
        CString::new(v.to_string()).unwrap_or_default().into_raw()
    }

    fn panic_text(e: Box<dyn std::any::Any + Send>) -> String {
        let why = e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned()).unwrap_or_default();
        if why.is_empty() { "The plugin stopped unexpectedly.".into() } else { format!("The plugin stopped unexpectedly: {why}") }
    }

    /// # Safety
    /// `host` must point to a valid HostApi; `init` must be a NUL-terminated string or null.
    pub unsafe fn start<P: Plugin>(host: *const HostApi, init: *const c_char) -> *mut c_void {
        if host.is_null() {
            return std::ptr::null_mut();
        }
        let api = unsafe { &*host };
        let init: Value = serde_json::from_str(&text(init)).unwrap_or_default();
        let host = Host {
            inner: Arc::new(HostInner {
                ctx: api.ctx as usize,
                send: api.send,
                id: init.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                dir: PathBuf::from(init.get("dir").and_then(|v| v.as_str()).unwrap_or("")),
                settings: Mutex::new(init.get("settings").cloned().filter(|s| s.is_object()).unwrap_or_else(|| json!({}))),
            }),
        };
        let h = host.clone();
        match catch_unwind(AssertUnwindSafe(|| P::start(h))) {
            Ok(p) => Box::into_raw(Box::new(Instance { plugin: Box::new(p), host })) as *mut c_void,
            Err(e) => {
                host.log(panic_text(e));
                std::ptr::null_mut()
            }
        }
    }

    fn event(msg: &Value) -> ButtonEvent {
        let s = |k: &str| msg.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        ButtonEvent {
            action: s("action"),
            button: Button { id: s("button") },
            state: msg.get("state").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
            fields: msg.get("fields").cloned().unwrap_or_else(|| json!({})),
        }
    }

    fn outcome(r: Outcome) -> Value {
        match r {
            Ok(message) => json!({ "ok": true, "message": message }),
            Err(message) => json!({ "ok": false, "message": message }),
        }
    }

    /// # Safety
    /// `inst` must come from `start`; `msg` must be a NUL-terminated string.
    pub unsafe fn call(inst: *mut c_void, msg: *const c_char) -> *mut c_char {
        if inst.is_null() {
            return reply(json!({ "ok": false, "message": "The plugin didn't start." }));
        }
        let inst = unsafe { &mut *(inst as *mut Instance) };
        let msg: Value = serde_json::from_str(&text(msg)).unwrap_or_default();
        let kind = msg.get("t").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let p = &mut inst.plugin;
            match kind.as_str() {
                "press" => outcome(p.press(&event(&msg))),
                "long_press" => outcome(p.long_press(&event(&msg))),
                "release" => {
                    p.release(&event(&msg));
                    json!({ "ok": true })
                }
                "shown" => {
                    p.shown(&event(&msg));
                    json!({ "ok": true })
                }
                "hidden" => {
                    p.hidden(&event(&msg));
                    json!({ "ok": true })
                }
                "view" => {
                    p.view(&event(&msg), msg.get("data").unwrap_or(&Value::Null));
                    json!({ "ok": true })
                }
                "turn" => {
                    let by = msg.get("data").and_then(|d| d.get("by")).and_then(|v| v.as_i64()).unwrap_or(0).clamp(-50, 50) as i32;
                    if by != 0 {
                        p.turn(&event(&msg), by);
                    }
                    json!({ "ok": true })
                }
                "options" => {
                    let s = |k: &str| msg.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let q = OptionsQuery { action: s("action"), field: s("field"), fields: msg.get("fields").cloned().unwrap_or_else(|| json!({})) };
                    let list: Vec<Value> = p
                        .options(&q)
                        .into_iter()
                        .map(|c| {
                            let mut o = json!({ "value": c.value, "label": c.label });
                            if !c.states.is_empty() {
                                o["states"] = c.states.iter().map(|s| json!({ "name": s.name, "icon": s.icon, "text": s.text, "ar": if s.ar > 0.0 { s.ar } else { 1.0 } })).collect();
                            }
                            o
                        })
                        .collect();
                    json!({ "options": list })
                }
                "settings" => {
                    let s = msg.get("settings").cloned().filter(|s| s.is_object()).unwrap_or_else(|| json!({}));
                    if let Ok(mut cur) = inst.host.inner.settings.lock() {
                        *cur = s.clone();
                    }
                    p.settings_changed(&s);
                    json!({ "ok": true })
                }
                "settings_button" => {
                    let key = msg.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    outcome(p.settings_button(&key))
                }
                other => json!({ "ok": false, "message": format!("This plugin doesn't understand \"{other}\". Update the plugin.") }),
            }
        }));
        reply(result.unwrap_or_else(|e| json!({ "ok": false, "message": panic_text(e) })))
    }

    /// # Safety
    /// `text` must come from `call`, freed once.
    pub unsafe fn free(text: *mut c_char) {
        if !text.is_null() {
            drop(unsafe { CString::from_raw(text) });
        }
    }

    /// # Safety
    /// `inst` must come from `start`, stopped once.
    pub unsafe fn stop(inst: *mut c_void) {
        if inst.is_null() {
            return;
        }
        let mut inst = unsafe { Box::from_raw(inst as *mut Instance) };
        let _ = catch_unwind(AssertUnwindSafe(|| inst.plugin.stop()));
    }
}

/// Export your plugin type as a Minniondeck plugin: `export!(MyPlugin);`
#[macro_export]
macro_rules! export {
    ($plugin:ty) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn md_plugin_api() -> u32 {
            $crate::API_VERSION
        }
        /// # Safety
        /// Called by Minniondeck only.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn md_plugin_start(host: *const $crate::HostApi, init: *const ::std::ffi::c_char) -> *mut ::std::ffi::c_void {
            unsafe { $crate::__private::start::<$plugin>(host, init) }
        }
        /// # Safety
        /// Called by Minniondeck only.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn md_plugin_call(inst: *mut ::std::ffi::c_void, msg: *const ::std::ffi::c_char) -> *mut ::std::ffi::c_char {
            unsafe { $crate::__private::call(inst, msg) }
        }
        /// # Safety
        /// Called by Minniondeck only.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn md_plugin_free(text: *mut ::std::ffi::c_char) {
            unsafe { $crate::__private::free(text) }
        }
        /// # Safety
        /// Called by Minniondeck only.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn md_plugin_stop(inst: *mut ::std::ffi::c_void) {
            unsafe { $crate::__private::stop(inst) }
        }
    };
}
