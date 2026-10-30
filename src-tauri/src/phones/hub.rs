// Paired phones, which of them are connected, the pairing session, and keeping phones in sync.
//
// Paired phones are saved in devices.json. The PC keeps only a hash of each phone's token,
// so the file alone can't be used to pose as a phone. Removing a phone deletes its entry,
// which stops its token working at once.
//
// Sync: the hub holds the latest saved library and Settings and the page open on the PC.
// Whenever one of those (or a phone's own settings) changes, each connected phone whose layout
// changed gets it again; one whose layout is the same only hears about a page change.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;

use super::pairing::{self, CODE_LIFETIME_MS, MAX_TRIES, Qr};
use super::protocol::{DeviceInfo, Outgoing};
use super::sync::{self, Sources};
use crate::storage::{SCHEMA_VERSION, now_ms, safe_name, sha256_hex, write_atomic};

/// A field missing from devices.json (saved by an older or newer version) takes its default, so
/// an update never drops a paired phone: that made the phone pair again.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct PhoneRecord {
    pub id: String,
    pub device_id: String,
    pub name: String,
    pub kind: String,
    pub os: String,
    pub app_version: String,
    pub aspect: f64,
    pub paired_ms: u64,
    pub last_seen_ms: u64,
    pub token_hash: String,
    // How the phone shows pages, set on the Phones page
    pub follows: String,
    pub orient: String,
    pub fit: String,
    pub grid: String,
}

impl Default for PhoneRecord {
    fn default() -> Self {
        PhoneRecord {
            id: String::new(),
            device_id: String::new(),
            name: "Phone".into(),
            kind: "phone".into(),
            os: String::new(),
            app_version: String::new(),
            aspect: 2.0,
            paired_ms: 0,
            last_seen_ms: 0,
            token_hash: String::new(),
            follows: "pc".into(),
            orient: "portrait".into(),
            fit: "turn".into(),
            grid: "pc".into(),
        }
    }
}

/// What the Phones page shows: the saved phone plus whether it is connected now
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PhoneInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub os: String,
    pub app_version: String,
    pub aspect: f64,
    pub paired_ms: u64,
    pub last_seen_ms: u64,
    pub follows: String,
    pub orient: String,
    pub fit: String,
    pub grid: String,
    pub online: bool,
    pub battery: Option<u8>,
    pub address: Option<String>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PhonePatch {
    pub name: Option<String>,
    pub follows: Option<String>,
    pub orient: Option<String>,
    pub fit: Option<String>,
    pub grid: Option<String>,
}

/// Something for a connected phone's connection to do
pub enum Cmd {
    Send(String),
    /// A binary frame: a gzipped message (a layout, for a phone that takes them)
    Binary(Vec<u8>),
    Close(Outgoing),
}

struct Live {
    conn: u64,
    battery: Option<u8>,
    address: String,
    tx: UnboundedSender<Cmd>,
    /// The layout and page this phone has, so only changes are sent
    sent_rev: Option<String>,
    sent_page: Option<String>,
    /// Layouts the phone keeps (from hello, and each sent since), so switching back to a profile
    /// is a "known" message instead of the whole layout again
    has: HashSet<String>,
    /// It takes layouts gzipped
    gz: bool,
    /// What plugins show on its buttons has been sent on this connection. A phone that comes
    /// back with the layout it had still needs it: it may have lost it (an app restart), and
    /// what a plugin sends only when it changes (an OFP, a station's name) isn't sent again.
    live_sent: bool,
    /// What its layout lets it press and download
    buttons: HashSet<String>,
    media: HashSet<String>,
    /// The page it shows now (the phone says so when it turns a page)
    viewing: Option<String>,
}

struct Session {
    code: String,
    expires_ms: u64,
    tries: u8,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PairingInfo {
    pub code: String,
    pub expires_ms: u64,
    pub qr: Qr,
    pub link: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub pc_name: String,
}

/// Why the pairing code stopped: paired, expired, too_many_tries or cancelled
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingEvent {
    pub reason: String,
    pub phone: Option<String>,
}

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub listening: bool,
    pub port: u16,
    pub error: Option<String>,
}

/// A button pressed on a phone, for the UI to run
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PressEvent {
    pub phone_id: String,
    pub phone_name: String,
    pub button_id: String,
    /// "press", "long" or "release"
    pub kind: String,
}

