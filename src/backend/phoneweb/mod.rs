//! Phones: a phone or tablet opens a web page this PC serves on your Wi-Fi and uses it as a deck.
//! Nothing to install on the phone: scan the QR, accept the certificate warning once, say yes on the
//! PC, and the page shows this PC's pages and buttons live. A press goes back to the PC.
//!
//!   server.rs  HTTPS and WebSocket (the page, pictures, live buttons)
//!   cert.rs    the certificate this PC makes for itself
//!   pics.rs    icons and GIFs for the page, made on the PC
//!   mod.rs     the hub: paired phones, the pairing code, who is connected, the latest deck
//!
//! Two doors: the page over https:// (what the QR opens: scrambled on the Wi-Fi; a phone warns once
//! about the certificate) and, one port below, a plain http:// door. The plain door only sends
//! phones on to the HTTPS page, unless "Allow the plain http page" is on in Settings › Phones (it is
//! switched on by itself for installs that already had phones paired, so they keep working). Both
//! doors listen only on this PC's own Wi-Fi/LAN addresses and on localhost, never on every network
//! the PC is on.
//!
//! Pairing: the QR holds the page's address and a 6-digit code (valid 10 minutes; a few wrong tries
//! lock that phone's address out for a minute, and many wrong tries from anywhere make a new code).
//! The phone sends the code and its name; the PC shows a number to compare and Allow; the phone
//! then keeps its own long key, which is the only thing that lets it in again. The key is only ever
//! sent inside the WebSocket; pictures are fetched with a short-lived ticket the PC hands out there.
//! Forget on the PC takes the phone away.
#![allow(dead_code)]

pub mod cert;
pub mod pics;
mod server;

use std::collections::HashMap;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
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
/// Wrong codes from every address together before the code is replaced (so guessing never adds up)
const MAX_WRONG_TOTAL: u32 = 30;
/// Phones waiting for Allow at once, and how long one may wait
const MAX_PENDING: usize = 8;
pub(crate) const PAIR_WAIT: Duration = Duration::from_secs(120);
/// How long a picture ticket opens the page's pictures (a phone that is connected gets a new one in time)
pub(crate) const TICKET_LIFE: Duration = Duration::from_secs(12 * 3600);
/// How often this PC's addresses are looked at again (Wi-Fi that came up after the app started)
const ADDRESS_CHECK: Duration = Duration::from_secs(5);
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
    /// Where it connected from (wrong codes are counted for each address)
    peer: IpAddr,
}

struct Pending {
    conn: u64,
    name: String,
    model: String,
    tablet: bool,
    number: String,
    since: Instant,
}

/// Wrong codes from one address
#[derive(Default)]
struct Guesses {
    wrong: u32,
    locked_until: Option<Instant>,
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
    /// Wrong codes, by the address they came from, and all together since the code was made
    guesses: HashMap<IpAddr, Guesses>,
    wrong_total: u32,
    /// Picture tickets: ticket → (the phone it was made for, when)
    tickets: HashMap<String, (String, Instant)>,
}

impl State {
    fn renew_code(&mut self) {
        self.code = new_code();
        self.wrong_total = 0;
    }

    /// A ticket for a phone's pictures: the one it has while that is young, else a new one.
    fn ticket_for(&mut self, device: &str) -> String {
        self.tickets.retain(|_, (_, made)| made.elapsed() < TICKET_LIFE);
        if let Some((k, _)) = self.tickets.iter().find(|(_, (d, made))| d == device && made.elapsed() < TICKET_LIFE / 2) {
            return k.clone();
        }
        let k = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(18));
        self.tickets.insert(k.clone(), (device.to_string(), Instant::now()));
        k
    }
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
    Approved { token: String, name: String, ticket: String },
    Refused(String),
}

