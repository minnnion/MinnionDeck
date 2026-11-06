//! Phones: a phone or tablet opens a web page this PC serves on your Wi-Fi and uses it as a deck.
//! Nothing to install on the phone: scan the QR, accept the certificate warning once, say yes on the
//! PC, and the page shows this PC's pages and buttons live. A press goes back to the PC.
//!
//!   server.rs  HTTPS and WebSocket (the page, pictures, live buttons)
//!   cert.rs    the certificate this PC makes for itself
//!   pics.rs    icons and GIFs for the page, made on the PC
//!   mod.rs     the hub: paired phones, the pairing code, who is connected, the latest deck
//!
//! Two doors: a plain http:// page (what the QR opens: nothing to warn about) and the same page over
//! https:// on the next port (scrambled on the Wi-Fi; a phone warns once about the certificate).
//!
//! Pairing: the QR holds the page's address and a 6-digit code (valid 10 minutes, a few wrong
//! tries). The phone sends the code and its name; the PC shows a number to compare and Allow; the
//! phone then keeps its own long key, which is the only thing that lets it in again. Forget on the
//! PC takes it away.
#![allow(dead_code)]

pub mod cert;
pub mod pics;
mod server;

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::backend::package::Kind as SealKind;
use crate::backend::util::{now_ms, sha256_hex};
use crate::backend::vault;

const CODE_LIFE: Duration = Duration::from_secs(600);
const MAX_WRONG: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);
pub const DEFAULT_PORT: u16 = 47800;

/// What a phone did to a button.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Press,
    Long,
    Release,
    Turn(i32),
}

/// What the hub tells the window.
pub enum PhoneEvent {
    Press { device: String, button: String, kind: Kind },
    /// Phones, a request to pair or the pages phones show changed: look again
    Changed,
}

/// A message to one phone's connection.
pub(crate) enum Out {
    Text(String),
    Close,
}

#[derive(Serialize, Deserialize, Clone)]
struct Device {
    id: String,
    name: String,
    model: String,
    tablet: bool,
    token_hash: String,
    added_ms: u64,
    last_seen_ms: u64,
}

struct Conn {
    outbox: Sender<Out>,
    device: Option<String>,
    page: String,
}

struct Pending {
    conn: u64,
    name: String,
    model: String,
    tablet: bool,
    number: String,
}

struct Code {
    code: String,
    made: Instant,
}

struct State {
    snapshot: Arc<String>,
    version: u64,
    conns: HashMap<u64, Conn>,
    next_conn: u64,
    devices: Vec<Device>,
    pending: Vec<Pending>,
    code: Code,
    wrong: u32,
    locked_until: Option<Instant>,
}

/// One phone as the Phones page lists it.
#[derive(Clone, Debug)]
pub struct PhoneRow {
    pub id: String,
    pub name: String,
    pub model: String,
    pub tablet: bool,
    pub online: bool,
    pub last_seen_ms: u64,
    pub page: String,
}

/// A phone asking to be allowed in.
#[derive(Clone, Debug)]
pub struct PendingRow {
    pub conn: u64,
    pub name: String,
    pub model: String,
    pub number: String,
}

pub enum PairResult {
    /// Compare this number with the one on the PC, then Allow
    Waiting(String),
    Approved { token: String, name: String },
    Refused(String),
}

pub struct Hub {
    st: Mutex<State>,
    /// The plain port (what the QR opens) and the HTTPS one (0 when there is no certificate)
    pub port: u16,
    pub tls_port: u16,
    /// This PC's addresses on the network, the best first
    pub addrs: Vec<String>,
    file: PathBuf,
    events: Mutex<Sender<PhoneEvent>>,
    repaint: Box<dyn Fn() + Send + Sync>,
    /// Ask the PC to Allow a new phone (otherwise the code is enough)
    pub require_match: AtomicBool,
    next_id: AtomicU64,
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    let _ = SystemRandom::new().fill(&mut b);
    b
}

fn random_number(min: u32, max: u32) -> u32 {
    let b = random_bytes(4);
    min + u32::from_le_bytes([b[0], b[1], b[2], b[3]]) % (max - min + 1)
}

fn token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(32))
}