/// What a live map on a phone is looking at, for the UI to pass to the button's plugin
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ViewEvent {
    pub phone_id: String,
    pub button_id: String,
    pub data: Value,
}

/// A knob button turned on a phone, for the UI to pass to the button's plugin
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TurnEvent {
    pub phone_id: String,
    pub button_id: String,
    pub by: i32,
}

pub enum Event {
    Phones(Vec<PhoneInfo>),
    Pairing(PairingEvent),
    Press(PressEvent),
    View(ViewEvent),
    Turn(TurnEvent),
    /// The pages the connected phones show, so their buttons' plugins send live values too
    Pages(Vec<String>),
    /// A phone turned to a page: the PC opens it too, so the two stay together
    PhonePage(PhonePageEvent),
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PhonePageEvent {
    pub phone_id: String,
    pub page: String,
}

pub type Notify = Box<dyn Fn(Event) + Send + Sync>;

/// A refusal sent to the phone: (error code, message)
pub type Refusal = (&'static str, &'static str);

pub struct Hub {
    file: PathBuf,
    records: Mutex<Vec<PhoneRecord>>,
    live: Mutex<HashMap<String, Live>>,
    session: Mutex<Option<Session>>,
    status: Mutex<ServerStatus>,
    next_conn: AtomicU64,
    sources: Mutex<Sources>,
    media_dir: PathBuf,
    media_cache: Mutex<HashMap<String, (u64, String)>>,
    /// What plugins show on buttons right now: button id → (title, icon)
    button_live: Mutex<HashMap<String, (Option<String>, Option<String>)>>,
    /// Live data plugins send for buttons that draw it themselves (a map): button id → object
    button_data: Mutex<HashMap<String, Value>>,
    /// The state each button's plugin put it in, sent to phones as it changes
    button_state: Mutex<HashMap<String, u64>>,
    pub fingerprint: String,
    pub pc_name: String,
    notify: Notify,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn clip(s: &str, max: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(max).collect()
}

fn valid_grid(g: &str) -> bool {
    g == "pc"
        || g.split_once('x').is_some_and(|(a, b)| matches!((a.parse::<u8>(), b.parse::<u8>()), (Ok(a), Ok(b)) if (1..=12).contains(&a) && (1..=12).contains(&b)))
}

impl Hub {
    /// Load paired phones. Entries without a token (such as the example phones of earlier
    /// versions) are dropped, and the file is rewritten without them. A file or entry that can't
    /// be read is copied to devices.unreadable.json first and logged, so no phone goes unseen.
    pub fn load(file: PathBuf, media_dir: PathBuf, fingerprint: String, pc_name: String, notify: Notify) -> Self {
        let (mut dropped, mut unreadable) = (false, false);
        let saved = fs::read_to_string(&file).ok().map(|t| serde_json::from_str::<Value>(&t));
        let list = match saved {
            None => Vec::new(),
            Some(Ok(v)) => v["data"]["phones"].as_array().cloned().unwrap_or_default(),
            Some(Err(e)) => {
                crate::applog::line(&format!("Paired phones could not be read: {e}"));
                unreadable = true;
                Vec::new()
            }
        };
        let records: Vec<PhoneRecord> = list
            .into_iter()
            .filter_map(|p| match serde_json::from_value::<PhoneRecord>(p) {
                Ok(r) if r.token_hash.len() == 64 => Some(r),
                Ok(_) => {
                    dropped = true;
                    None
                }
                Err(e) => {
                    crate::applog::line(&format!("A paired phone could not be read: {e}"));
                    unreadable = true;
                    None
                }
            })
            .collect();
        if unreadable {
            let _ = fs::copy(&file, file.with_extension("unreadable.json"));
        }
        let hub = Hub {
            file,
            records: Mutex::new(records),
            live: Mutex::new(HashMap::new()),
            session: Mutex::new(None),
            status: Mutex::new(ServerStatus::default()),
            next_conn: AtomicU64::new(1),
            sources: Mutex::new(Sources::default()),
            media_dir,
            media_cache: Mutex::new(HashMap::new()),
            button_live: Mutex::new(HashMap::new()),
            button_data: Mutex::new(HashMap::new()),
            button_state: Mutex::new(HashMap::new()),
            fingerprint,
            pc_name,
            notify,
        };
        if dropped || unreadable {
            hub.save();
        }
        crate::applog::line(&format!("Paired phones: {}", lock(&hub.records).len()));
        hub
    }