pub struct Hub {
    st: Mutex<State>,
    /// The plain port and the HTTPS one (what the QR opens; 0 when there is no certificate)
    pub port: u16,
    pub tls_port: u16,
    /// This PC's addresses on the network, the best first (looked at again every few seconds)
    addrs: Mutex<Vec<String>>,
    file: PathBuf,
    events: Mutex<Sender<PhoneEvent>>,
    repaint: Box<dyn Fn() + Send + Sync>,
    /// Ask the PC to Allow a new phone (otherwise the code is enough)
    pub require_match: AtomicBool,
    /// The QR opens the HTTPS page (Settings › Phones › Secure phone link)
    pub secure_link: AtomicBool,
    /// The plain http page works too (Settings › Phones › Allow the plain http page); otherwise the
    /// plain port only sends phones to the HTTPS page
    pub plain_ok: AtomicBool,
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
    // only this PC's own addresses on the local network, and localhost: not every network it is on
    let ips = listen_addresses(&addrs);
    let bind_from = |from: u16| {
        (0..10u16).find_map(|i| {
            let p = from.checked_add(i)?;
            ips.iter().map(|ip| TcpListener::bind((*ip, p)).ok()).collect::<Option<Vec<TcpListener>>>().map(|l| (l, p))
        })
    };
    let (listeners, port) = bind_from(want_port).ok_or_else(|| format!("No port from {want_port} to {} is free for the phone page.", want_port.saturating_add(9)))?;
    // the HTTPS page is the safe way in; if its certificate can't be made, the plain page still works
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
            guesses: HashMap::new(),
            wrong_total: 0,
            tickets: HashMap::new(),
        }),
        port,
        tls_port,
        addrs: Mutex::new(addrs.clone()),
        file,
        events: Mutex::new(events),
        repaint: Box::new(repaint),
        require_match: AtomicBool::new(true),
        secure_link: AtomicBool::new(true),
        plain_ok: AtomicBool::new(false),
        next_id: AtomicU64::new(1),
    });
    let (tls, secure_listeners) = match secure {
        Some((l, _, tls)) => (Some(tls), l),
        None => (None, vec![]),
    };
    let doors = server::Doors::new(hub.clone(), tls);
    for l in listeners {
        doors.listen(l, false);
    }
    for l in secure_listeners {
        doors.listen(l, true);
    }
    watch_addresses(hub.clone(), doors, addrs);
    Ok(hub)
}

/// The addresses the doors listen on: localhost and each LAN address.
fn listen_addresses(lan: &[String]) -> Vec<IpAddr> {
    let mut ips = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    ips.extend(lan.iter().filter_map(|a| a.parse::<IpAddr>().ok()));
    ips
}

/// The PC may get its Wi-Fi address after the app has started (it starts with Windows), or a new
/// one later: the doors open on each new address, and the Phones page shows the address in use.
fn watch_addresses(hub: Arc<Hub>, doors: Arc<server::Doors>, mut bound: Vec<String>) {
    let _ = std::thread::Builder::new().name("phone addresses".into()).spawn(move || {
        loop {
            std::thread::sleep(ADDRESS_CHECK);
            let now = lan_addresses();
            for a in now.iter().filter(|a| !bound.contains(a)).cloned().collect::<Vec<_>>() {
                let Ok(ip) = a.parse::<IpAddr>() else { continue };
                // both doors or neither, so the address on screen always works
                let plain = TcpListener::bind((ip, hub.port));
                let secure = (hub.tls_port != 0).then(|| TcpListener::bind((ip, hub.tls_port)));
                match (plain, secure) {
                    (Ok(p), None) => doors.listen(p, false),
                    (Ok(p), Some(Ok(s))) => {
                        doors.listen(p, false);
                        doors.listen(s, true);
                    }
                    _ => continue,
                }
                bound.push(a);
            }
            if !now.is_empty() && hub.set_addrs(now) {
                hub.emit(PhoneEvent::Changed);
            }
        }
    });
}

impl Hub {
    fn save(&self, st: &State) {
        let _ = vault::write(&self.file, SealKind::Save, serde_json::to_string(&st.devices).unwrap_or_default().as_bytes());
    }

    pub(crate) fn emit(&self, e: PhoneEvent) {
        let _ = self.events.lock().unwrap_or_else(|e| e.into_inner()).send(e);
        (self.repaint)();
    }

    /// This PC's own certificate (DER), to install on a phone so the page is trusted (cert.rs).
    pub fn cert_der(&self) -> Option<Vec<u8>> {
        cert::authority_der(self.file.parent()?)
    }

