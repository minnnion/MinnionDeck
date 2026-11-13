//! The phone server: HTTPS for the page and its pictures, and one WebSocket per phone for live
//! buttons. One thread for each connection. The plain http:// port serves the same page only when
//! that is allowed in Settings; otherwise it sends phones on to https://.
//!
//! Nobody on the network can hold the server up: connections are counted (all together, and for
//! each address), a request's headers and the TLS handshake have to arrive within seconds, a
//! WebSocket message is a few KB at most, and a connection that neither shows a paired phone's key
//! nor asks to pair is closed after a moment.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{Value, json};
use tungstenite::Message;
use tungstenite::protocol::WebSocketConfig;

use super::{Hub, Kind, Out, PairResult, PhoneEvent, pics};
use crate::backend::media;

const INDEX: &str = include_str!("../../../assets/phone/index.html");
const MANIFEST: &str = include_str!("../../../assets/phone/manifest.webmanifest");
const FONT_400: &[u8] = include_bytes!("../../../assets/fonts/PixelifySans-400.ttf");
const FONT_600: &[u8] = include_bytes!("../../../assets/fonts/PixelifySans-600.ttf");
const FONT_700: &[u8] = include_bytes!("../../../assets/fonts/PixelifySans-700.ttf");

const MAX_HEAD: usize = 16 * 1024;
/// The biggest message a phone may send (a press, a code): bigger ones close the connection
/// before they are read into memory.
pub(crate) const MAX_MESSAGE: usize = 4096;
/// A phone that says nothing this long is gone
const SILENCE: Duration = Duration::from_secs(45);
/// A request's headers (and the TLS handshake before them) must arrive within this, all told
const HEAD_WITHIN: Duration = Duration::from_secs(10);
/// A connection must show a paired phone's key or ask to pair within this (again after each try)
pub(crate) const AUTH_WITHIN: Duration = Duration::from_secs(10);
/// A connection that never gets in is closed after this whatever it does
const UNPAIRED_AT_MOST: Duration = Duration::from_secs(300);

/// Connections served at once, on both ports together, and from one address. Each paired phone
/// keeps one open and fetches a few pictures beside it; more than this are closed as they arrive.
pub(crate) const MAX_CONNECTIONS: usize = 64;
pub(crate) const MAX_PER_ADDRESS: usize = 16;

/// The count of open connections, so nobody on the network can open threads without end, and one
/// address can't take every place.
#[derive(Default)]
pub(crate) struct Places {
    open: AtomicUsize,
    by_address: Mutex<HashMap<IpAddr, usize>>,
}

/// A connection's place in the count, given back when the connection ends (however it ends).
pub(crate) struct Slot(Arc<Places>, IpAddr);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::SeqCst);
        let mut by = self.0.by_address.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = by.get_mut(&self.1) {
            *n -= 1;
            if *n == 0 {
                by.remove(&self.1);
            }
        }
    }
}

impl Places {
    pub(crate) fn take(self: &Arc<Self>, from: IpAddr) -> Option<Slot> {
        let mut by = self.by_address.lock().unwrap_or_else(|e| e.into_inner());
        let mine = by.get(&from).copied().unwrap_or(0);
        if mine >= MAX_PER_ADDRESS || self.open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            return None;
        }
        self.open.fetch_add(1, Ordering::SeqCst);
        by.insert(from, mine + 1);
        Some(Slot(self.clone(), from))
    }
}

/// The server's doors: every listener (one for each of this PC's addresses and each port) hands
/// its connections to the same hub, under the same count.
pub(crate) struct Doors {
    hub: Arc<Hub>,
    tls: Option<Arc<ServerConfig>>,
    places: Arc<Places>,
}

impl Doors {
    pub(crate) fn new(hub: Arc<Hub>, tls: Option<Arc<ServerConfig>>) -> Arc<Doors> {
        Arc::new(Doors { hub, tls, places: Arc::new(Places::default()) })
    }

    /// Serves what arrives at `listener`: over HTTPS when `secure` (and there is a certificate),
    /// else on the plain door.
    pub(crate) fn listen(self: &Arc<Self>, listener: TcpListener, secure: bool) {
        let doors = self.clone();
        let tls = if secure { self.tls.clone() } else { None };
        if secure && tls.is_none() {
            return;
        }
        let _ = std::thread::Builder::new().name(if secure { "phone server (https)" } else { "phone server" }.into()).spawn(move || {
            loop {
                let (stream, peer) = match listener.accept() {
                    Ok(x) => x,
                    // the address went away, or Windows is out of sockets: wait, don't spin
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(250));
                        continue;
                    }
                };
                let Some(slot) = doors.places.take(peer.ip()) else { continue };
                let (hub, tls) = (doors.hub.clone(), tls.clone());
                let _ = std::thread::Builder::new().name("phone connection".into()).spawn(move || {
                    let _slot = slot;
                    let _ = match tls {
                        Some(tls) => handle(hub, tls, stream, peer.ip()),
                        None => handle_plain(hub, stream, peer.ip()),
                    };
                });
            }
        });
    }
}