    fn save(&self) {
        let phones = lock(&self.records).clone();
        let body = json!({ "schema_version": SCHEMA_VERSION, "saved_at": now_ms(), "data": { "phones": phones } });
        if let Err(e) = write_atomic(&self.file, body.to_string().as_bytes()) {
            crate::applog::line(&format!("Could not save paired phones: {e}"));
        }
    }

    // Callers must not hold any lock here: listeners read the list back
    fn changed(&self) {
        (self.notify)(Event::Phones(self.list()));
    }

    fn pairing_ended(&self, reason: &str, phone: Option<String>) {
        (self.notify)(Event::Pairing(PairingEvent { reason: reason.into(), phone }));
    }

    pub fn list(&self) -> Vec<PhoneInfo> {
        let records = lock(&self.records);
        let live = lock(&self.live);
        records
            .iter()
            .map(|r| {
                let l = live.get(&r.id);
                PhoneInfo {
                    id: r.id.clone(),
                    name: r.name.clone(),
                    kind: r.kind.clone(),
                    os: r.os.clone(),
                    app_version: r.app_version.clone(),
                    aspect: r.aspect,
                    paired_ms: r.paired_ms,
                    last_seen_ms: r.last_seen_ms,
                    follows: r.follows.clone(),
                    orient: r.orient.clone(),
                    fit: r.fit.clone(),
                    grid: r.grid.clone(),
                    online: l.is_some(),
                    battery: l.and_then(|l| l.battery),
                    address: l.map(|l| l.address.clone()),
                }
            })
            .collect()
    }

    pub fn status(&self) -> ServerStatus {
        lock(&self.status).clone()
    }

    pub fn set_status(&self, listening: bool, port: u16, error: Option<String>) {
        *lock(&self.status) = ServerStatus { listening, port, error };
    }

    // ----- Pairing -----

    /// A fresh code that works once, for 5 minutes. Any earlier code stops working.
    pub fn start_pairing(&self) -> Result<PairingInfo, String> {
        let code = pairing::new_code();
        let expires_ms = now_ms() + CODE_LIFETIME_MS;
        *lock(&self.session) = Some(Session { code: code.clone(), expires_ms, tries: 0 });
        let port = self.status().port;
        let addresses = pairing::lan_addresses();
        let link = pairing::link(&addresses, port, &code, &self.fingerprint, &self.pc_name);
        Ok(PairingInfo { qr: pairing::qr(&link)?, code, expires_ms, link, addresses, port, pc_name: self.pc_name.clone() })
    }

    pub fn cancel_pairing(&self) {
        if lock(&self.session).take().is_some() {
            self.pairing_ended("cancelled", None);
        }
    }

    /// A new phone sent a code. On success it is saved and gets its token (shown to it once).
    pub fn try_pair(&self, code: &str, device: DeviceInfo) -> Result<(PhoneRecord, String), Refusal> {
        {
            let mut session = lock(&self.session);
            let Some(s) = session.as_mut() else {
                return Err(("no_pairing", "Pairing is not open on the PC. Open Phones in Minniondeck to get a code."));
            };
            if now_ms() > s.expires_ms {
                *session = None;
                drop(session);
                self.pairing_ended("expired", None);
                return Err(("expired", "That code has expired. Get a new one on the PC."));
            }
            if pairing::digits(code) != s.code {
                s.tries += 1;
                if s.tries >= MAX_TRIES {
                    *session = None;
                    drop(session);
                    self.pairing_ended("too_many_tries", None);
                    return Err(("too_many_tries", "Too many wrong codes. Get a new one on the PC."));
                }
                return Err(("bad_code", "That code is not right. Check the number on the PC."));
            }
            *session = None;
        }
        let device_id = clip(&device.device_id, 100);
        if device_id.is_empty() {
            return Err(("bad_request", "The phone did not say who it is."));
        }
        let kind = if device.kind == "tablet" { "tablet" } else { "phone" };
        let name = Some(clip(&device.name, 40)).filter(|n| !n.is_empty()).unwrap_or_else(|| if kind == "tablet" { "Tablet".into() } else { "Phone".into() });
        let token = pairing::new_token();
        let now = now_ms();
        let record = PhoneRecord {
            id: pairing::new_id(),
            device_id: device_id.clone(),
            name: name.clone(),
            kind: kind.into(),
            os: clip(&device.os, 40),
            app_version: clip(&device.app_version, 20),
            aspect: if device.aspect.is_finite() && device.aspect >= 1.0 { device.aspect.min(3.0) } else { 2.0 },
            paired_ms: now,
            last_seen_ms: now,
            token_hash: sha256_hex(token.as_bytes()),
            follows: "pc".into(),
            orient: if kind == "tablet" { "landscape".into() } else { "portrait".into() },
            fit: "turn".into(),
            grid: "pc".into(),
        };
        // Pairing the same phone again replaces its old entry (and ends its old connection)
        let replaced: Vec<String> = {
            let mut records = lock(&self.records);
            let old: Vec<String> = records.iter().filter(|r| r.device_id == device_id).map(|r| r.id.clone()).collect();
            records.retain(|r| r.device_id != device_id);
            records.push(record.clone());
            old
        };
        for id in replaced {
            self.drop_live(&id, Outgoing::error("replaced", "This phone was paired again."));
        }
        self.save();
        crate::applog::line(&format!("Phone paired: {name}"));
        self.pairing_ended("paired", Some(name));
        self.changed();
        Ok((record, token))
    }