    /// Its fingerprint, to compare with what the phone shows when it is installed.
    pub fn cert_fingerprint(&self) -> Option<String> {
        self.cert_der().map(|d| cert::fingerprint(&d))
    }

    // ----- the address and the code -----

    /// This PC's addresses on the network, the best first.
    pub fn addrs(&self) -> Vec<String> {
        self.addrs.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// True when they changed.
    fn set_addrs(&self, addrs: Vec<String>) -> bool {
        let mut a = self.addrs.lock().unwrap_or_else(|e| e.into_inner());
        let changed = *a != addrs;
        *a = addrs;
        changed
    }

    fn host(&self) -> String {
        self.addrs().into_iter().next().unwrap_or_else(|| "localhost".into())
    }

    /// Whether the plain http page is served (else the plain port only points at the HTTPS page).
    /// Without a certificate there is no HTTPS page, so the plain one is all there is.
    pub fn plain_served(&self) -> bool {
        self.tls_port == 0 || self.plain_ok.load(Ordering::Relaxed)
    }

    /// The code on screen now, made again when it ran out.
    pub fn code(&self) -> String {
        let mut st = lock(&self.st);
        if st.code.made.elapsed() > CODE_LIFE {
            st.renew_code();
        }
        st.code.code.clone()
    }

    /// What the QR holds: the page and the code, after a # so it never goes in a log.
    pub fn url(&self) -> String {
        format!("{}#{}", self.page_url(), self.code())
    }

    /// The page the QR and the Phones page point to: the HTTPS page, unless there is no
    /// certificate, or the plain page is allowed and Secure phone link is off.
    pub fn page_url(&self) -> String {
        match self.secure_url() {
            Some(s) if self.secure_link.load(Ordering::Relaxed) || !self.plain_served() => s,
            _ => self.base_url(),
        }
    }

    /// The plain http page: no certificate warning, but everything crosses the Wi-Fi readable.
    pub fn base_url(&self) -> String {
        format!("http://{}:{}/", self.host(), self.port)
    }

    /// The same page over HTTPS (traffic on the Wi-Fi is scrambled, but a phone warns once).
    pub fn secure_url(&self) -> Option<String> {
        (self.tls_port != 0).then(|| format!("https://{}:{}/", self.host(), self.tls_port))
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
        st.tickets.retain(|_, (d, _)| d != device);
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
        let ticket = st.ticket_for(&id);
        if let Some(c) = st.conns.get_mut(&conn) {
            c.device = Some(id);
            let _ = c.outbox.send(Out::Text(json!({ "t": "paired", "token": tok, "name": p.name, "ticket": ticket }).to_string()));
        }
        // the code was used: the next phone gets a new one
        st.renew_code();
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

    pub(crate) fn register(&self, outbox: Sender<Out>, peer: IpAddr) -> u64 {
        let mut st = lock(&self.st);
        let id = st.next_conn;
        st.next_conn += 1;
        st.conns.insert(id, Conn { outbox, device: None, page: String::new(), peer });
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

    /// The picture ticket for the phone on this connection (None when it isn't in).
    pub(crate) fn ticket(&self, conn: u64) -> Option<String> {
        let mut st = lock(&self.st);
        let device = st.conns.get(&conn)?.device.clone()?;
        Some(st.ticket_for(&device))
    }

    /// Whether a picture ticket is good: handed to a phone that is still paired, and not too old.
    pub(crate) fn ticket_ok(&self, ticket: &str) -> bool {
        let st = lock(&self.st);
        st.tickets.get(ticket).is_some_and(|(device, made)| made.elapsed() < TICKET_LIFE && st.devices.iter().any(|d| d.id == *device))
    }

    /// Whether this connection is waiting for Allow. One that waited too long is taken off the
    /// list and told (false then, as for one that isn't waiting).
    pub(crate) fn waiting(&self, conn: u64) -> bool {
        let mut st = lock(&self.st);
        let Some(i) = st.pending.iter().position(|p| p.conn == conn) else { return false };
        if st.pending[i].since.elapsed() <= PAIR_WAIT {
            return true;
        }
        st.pending.remove(i);
        if let Some(c) = st.conns.get(&conn) {
            let _ = c.outbox.send(Out::Text(json!({ "t": "refused", "message": "The PC didn't answer in time. Try again." }).to_string()));
        }
        drop(st);
        self.emit(PhoneEvent::Changed);
        false
    }

    pub(crate) fn device_of(&self, conn: u64) -> Option<String> {
        lock(&self.st).conns.get(&conn).and_then(|c| c.device.clone())
    }

    pub(crate) fn set_page(&self, conn: u64, page: &str) {
        let mut st = lock(&self.st);
        // only a phone that is in says which page it shows
        if let Some(c) = st.conns.get_mut(&conn).filter(|c| c.device.is_some()) {
            if c.page != page {
                c.page = page.chars().take(80).collect();
                drop(st);
                self.emit(PhoneEvent::Changed);
            }
        }
    }

    pub(crate) fn pair(&self, conn: u64, code: &str, name: &str, model: &str, tablet: bool) -> PairResult {
        let mut st = lock(&self.st);
        let Some(peer) = st.conns.get(&conn).map(|c| c.peer) else { return PairResult::Refused("That didn't work. Try again.".into()) };
        let now = Instant::now();
        // wrong codes are counted for each address: someone else guessing never locks this phone out
        if st.guesses.get(&peer).is_some_and(|g| g.locked_until.is_some_and(|t| t > now)) {
            return PairResult::Refused("Too many wrong codes. Wait a minute and try again.".into());
        }
        let fresh = st.code.made.elapsed() <= CODE_LIFE;
        // compared in full whatever the first wrong digit is
        let right = fresh && code.len() == st.code.code.len() && code.bytes().zip(st.code.code.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0;
        if !right {
            if st.guesses.len() > 256 {
                st.guesses.retain(|_, g| g.locked_until.is_some_and(|t| t > now));
            }
            let g = st.guesses.entry(peer).or_default();
            g.wrong += 1;
            if g.wrong >= MAX_WRONG {
                g.wrong = 0;
                g.locked_until = Some(now + LOCKOUT);
            }
            // guesses from many addresses never add up against one code: it is replaced
            st.wrong_total += 1;
            let renewed = fresh && st.wrong_total >= MAX_WRONG_TOTAL;
            if renewed {
                st.renew_code();
            }
            drop(st);
            if renewed {
                self.emit(PhoneEvent::Changed);
            }
            return PairResult::Refused(if fresh { "That code isn't right. Look at the code on the PC.".into() } else { "That code ran out. Look at the new one on the PC.".into() });
        }
        st.guesses.remove(&peer);
        let name: String = name.chars().filter(|c| !c.is_control()).take(30).collect::<String>().trim().to_string();
        let name = if name.is_empty() { "Phone".to_string() } else { name };
        let model: String = model.chars().filter(|c| !c.is_control()).take(40).collect();
        if !self.require_match.load(Ordering::Relaxed) {
            // the code is enough
            let (tok, id) = (token(), format!("ph{}-{}", now_ms(), self.next_id.fetch_add(1, Ordering::Relaxed)));
            st.devices.push(Device { id: id.clone(), name: name.clone(), model, tablet, token_hash: sha256_hex(tok.as_bytes()), added_ms: now_ms(), last_seen_ms: now_ms() });
            let ticket = st.ticket_for(&id);
            if let Some(c) = st.conns.get_mut(&conn) {
                c.device = Some(id);
            }
            st.renew_code();
            self.save(&st);
            drop(st);
            self.emit(PhoneEvent::Changed);
            return PairResult::Approved { token: tok, name, ticket };
        }
        st.pending.retain(|p| p.conn != conn && p.since.elapsed() <= PAIR_WAIT);
        if st.pending.len() >= MAX_PENDING {
            return PairResult::Refused("Too many phones are waiting to be allowed. Try again in a moment.".into());
        }
        let number = random_number(100, 999).to_string();
        st.pending.push(Pending { conn, name, model, tablet, number: number.clone(), since: now });
        drop(st);
        self.emit(PhoneEvent::Changed);
        PairResult::Waiting(number)
    }
}

#[cfg(test)]
mod tests;