/// A connection whose reads must be done by a deadline while the request arrives (however slowly
/// the bytes are fed), and wait only a moment once it is a WebSocket.
struct Sock {
    tcp: TcpStream,
    until: Option<Instant>,
}

impl Read for Sock {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if let Some(until) = self.until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(std::io::ErrorKind::TimedOut.into());
            }
            self.tcp.set_read_timeout(Some(left))?;
        }
        self.tcp.read(out)
    }
}

impl Write for Sock {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.tcp.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.tcp.flush()
    }
}

/// A stream whose reads can be made to wait only a moment.
trait Timed {
    fn read_timeout(&mut self, d: Duration);
}

impl Timed for Replay<Tls> {
    fn read_timeout(&mut self, d: Duration) {
        self.inner.sock.until = None;
        self.inner.sock.tcp.set_read_timeout(Some(d)).ok();
    }
}

impl Timed for Replay<Sock> {
    fn read_timeout(&mut self, d: Duration) {
        self.inner.until = None;
        self.inner.tcp.set_read_timeout(Some(d)).ok();
    }
}

/// Bytes already read from a stream, then the stream itself.
struct Replay<S> {
    buf: Vec<u8>,
    pos: usize,
    inner: S,
}

impl<S: Read> Read for Replay<S> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos < self.buf.len() {
            let n = out.len().min(self.buf.len() - self.pos);
            out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            return Ok(n);
        }
        self.inner.read(out)
    }
}

impl<S: Write> Write for Replay<S> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.inner.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

type Tls = StreamOwned<ServerConnection, Sock>;

struct Head {
    raw: Vec<u8>,
    /// HEAD: the answer's headers only
    head_only: bool,
    path: String,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if s.get(i + 1..i + 3).is_some() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads up to the end of the request's headers.
fn read_head(s: &mut impl Read) -> Option<Head> {
    let mut raw = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let n = s.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if raw.len() > MAX_HEAD {
            return None;
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?, first.next()?);
    if method != "GET" && method != "HEAD" {
        return None;
    }
    let (path, q) = target.split_once('?').unwrap_or((target, ""));
    let query = q.split('&').filter_map(|p| p.split_once('=')).map(|(k, v)| (percent_decode(k), percent_decode(v))).collect();
    let headers = lines.take_while(|l| !l.is_empty()).filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    Some(Head { raw, head_only: method == "HEAD", path: path.to_string(), query, headers })
}

/// What the page may load and run: itself, and only the script and style that are in it (by their
/// hashes), so text from a deck can never run as code. It can't be put in a frame.
fn content_security_policy() -> &'static str {
    static CSP: OnceLock<String> = OnceLock::new();
    CSP.get_or_init(|| {
        // a browser hashes what is between the tags, with line ends as the page parser leaves them
        let hash = |open: &str, close: &str| {
            let inner = INDEX.split_once(open).and_then(|(_, rest)| rest.split_once(close)).map(|(inner, _)| inner.replace("\r\n", "\n")).unwrap_or_default();
            let digest = ring::digest::digest(&ring::digest::SHA256, inner.as_bytes());
            format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(digest.as_ref()))
        };
        format!(
            "default-src 'none'; script-src {}; style-src {}; img-src 'self'; font-src 'self'; media-src data:; connect-src 'self' ws: wss:; manifest-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
            hash("<script>", "</script>"),
            hash("<style>", "</style>")
        )
    })
}

fn respond(s: &mut impl Write, h: &Head, status: &str, ctype: &str, body: &[u8], cache: &str) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: {cache}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: {}\r\n\r\n",
        body.len(),
        content_security_policy()
    );
    let _ = s.write_all(head.as_bytes());
    if !h.head_only {
        let _ = s.write_all(body);
    }
    let _ = s.flush();
}