    /// A paired phone came back. Its token must match the hash kept for it.
    pub fn check_hello(&self, phone_id: &str, token: &str) -> Result<PhoneRecord, Refusal> {
        let records = lock(&self.records);
        let r = records.iter().find(|r| r.id == phone_id).ok_or(("unknown_phone", "This phone is not paired with this PC. Pair it again."))?;
        if sha256_hex(token.as_bytes()) != r.token_hash {
            return Err(("bad_token", "This phone's key is not right. Pair it again."));
        }
        Ok(r.clone())
    }

    // ----- Connections -----

    /// A phone is connected. Only one connection per phone: an older one is closed.
    /// `has_rev` is the layout it already has (from hello), so it isn't sent again.
    /// `has` are the layouts it keeps, `gz` whether it takes them gzipped.
    pub fn connect(&self, phone_id: &str, address: String, has_rev: Option<String>, has: Vec<String>, gz: bool, tx: UnboundedSender<Cmd>) -> u64 {
        let conn = self.next_conn.fetch_add(1, Ordering::SeqCst);
        let has = has.into_iter().filter(|r| r.len() <= 64).take(32).chain(has_rev.clone()).collect();
        let live = Live { conn, battery: None, address, tx, sent_rev: has_rev, sent_page: None, has, gz, live_sent: false, buttons: HashSet::new(), media: HashSet::new(), viewing: None };
        let old = lock(&self.live).insert(phone_id.to_string(), live);
        if let Some(old) = old {
            let _ = old.tx.send(Cmd::Close(Outgoing::error("replaced", "This phone connected again.")));
        }
        self.touch(phone_id);
        crate::applog::line(&format!("Phone connected: {phone_id}"));
        self.changed();
        self.sync_phone(phone_id);
        conn
    }

    pub fn disconnect(&self, phone_id: &str, conn: u64) {
        let gone = {
            let mut live = lock(&self.live);
            if live.get(phone_id).is_some_and(|l| l.conn == conn) { live.remove(phone_id).is_some() } else { false }
        };
        if gone {
            crate::applog::line(&format!("Phone left: {phone_id}"));
            self.touch(phone_id);
            self.changed();
            self.pages_changed();
        }
    }

    /// The page a phone shows now
    pub fn viewing(&self, phone_id: &str, conn: u64, page: String) {
        let changed = match lock(&self.live).get_mut(phone_id) {
            Some(l) if l.conn == conn && l.viewing.as_deref() != Some(page.as_str()) => {
                l.viewing = Some(page.chars().take(80).collect());
                true
            }
            _ => false,
        };
        if changed {
            self.pages_changed();
            (self.notify)(Event::PhonePage(PhonePageEvent { phone_id: phone_id.into(), page: page.chars().take(80).collect() }));
        }
    }

    /// Tell the app which pages the phones show (each once)
    fn pages_changed(&self) {
        let mut pages: Vec<String> = lock(&self.live).values().filter_map(|l| l.viewing.clone()).collect();
        pages.sort();
        pages.dedup();
        (self.notify)(Event::Pages(pages));
    }