fn new_code() -> Code {
    Code { code: format!("{:06}", random_number(0, 999_999)), made: Instant::now() }
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// This PC's addresses a phone on the same Wi-Fi could reach, the one Windows would use first.
pub fn lan_addresses() -> Vec<String> {
    fn private(ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(v) => v.is_private() && !v.is_link_local(),
            IpAddr::V6(_) => false,
        }
    }
    let mut out: Vec<String> = vec![];
    if let Ok(ip) = local_ip_address::local_ip() {
        if private(&ip) {
            out.push(ip.to_string());
        }
    }
    if let Ok(list) = local_ip_address::list_afinet_netifas() {
        for (_, ip) in list {
            if private(&ip) && !out.contains(&ip.to_string()) {
                out.push(ip.to_string());
            }
        }
    }
    out
}

/// Starts the phone server: the certificate, the port (the wanted one, or the next free), and the
/// threads. `events` gets what phones do; `repaint` wakes the window.
pub fn start(root: &Path, want_port: u16, events: Sender<PhoneEvent>, repaint: impl Fn() + Send + Sync + 'static) -> Result<Arc<Hub>, String> {
    let dir = root.join("phones");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let addrs = lan_addresses();
    let bind_from = |from: u16| {
        (0..10u16).find_map(|i| {
            let p = from.checked_add(i)?;
            TcpListener::bind(("0.0.0.0", p)).ok().map(|l| (l, p))
        })
    };
    let (listener, port) = bind_from(want_port).ok_or_else(|| format!("No port from {want_port} to {} is free for the phone page.", want_port.saturating_add(9)))?;
    // the HTTPS page is the safer way in; if its certificate can't be made, the plain page still works
    let secure = cert::load_or_make(&dir, &addrs).ok().and_then(|tls| bind_from(port.saturating_add(1)).map(|(l, p)| (l, p, tls)));
    let tls_port = secure.as_ref().map(|(_, p, _)| *p).unwrap_or(0);
    // the paired phones, sealed (a plain devices.json from an earlier version is sealed first)
    let file = dir.join("devices.mdsave");
    let _ = vault::migrate(&dir.join("devices.json"), &file, SealKind::Save);
    let devices: Vec<Device> = vault::read(&file, SealKind::Save).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let hub = Arc::new(Hub {
        st: Mutex::new(State {
            snapshot: Arc::new(String::new()),
            version: 0,
            conns: HashMap::new(),
            next_conn: 1,
            devices,
            pending: vec![],
            code: new_code(),
            wrong: 0,
            locked_until: None,
        }),
        port,
        tls_port,
        addrs,
        file,
        events: Mutex::new(events),
        repaint: Box::new(repaint),
        require_match: AtomicBool::new(true),
        next_id: AtomicU64::new(1),
    });
    server::serve(hub.clone(), listener, secure.map(|(l, _, tls)| (l, tls)));
    Ok(hub)
}

impl Hub {
    fn save(&self, st: &State) {
        let _ = vault::write(&self.file, SealKind::Save, serde_json::to_string(&st.devices).unwrap_or_default().as_bytes());
    }

    pub(crate) fn emit(&self, e: PhoneEvent) {
        let _ = self.events.lock().unwrap_or_else(|e| e.into_inner()).send(e);
        (self.repaint)();
    }

    /// The certificate (DER), for installing it on a phone that insists on trusting it.
    pub fn cert_der(&self) -> Option<Vec<u8>> {
        fs::read(self.file.parent()?.join("cert.der")).ok()
    }

    // ----- the address and the code -----

    /// The code on screen now, made again when it ran out.
    pub fn code(&self) -> String {
        let mut st = lock(&self.st);
        if st.code.made.elapsed() > CODE_LIFE {
            st.code = new_code();
        }
        st.code.code.clone()
    }

    /// What the QR holds: the page and the code, after a # so it never goes in a log.
    pub fn url(&self) -> String {
        format!("{}#{}", self.base_url(), self.code())
    }

    /// The page a phone opens: plain http, so a phone shows no certificate warning.
    pub fn base_url(&self) -> String {
        format!("http://{}:{}/", self.addrs.first().map(|s| s.as_str()).unwrap_or("localhost"), self.port)
    }

    /// The same page over HTTPS (traffic on the Wi-Fi is scrambled, but a phone warns once).
    pub fn secure_url(&self) -> Option<String> {
        (self.tls_port != 0).then(|| format!("https://{}:{}/", self.addrs.first().map(|s| s.as_str()).unwrap_or("localhost"), self.tls_port))
    }

    // ----- the deck the phones show -----