/// Sends a browser that asked on the plain door to the HTTPS page, under the host it asked for.
fn redirect_to_https(hub: &Hub, head: &Head, s: &mut impl Write) {
    let host = head.headers.get("host").cloned().unwrap_or_default();
    let host = host.split(':').next().unwrap_or("").to_string();
    let ok = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    let to = format!("https://{}:{}/", if ok { host } else { "localhost".into() }, hub.tls_port);
    let msg = format!("HTTP/1.1 301 Moved Permanently\r\nLocation: {to}\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n");
    let _ = s.write_all(msg.as_bytes());
    let _ = s.flush();
}

fn sock(tcp: TcpStream) -> Sock {
    tcp.set_nodelay(true).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(10))).ok();
    Sock { tcp, until: Some(Instant::now() + HEAD_WITHIN) }
}

/// A WebSocket asked for by a page of another site (a browser says where the page came from) is
/// refused: only the phone page itself talks to this server.
fn same_origin(head: &Head) -> bool {
    match (head.headers.get("origin"), head.headers.get("host")) {
        (Some(origin), Some(host)) => origin.split_once("://").is_some_and(|(_, o)| o.eq_ignore_ascii_case(host)),
        _ => true,
    }
}

fn wants_websocket(head: &Head) -> bool {
    head.path == "/ws" && head.headers.get("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"))
}

/// A connection on the plain port.
fn handle_plain(hub: Arc<Hub>, tcp: TcpStream, peer: IpAddr) -> Option<()> {
    let mut s = sock(tcp);
    // someone starting a TLS handshake here wants the other port
    let mut first = [0u8; 1];
    s.tcp.set_read_timeout(Some(HEAD_WITHIN)).ok();
    s.tcp.peek(&mut first).ok()?;
    if first[0] == 0x16 {
        return None;
    }
    let head = read_head(&mut s)?;
    // the plain page only when it is allowed: otherwise codes and keys would cross the Wi-Fi readable
    if !hub.plain_served() {
        if !wants_websocket(&head) {
            redirect_to_https(&hub, &head, &mut s);
        }
        return Some(());
    }
    if wants_websocket(&head) {
        if !same_origin(&head) {
            return None;
        }
        let replay = Replay { buf: head.raw, pos: 0, inner: s };
        return websocket(hub, replay, peer);
    }
    route(&hub, &head, &mut s);
    let _ = s.flush();
    Some(())
}

/// A connection on the HTTPS port.
fn handle(hub: Arc<Hub>, tls: Arc<ServerConfig>, tcp: TcpStream, peer: IpAddr) -> Option<()> {
    let mut plain = sock(tcp);
    // 0x16 starts a TLS handshake; anything else is someone who typed http://
    let mut first = [0u8; 1];
    plain.tcp.set_read_timeout(Some(HEAD_WITHIN)).ok();
    plain.tcp.peek(&mut first).ok()?;
    if first[0] != 0x16 {
        let head = read_head(&mut plain)?;
        redirect_to_https(&hub, &head, &mut plain);
        return Some(());
    }
    let conn = ServerConnection::new(tls).ok()?;
    let mut s: Tls = StreamOwned::new(conn, plain);
    let head = read_head(&mut s)?;
    if wants_websocket(&head) {
        if !same_origin(&head) {
            return None;
        }
        let replay = Replay { buf: head.raw, pos: 0, inner: s };
        return websocket(hub, replay, peer);
    }
    route(&hub, &head, &mut s);
    s.conn.send_close_notify();
    let _ = s.flush();
    Some(())
}

fn ctype_of(path: &std::path::Path) -> &'static str {
    // a sealed picture is "pic.png.mdmedia": its kind is the ending before that
    let path = if path.extension().and_then(|e| e.to_str()) == Some(crate::backend::vault::MEDIA_EXT) { path.with_extension("") } else { path.to_path_buf() };
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        _ => "application/octet-stream",
    }
}