    pub fn set_battery(&self, phone_id: &str, conn: u64, battery: Option<u8>) {
        let updated = match lock(&self.live).get_mut(phone_id) {
            Some(l) if l.conn == conn => {
                l.battery = battery.map(|b| b.min(100));
                true
            }
            _ => false,
        };
        if updated {
            self.changed();
        }
    }

    fn touch(&self, phone_id: &str) {
        if let Some(r) = lock(&self.records).iter_mut().find(|r| r.id == phone_id) {
            r.last_seen_ms = now_ms();
        }
        self.save();
    }

    fn drop_live(&self, phone_id: &str, why: Outgoing) {
        let dropped = lock(&self.live).remove(phone_id);
        if let Some(l) = dropped {
            let _ = l.tx.send(Cmd::Close(why));
            self.pages_changed();
        }
    }

    // ----- Sync -----

    pub fn set_library(&self, text: &str) {
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            self.set_library_value(v);
        }
    }

    pub fn set_library_value(&self, library: Value) {
        lock(&self.sources).library = Some(library);
        self.sync_all();
    }

    pub fn set_settings(&self, text: &str) {
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            self.set_settings_value(v);
        }
    }

    pub fn set_settings_value(&self, settings: Value) {
        lock(&self.sources).settings = settings;
        self.sync_all();
    }

    /// The page open on the PC, for phones that follow it
    pub fn set_pc_page(&self, profile: String, page: String) {
        {
            let mut s = lock(&self.sources);
            if s.pc_profile.as_deref() == Some(profile.as_str()) && s.pc_page.as_deref() == Some(page.as_str()) {
                return;
            }
            s.pc_profile = Some(profile);
            s.pc_page = Some(page);
        }
        self.sync_all();
    }

    /// Send a phone its layout if it changed, or just the page if only that moved
    fn sync_phone(&self, phone_id: &str) {
        let Some(record) = lock(&self.records).iter().find(|r| r.id == phone_id).cloned() else { return };
        let started = std::time::Instant::now();
        let layout = {
            let src = lock(&self.sources);
            let mut cache = lock(&self.media_cache);
            sync::layout(&src, &record, &self.media_dir, &mut cache)
        };
        let Some(l) = layout else { return };
        let mut live = lock(&self.live);
        let Some(conn) = live.get_mut(phone_id) else { return };
        conn.media = l.media.iter().map(|m| m.file.clone()).collect();
        conn.buttons = l.buttons;
        if conn.sent_rev.as_deref() != Some(l.rev.as_str()) {
            // A layout the phone kept is named, not sent: a profile is megabytes of designs
            let cmd = if conn.has.contains(&l.rev) {
                Cmd::Send(Outgoing::Known { rev: l.rev.clone(), page: l.page.clone() }.text())
            } else {
                let msg = Outgoing::Layout { rev: l.rev.clone(), page: l.page.clone(), layout: l.body, media: l.media }.text();
                let kb = msg.len() / 1024;
                let cmd = if conn.gz { gzip(&msg).map_or(Cmd::Send(msg), Cmd::Binary) } else { Cmd::Send(msg) };
                let sent_kb = match &cmd { Cmd::Binary(b) => b.len() / 1024, _ => kb };
                crate::applog::line(&format!("Phone layout: {kb} KB ({sent_kb} KB sent), made in {} ms", started.elapsed().as_millis()));
                cmd
            };
            if conn.tx.send(cmd).is_ok() {
                conn.has.insert(l.rev.clone());
                conn.sent_rev = Some(l.rev);
                conn.sent_page = Some(l.page);
                // A new layout shows buttons as saved (the phone clears what plugins showed)
                conn.live_sent = false;
            }
        } else if conn.sent_page.as_deref() != Some(l.page.as_str()) {
            if conn.tx.send(Cmd::Send(Outgoing::Page { page: l.page.clone() }.text())).is_ok() {
                conn.sent_page = Some(l.page);
            }
        }
        // After a new layout, and once on each connection: what plugins show on its buttons now
        if !conn.live_sent {
            conn.live_sent = true;
            for (button, (title, icon)) in lock(&self.button_live).iter() {
                if conn.buttons.contains(button) {
                    let _ = conn.tx.send(Cmd::Send(Outgoing::Live { button_id: button.clone(), title: title.clone(), icon: icon.clone() }.text()));
                }
            }
            for (button, data) in lock(&self.button_data).iter() {
                if conn.buttons.contains(button) {
                    let _ = conn.tx.send(Cmd::Send(Outgoing::Data { button_id: button.clone(), data: data.clone() }.text()));
                }
            }
            for (button, state) in lock(&self.button_state).iter() {
                if conn.buttons.contains(button) {
                    let _ = conn.tx.send(Cmd::Send(Outgoing::State { button_id: button.clone(), state: *state }.text()));
                }
            }
        }
    }