    /// Sets what phones show (a JSON snapshot, see app_phones.rs). Phones get it at once.
    pub fn set_snapshot(&self, json: String) {
        let mut st = lock(&self.st);
        if *st.snapshot != json {
            st.snapshot = Arc::new(json);
            st.version += 1;
        }
    }

    pub(crate) fn snapshot(&self) -> (u64, Arc<String>) {
        let st = lock(&self.st);
        (st.version, st.snapshot.clone())
    }

    /// How many phones are connected now.
    pub fn connected(&self) -> usize {
        let st = lock(&self.st);
        st.conns.values().filter_map(|c| c.device.as_ref()).collect::<std::collections::HashSet<_>>().len()
    }

    /// The pages phones are showing, so plugins hear about their buttons.
    pub fn pages_shown(&self) -> Vec<String> {
        let st = lock(&self.st);
        let mut p: Vec<String> = st.conns.values().filter(|c| c.device.is_some() && !c.page.is_empty()).map(|c| c.page.clone()).collect();
        p.sort();
        p.dedup();
        p
    }

    // ----- phones -----

    pub fn phones(&self) -> Vec<PhoneRow> {
        let st = lock(&self.st);
        let mut rows: Vec<PhoneRow> = st
            .devices
            .iter()
            .map(|d| {
                let conn = st.conns.values().find(|c| c.device.as_deref() == Some(d.id.as_str()));
                PhoneRow { id: d.id.clone(), name: d.name.clone(), model: d.model.clone(), tablet: d.tablet, online: conn.is_some(), last_seen_ms: d.last_seen_ms, page: conn.map(|c| c.page.clone()).unwrap_or_default() }
            })
            .collect();
        rows.sort_by_key(|r| (!r.online, std::cmp::Reverse(r.last_seen_ms)));
        rows
    }

    pub fn pending(&self) -> Vec<PendingRow> {
        lock(&self.st).pending.iter().map(|p| PendingRow { conn: p.conn, name: p.name.clone(), model: p.model.clone(), number: p.number.clone() }).collect()
    }

    /// Takes a phone away: it has to be paired again.
    pub fn forget(&self, device: &str) {
        let mut st = lock(&self.st);
        st.devices.retain(|d| d.id != device);
        for c in st.conns.values_mut().filter(|c| c.device.as_deref() == Some(device)) {
            let _ = c.outbox.send(Out::Text(json!({ "t": "forgotten" }).to_string()));
            let _ = c.outbox.send(Out::Close);
            c.device = None;
        }
        self.save(&st);
        drop(st);
        self.emit(PhoneEvent::Changed);
    }

    /// Allow a phone that asked to pair.
    pub fn approve(&self, conn: u64) {
        let mut st = lock(&self.st);
        let Some(i) = st.pending.iter().position(|p| p.conn == conn) else { return };
        let p = st.pending.remove(i);
        let (tok, id) = (token(), format!("ph{}-{}", now_ms(), self.next_id.fetch_add(1, Ordering::Relaxed)));
        st.devices.push(Device { id: id.clone(), name: p.name.clone(), model: p.model, tablet: p.tablet, token_hash: sha256_hex(tok.as_bytes()), added_ms: now_ms(), last_seen_ms: now_ms(), });
        if let Some(c) = st.conns.get_mut(&conn) {
            c.device = Some(id);
            let _ = c.outbox.send(Out::Text(json!({ "t": "paired", "token": tok, "name": p.name }).to_string()));
        }
        // the code was used: the next phone gets a new one
        st.code = new_code();
        self.save(&st);
        drop(st);
        self.emit(PhoneEvent::Changed);
    }

    /// Say no to a phone that asked to pair.
    pub fn deny(&self, conn: u64) {
        let mut st = lock(&self.st);
        if let Some(i) = st.pending.iter().position(|p| p.conn == conn) {
            st.pending.remove(i);
            if let Some(c) = st.conns.get(&conn) {
                let _ = c.outbox.send(Out::Text(json!({ "t": "denied" }).to_string()));
            }
        }
        drop(st);
        self.emit(PhoneEvent::Changed);
    }

    /// A message for every connection of a phone (the answer to a press).
    pub fn tell(&self, device: &str, msg: &Value) {
        let st = lock(&self.st);
        for c in st.conns.values().filter(|c| c.device.as_deref() == Some(device)) {
            let _ = c.outbox.send(Out::Text(msg.to_string()));
        }
    }