fn route(hub: &Hub, h: &Head, s: &mut impl Write) {
    let q = |k: &str| h.query.get(k).map(|v| v.as_str()).unwrap_or("");
    // pictures are for paired phones only: they come with the ticket the phone was handed inside
    // its WebSocket (its long key never goes in an address)
    let paired = || hub.ticket_ok(q("k"));
    let no = |s: &mut _| respond(s, h, "404 Not Found", "text/plain", b"no", "no-cache");
    // a picture is the same for as long as its address is (the ticket is part of it)
    const PICTURES: &str = "private, max-age=3600";
    match h.path.as_str() {
        "/" | "/index.html" => respond(s, h, "200 OK", "text/html; charset=utf-8", INDEX.as_bytes(), "no-cache"),
        "/manifest.webmanifest" => respond(s, h, "200 OK", "application/manifest+json", MANIFEST.as_bytes(), "no-cache"),
        "/app-icon-192.png" | "/apple-touch-icon.png" => match pics::app_icon_png(192) {
            Some(b) => respond(s, h, "200 OK", "image/png", &b, "max-age=86400"),
            None => no(s),
        },
        "/app-icon-512.png" => match pics::app_icon_png(512) {
            Some(b) => respond(s, h, "200 OK", "image/png", &b, "max-age=86400"),
            None => no(s),
        },
        "/cert-info" => {
            let fp = hub.cert_fingerprint().unwrap_or_default();
            respond(s, h, "200 OK", "application/json", json!({ "fingerprint": fp }).to_string().as_bytes(), "no-cache")
        }
        "/minniondeck.cer" => match hub.cert_der() {
            Some(b) => respond(s, h, "200 OK", "application/x-x509-ca-cert", &b, "no-cache"),
            None => no(s),
        },
        "/font-400.ttf" => respond(s, h, "200 OK", "font/ttf", FONT_400, "max-age=86400"),
        "/font-600.ttf" => respond(s, h, "200 OK", "font/ttf", FONT_600, "max-age=86400"),
        "/font-700.ttf" => respond(s, h, "200 OK", "font/ttf", FONT_700, "max-age=86400"),
        "/icon" if paired() => {
            let c = q("c");
            let ink = u32::from_str_radix(c.trim_start_matches('#'), 16).ok().filter(|_| c.trim_start_matches('#').len() == 6).map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]).unwrap_or([0, 0, 0, 255]);
            match pics::icon_png(q("id"), ink) {
                Some(b) => respond(s, h, "200 OK", "image/png", &b, PICTURES),
                None => no(s),
            }
        }
        "/media" if paired() => match media::picture(q("id")).and_then(|p| p.file).and_then(|f| media::read_bytes(&f).map(|b| (ctype_of(&f), b))) {
            Some((ct, b)) => respond(s, h, "200 OK", ct, &b, PICTURES),
            None => no(s),
        },
        "/gif" if paired() => {
            let bytes = media::gif(q("id")).and_then(|g| g.src).and_then(|src| match src {
                media::GifSource::File(p) => media::read_bytes(&p),
                media::GifSource::Drawn { size, fps, frames } => pics::drawn_gif(size, fps, &frames),
            });
            match bytes {
                Some(b) => respond(s, h, "200 OK", "image/gif", &b, PICTURES),
                None => no(s),
            }
        }
        "/icon" | "/media" | "/gif" => respond(s, h, "403 Forbidden", "text/plain", b"Pair this phone first.", "no-store"),
        _ => respond(s, h, "404 Not Found", "text/plain", b"Not found", "no-cache"),
    }
}

fn send<S: Read + Write>(ws: &mut tungstenite::WebSocket<S>, v: Value) -> bool {
    ws.send(Message::text(v.to_string())).is_ok()
}