    /// The phone was told to show a layout it kept, but doesn't have it any more: send it in full
    pub fn need_layout(&self, phone_id: &str, conn: u64, rev: &str) {
        let found = match lock(&self.live).get_mut(phone_id) {
            Some(l) if l.conn == conn => {
                l.has.remove(rev);
                l.sent_rev = None;
                true
            }
            _ => false,
        };
        if found {
            self.sync_phone(phone_id);
        }
    }

    pub fn sync_all(&self) {
        let ids: Vec<String> = lock(&self.live).keys().cloned().collect();
        for id in ids {
            self.sync_phone(&id);
        }
    }

    /// A phone tapped a button. Only buttons in its layout count; the UI runs it.
    pub fn press(&self, phone_id: &str, button_id: &str, kind: &str) -> Result<(), Refusal> {
        let kind = match kind {
            "long" | "release" => kind,
            _ => "press",
        };
        let allowed = lock(&self.live).get(phone_id).is_some_and(|l| l.buttons.contains(button_id));
        if !allowed {
            return Err(("unknown_button", "That button isn't on this phone any more."));
        }
        let phone_name = lock(&self.records).iter().find(|r| r.id == phone_id).map(|r| r.name.clone()).unwrap_or_default();
        (self.notify)(Event::Press(PressEvent { phone_id: phone_id.into(), phone_name, button_id: button_id.into(), kind: kind.into() }));
        Ok(())
    }

    /// A map on a phone is looking at something (a small object). Only buttons in its layout
    /// count; the UI passes it to the button's plugin.
    pub fn view(&self, phone_id: &str, button_id: &str, data: Value) {
        let allowed = lock(&self.live).get(phone_id).is_some_and(|l| l.buttons.contains(button_id));
        if allowed && data.is_object() && data.to_string().len() <= 4096 {
            (self.notify)(Event::View(ViewEvent { phone_id: phone_id.into(), button_id: button_id.into(), data }));
        }
    }

    /// A knob button turned on a phone. Only buttons in its layout count.
    pub fn turn(&self, phone_id: &str, button_id: &str, by: i32) {
        let allowed = lock(&self.live).get(phone_id).is_some_and(|l| l.buttons.contains(button_id));
        if allowed && by != 0 {
            (self.notify)(Event::Turn(TurnEvent { phone_id: phone_id.into(), button_id: button_id.into(), by: by.clamp(-50, 50) }));
        }
    }

    fn set_live(&self, button_id: &str, change: impl FnOnce(&mut (Option<String>, Option<String>))) {
        let (title, icon) = {
            let mut all = lock(&self.button_live);
            let entry = all.entry(button_id.to_string()).or_default();
            change(entry);
            let now = entry.clone();
            if now == (None, None) {
                all.remove(button_id);
            }
            now
        };
        let text = Outgoing::Live { button_id: button_id.into(), title, icon }.text();
        for conn in lock(&self.live).values() {
            if conn.buttons.contains(button_id) {
                let _ = conn.tx.send(Cmd::Send(text.clone()));
            }
        }
    }

    /// A plugin put a button in a state: phones with it get just that, at once
    pub fn set_live_state(&self, button_id: &str, state: u64) {
        if lock(&self.button_state).insert(button_id.to_string(), state) == Some(state) {
            return;
        }
        let text = Outgoing::State { button_id: button_id.into(), state }.text();
        for conn in lock(&self.live).values() {
            if conn.buttons.contains(button_id) {
                let _ = conn.tx.send(Cmd::Send(text.clone()));
            }
        }
    }

    /// A plugin changed a button's text (None: its own text again)
    pub fn set_live_title(&self, button_id: &str, title: Option<String>) {
        self.set_live(button_id, |e| e.0 = title);
    }

    /// A plugin changed a button's icon, as a data: URL (None: its own icon again)
    pub fn set_live_icon(&self, button_id: &str, icon: Option<String>) {
        self.set_live(button_id, |e| e.1 = icon);
    }