    // ----- used by the server -----

    pub(crate) fn register(&self, outbox: Sender<Out>) -> u64 {
        let mut st = lock(&self.st);
        let id = st.next_conn;
        st.next_conn += 1;
        st.conns.insert(id, Conn { outbox, device: None, page: String::new() });
        id
    }

    pub(crate) fn unregister(&self, conn: u64) {
        let mut st = lock(&self.st);
        st.conns.remove(&conn);
        st.pending.retain(|p| p.conn != conn);
        drop(st);
        self.emit(PhoneEvent::Changed);
    }

    /// The phone's own long key: who it is, or None.
    pub(crate) fn authenticate(&self, conn: u64, tok: &str) -> Option<String> {
        let hash = sha256_hex(tok.as_bytes());
        let mut st = lock(&self.st);
        let d = st.devices.iter_mut().find(|d| d.token_hash == hash)?;
        d.last_seen_ms = now_ms();
        let (id, name) = (d.id.clone(), d.name.clone());
        if let Some(c) = st.conns.get_mut(&conn) {
            c.device = Some(id.clone());
        }
        self.save(&st);
        drop(st);
        self.emit(PhoneEvent::Changed);
        Some(name)
    }

    /// Whether a long key belongs to a paired phone (for the page's pictures).
    pub(crate) fn token_ok(&self, tok: &str) -> bool {
        let hash = sha256_hex(tok.as_bytes());
        lock(&self.st).devices.iter().any(|d| d.token_hash == hash)
    }

    pub(crate) fn device_of(&self, conn: u64) -> Option<String> {
        lock(&self.st).conns.get(&conn).and_then(|c| c.device.clone())
    }

    pub(crate) fn set_page(&self, conn: u64, page: &str) {
        let mut st = lock(&self.st);
        if let Some(c) = st.conns.get_mut(&conn) {
            if c.page != page {
                c.page = page.chars().take(80).collect();
                drop(st);
                self.emit(PhoneEvent::Changed);
            }
        }
    }

    pub(crate) fn pair(&self, conn: u64, code: &str, name: &str, model: &str, tablet: bool) -> PairResult {
        let mut st = lock(&self.st);
        if st.locked_until.is_some_and(|t| t > Instant::now()) {
            return PairResult::Refused("Too many wrong codes. Wait a minute and try again.".into());
        }
        let fresh = st.code.made.elapsed() <= CODE_LIFE;
        // compared in full whatever the first wrong digit is
        let right = fresh && code.len() == st.code.code.len() && code.bytes().zip(st.code.code.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
        if !right {
            st.wrong += 1;
            if st.wrong >= MAX_WRONG {
                st.wrong = 0;
                st.locked_until = Some(Instant::now() + LOCKOUT);
            }
            return PairResult::Refused(if fresh { "That code isn't right. Look at the code on the PC.".into() } else { "That code ran out. Look at the new one on the PC.".into() });
        }
        st.wrong = 0;
        let name: String = name.chars().filter(|c| !c.is_control()).take(30).collect::<String>().trim().to_string();
        let name = if name.is_empty() { "Phone".to_string() } else { name };
        let model: String = model.chars().filter(|c| !c.is_control()).take(40).collect();
        if !self.require_match.load(Ordering::Relaxed) {
            drop(st);
            // the code is enough
            let number = String::new();
            let _ = number;
            let mut st = lock(&self.st);
            let (tok, id) = (token(), format!("ph{}-{}", now_ms(), self.next_id.fetch_add(1, Ordering::Relaxed)));
            st.devices.push(Device { id: id.clone(), name: name.clone(), model, tablet, token_hash: sha256_hex(tok.as_bytes()), added_ms: now_ms(), last_seen_ms: now_ms() });
            if let Some(c) = st.conns.get_mut(&conn) {
                c.device = Some(id);
            }
            st.code = new_code();
            self.save(&st);
            drop(st);
            self.emit(PhoneEvent::Changed);
            return PairResult::Approved { token: tok, name };
        }
        let number = random_number(100, 999).to_string();
        st.pending.retain(|p| p.conn != conn);
        st.pending.push(Pending { conn, name, model, tablet, number: number.clone() });
        drop(st);
        self.emit(PhoneEvent::Changed);
        PairResult::Waiting(number)
    }
}

#[cfg(test)]
mod tests;