fn websocket<S: Read + Write + Timed>(hub: Arc<Hub>, stream: S, peer: IpAddr) -> Option<()> {
    // a phone only ever sends a few words: anything bigger is refused before it is read
    let limits = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
    let mut ws = tungstenite::accept_with_config(stream, Some(limits)).ok()?;
    // from here on a read waits a moment, so this thread can also send what the PC has to say
    ws.get_mut().read_timeout(Duration::from_millis(100));
    let (otx, orx): (_, Receiver<Out>) = channel();
    let conn = hub.register(otx, peer);
    let mut sent_version = 0u64;
    let born = Instant::now();
    let mut last_heard = born;
    // when it connected or last asked to pair: a connection that isn't a phone's doesn't stay
    let mut last_try = born;
    let mut ticket_at: Option<Instant> = None;
    let mut announced = false;
    'conn: loop {
        match ws.read() {
            Ok(Message::Text(t)) => {
                last_heard = Instant::now();
                if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                    if text(&v, "t") == "pair" {
                        last_try = last_heard;
                    }
                    if !on_message(&hub, conn, &mut ws, &v) {
                        break 'conn;
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => last_heard = Instant::now(),
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
        if last_heard.elapsed() > SILENCE {
            break;
        }
        while let Ok(o) = orx.try_recv() {
            match o {
                Out::Text(t) => {
                    if ws.send(Message::text(t)).is_err() {
                        break 'conn;
                    }
                }
                Out::Close => break 'conn,
            }
        }
        if hub.device_of(conn).is_some() {
            // a phone that stays connected gets a new picture ticket before the old one runs out
            match ticket_at {
                None => ticket_at = Some(Instant::now()),
                Some(t) if t.elapsed() > super::TICKET_LIFE / 2 => {
                    ticket_at = Some(Instant::now());
                    if let Some(k) = hub.ticket(conn) {
                        if !send(&mut ws, json!({ "t": "ticket", "ticket": k })) {
                            break;
                        }
                    }
                }
                Some(_) => {}
            }
            // a phone that is in sees the deck, and every change to it
            let (v, snap) = hub.snapshot();
            if v != sent_version || !announced {
                if !snap.is_empty() && ws.send(Message::text(format!("{{\"t\":\"snap\",\"deck\":{snap}}}"))).is_err() {
                    break;
                }
                sent_version = v;
                announced = true;
            }
        } else if born.elapsed() > UNPAIRED_AT_MOST || (!hub.waiting(conn) && last_try.elapsed() > AUTH_WITHIN) {
            // not a paired phone, and not waiting for Allow: pings don't keep it open
            // (it is told why first, if the PC had something to say)
            while let Ok(Out::Text(t)) = orx.try_recv() {
                let _ = ws.send(Message::text(t));
            }
            break;
        }
    }
    hub.unregister(conn);
    let _ = ws.close(None);
    let _ = ws.flush();
    Some(())
}

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

/// What a phone said. False closes the connection.
fn on_message<S: Read + Write>(hub: &Hub, conn: u64, ws: &mut tungstenite::WebSocket<S>, v: &Value) -> bool {
    match text(v, "t") {
        "ping" => send(ws, json!({ "t": "pong" })),
        "hello" => match hub.authenticate(conn, text(v, "token")) {
            Some(name) => send(ws, json!({ "t": "hello", "name": name, "ticket": hub.ticket(conn) })),
            None => send(ws, json!({ "t": "unpaired" })),
        },
        "pair" => {
            let (name, model, tablet) = (text(v, "name"), text(v, "model"), v.get("tablet").and_then(|b| b.as_bool()).unwrap_or(false));
            match hub.pair(conn, text(v, "code"), name, model, tablet) {
                PairResult::Waiting(number) => send(ws, json!({ "t": "waiting", "number": number })),
                PairResult::Approved { token, name, ticket } => send(ws, json!({ "t": "paired", "token": token, "name": name, "ticket": ticket })),
                PairResult::Refused(message) => send(ws, json!({ "t": "refused", "message": message })),
            }
        }
        "page" => {
            hub.set_page(conn, text(v, "page"));
            true
        }
        kind @ ("press" | "long" | "release" | "turn") => {
            if let Some(device) = hub.device_of(conn) {
                let kind = match kind {
                    "press" => Kind::Press,
                    "long" => Kind::Long,
                    "release" => Kind::Release,
                    _ => Kind::Turn(v.get("by").and_then(|b| b.as_i64()).unwrap_or(0).clamp(-50, 50) as i32),
                };
                hub.emit(PhoneEvent::Press { device, button: text(v, "button").chars().take(80).collect(), kind });
            }
            true
        }
        _ => true,
    }
}

#[cfg(test)]
pub(crate) fn index_len_for_tests() -> usize {
    INDEX.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // the page's own script and style must be exactly what the policy names, or phones show nothing
    #[test]
    fn the_policy_names_the_pages_own_script_and_style() {
        let csp = content_security_policy();
        assert_eq!(INDEX.matches("<script>").count(), 1, "one script, the one the policy allows");
        assert_eq!(INDEX.matches("<style>").count(), 1);
        assert_eq!(csp.matches("'sha256-").count(), 2, "{csp}");
        // nothing in the page needs more than the policy gives: no inline handlers or style attributes
        for needs_more in ["innerHTML", " onclick=", " onload=", " style=\"", "javascript:", "eval("] {
            assert!(!INDEX.contains(needs_more), "the phone page uses {needs_more}");
        }
    }

    #[test]
    fn another_sites_page_is_not_the_phone_page() {
        let head = |origin: Option<&str>| Head {
            raw: vec![],
            head_only: false,
            path: "/ws".into(),
            query: HashMap::new(),
            headers: [("host".to_string(), "192.168.1.5:47801".to_string())].into_iter().chain(origin.map(|o| ("origin".to_string(), o.to_string()))).collect(),
        };
        assert!(same_origin(&head(None)));
        assert!(same_origin(&head(Some("https://192.168.1.5:47801"))));
        assert!(!same_origin(&head(Some("https://evil.example"))));
        assert!(!same_origin(&head(Some("https://192.168.1.5:9999"))));
        assert!(!same_origin(&head(Some("null"))));
    }
}