    /// A plugin sent live data for a button: its keys replace the ones the button had (null clears
    /// it all). Phones with the button get the change; phones that load it later get all of it.
    pub fn set_live_data(&self, button_id: &str, data: Value) {
        {
            let mut all = lock(&self.button_data);
            match &data {
                Value::Object(keys) => {
                    let entry = all.entry(button_id.to_string()).or_insert_with(|| Value::Object(Default::default()));
                    if let Value::Object(have) = entry {
                        for (k, v) in keys {
                            have.insert(k.clone(), v.clone());
                        }
                    }
                }
                _ => {
                    all.remove(button_id);
                }
            }
        }
        let text = Outgoing::Data { button_id: button_id.into(), data }.text();
        for conn in lock(&self.live).values() {
            if conn.buttons.contains(button_id) {
                let _ = conn.tx.send(Cmd::Send(text.clone()));
            }
        }
    }

    fn send_to(&self, phone_id: &str, out: &Outgoing) -> bool {
        lock(&self.live).get(phone_id).is_some_and(|l| l.tx.send(Cmd::Send(out.text())).is_ok())
    }

    /// Tell a phone how its press went
    pub fn press_result(&self, phone_id: &str, button_id: &str, ok: bool, message: &str) -> bool {
        self.send_to(phone_id, &Outgoing::Ran { button_id: button_id.into(), ok, message: clip(message, 200) })
    }

    /// A picture or GIF file a phone asked for, only if its layout uses it
    pub fn media_path(&self, phone_id: &str, file: &str) -> Option<PathBuf> {
        let name = safe_name(file)?;
        lock(&self.live).get(phone_id).filter(|l| l.media.contains(name)).map(|_| self.media_dir.join(name))
    }

    /// Tell every connected phone a newer phone app is out. Returns how many were told.
    pub fn notify_update(&self, version: &str, notes: &str) -> usize {
        let text = Outgoing::UpdateAvailable { version: clip(version, 40), notes: clip(notes, 2000) }.text();
        lock(&self.live).values().filter(|l| l.tx.send(Cmd::Send(text.clone())).is_ok()).count()
    }

    /// Tell connected phones the PC is moving to another port, so they reconnect there
    pub fn notify_port(&self, port: u16) -> usize {
        let text = Outgoing::PortChanged { port }.text();
        lock(&self.live).values().filter(|l| l.tx.send(Cmd::Send(text.clone())).is_ok()).count()
    }

    // ----- Changes from the Phones page -----

    pub fn forget(&self, phone_id: &str) -> bool {
        let removed = {
            let mut records = lock(&self.records);
            let before = records.len();
            records.retain(|r| r.id != phone_id);
            records.len() != before
        };
        if removed {
            self.drop_live(phone_id, Outgoing::error("removed", "This phone was removed on the PC."));
            self.save();
            self.changed();
        }
        removed
    }

    pub fn update(&self, phone_id: &str, patch: PhonePatch) -> Result<(), String> {
        {
            let mut records = lock(&self.records);
            let r = records.iter_mut().find(|r| r.id == phone_id).ok_or("That phone is no longer paired.")?;
            if let Some(n) = patch.name.map(|n| clip(&n, 40)).filter(|n| !n.is_empty()) {
                r.name = n;
            }
            if let Some(f) = patch.follows.map(|f| clip(&f, 100)).filter(|f| !f.is_empty()) {
                r.follows = f;
            }
            match patch.orient.as_deref() {
                Some(o @ ("portrait" | "landscape")) => r.orient = o.into(),
                Some(_) => return Err("Orientation must be portrait or landscape.".into()),
                None => {}
            }
            match patch.fit.as_deref() {
                Some(f @ ("turn" | "keep")) => r.fit = f.into(),
                Some(_) => return Err("Fit must be turn or keep.".into()),
                None => {}
            }
            match patch.grid {
                Some(g) if valid_grid(&g) => r.grid = g,
                Some(_) => return Err("That grid size is not allowed.".into()),
                None => {}
            }
        }
        self.save();
        self.changed();
        self.sync_phone(phone_id);
        Ok(())
    }

    #[cfg(test)]
    pub fn expire_pairing_now(&self) {
        if let Some(s) = lock(&self.session).as_mut() {
            s.expires_ms = 0;
        }
    }
}

/// A message gzipped, for a binary frame
fn gzip(text: &str) -> Option<Vec<u8>> {
    use std::io::Write;
    let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(text.as_bytes()).ok()?;
    z.finish().ok()
}
