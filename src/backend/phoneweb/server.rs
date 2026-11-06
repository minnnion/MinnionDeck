//! The phone server: HTTPS for the page and its pictures, and one WebSocket per phone for live
//! buttons. One thread for each connection; plain http:// on the port is sent to https://.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, channel};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{Value, json};
use tungstenite::Message;

use super::{Hub, Kind, Out, PairResult, PhoneEvent, pics};
use crate::backend::media;

const INDEX: &str = include_str!("../../../assets/phone/index.html");
const MANIFEST: &str = include_str!("../../../assets/phone/manifest.webmanifest");
const FONT_400: &[u8] = include_bytes!("../../../assets/fonts/PixelifySans-400.ttf");
const FONT_600: &[u8] = include_bytes!("../../../assets/fonts/PixelifySans-600.ttf");
const FONT_700: &[u8] = include_bytes!("../../../assets/fonts/PixelifySans-700.ttf");

const MAX_HEAD: usize = 16 * 1024;
const MAX_MESSAGE: usize = 4096;
/// A phone that says nothing this long is gone
const SILENCE: Duration = Duration::from_secs(45);

/// Serves the page on a plain port (what the QR opens: no certificate, so no warning) and, when
/// there is a certificate, on a second port over HTTPS.
pub fn serve(hub: Arc<Hub>, http: TcpListener, https: Option<(TcpListener, Arc<ServerConfig>)>) {
    let h = hub.clone();
    std::thread::Builder::new()
        .name("phone server".into())
        .spawn(move || {
            for stream in http.incoming().flatten() {
                let hub = h.clone();
                let _ = std::thread::Builder::new().name("phone connection".into()).spawn(move || {
                    let _ = handle_plain(hub, stream);
                });
            }
        })
        .ok();
    if let Some((listener, tls)) = https {
        std::thread::Builder::new()
            .name("phone server (https)".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let (hub, tls) = (hub.clone(), tls.clone());
                    let _ = std::thread::Builder::new().name("phone connection".into()).spawn(move || {
                        let _ = handle(hub, tls, stream);
                    });
                }
            })
            .ok();
    }
}

/// A stream whose reads can be made to wait only a moment.
trait Timed {
    fn read_timeout(&mut self, d: Duration);
}

impl Timed for Replay<Tls> {
    fn read_timeout(&mut self, d: Duration) {
        self.inner.sock.set_read_timeout(Some(d)).ok();
    }
}

impl Timed for Replay<TcpStream> {
    fn read_timeout(&mut self, d: Duration) {
        self.inner.set_read_timeout(Some(d)).ok();
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

type Tls = StreamOwned<ServerConnection, TcpStream>;

struct Head {
    raw: Vec<u8>,
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
            b'%' if i + 2 < b.len() + 0 && s.get(i + 1..i + 3).is_some() => {
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
    Some(Head { raw, path: path.to_string(), query, headers })
}

fn respond(s: &mut impl Write, status: &str, ctype: &str, body: &[u8], cache: &str) {
    let head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: {cache}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n", body.len());
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(body);
    let _ = s.flush();
}

/// A connection on the plain port.
fn handle_plain(hub: Arc<Hub>, mut tcp: TcpStream) -> Option<()> {
    tcp.set_nodelay(true).ok();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(10))).ok();
    // someone starting a TLS handshake here wants the other port
    let mut first = [0u8; 1];
    tcp.peek(&mut first).ok()?;
    if first[0] == 0x16 {
        return None;
    }
    let head = read_head(&mut tcp)?;
    let wants_ws = head.headers.get("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    if wants_ws && head.path == "/ws" {
        let replay = Replay { buf: head.raw, pos: 0, inner: tcp };
        return websocket(hub, replay);
    }
    route(&hub, &head, &mut tcp);
    let _ = tcp.flush();
    Some(())
}

/// A connection on the HTTPS port.
fn handle(hub: Arc<Hub>, tls: Arc<ServerConfig>, tcp: TcpStream) -> Option<()> {
    tcp.set_nodelay(true).ok();
    tcp.set_read_timeout(Some(Duration::from_secs(10))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(10))).ok();
    // 0x16 starts a TLS handshake; anything else is someone who typed http://
    let mut first = [0u8; 1];
    tcp.peek(&mut first).ok()?;
    if first[0] != 0x16 {
        let mut plain = tcp;
        let head = read_head(&mut plain)?;
        let host = head.headers.get("host").cloned().unwrap_or_default();
        let host = host.split(':').next().unwrap_or("").to_string();
        let ok = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        let to = format!("https://{}:{}/", if ok { host } else { "localhost".into() }, hub.tls_port);
        let msg = format!("HTTP/1.1 301 Moved Permanently\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = plain.write_all(msg.as_bytes());
        return Some(());
    }
    let conn = ServerConnection::new(tls).ok()?;
    let mut s: Tls = StreamOwned::new(conn, tcp);
    let head = read_head(&mut s)?;
    let wants_ws = head.headers.get("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    if wants_ws && head.path == "/ws" {
        let replay = Replay { buf: head.raw, pos: 0, inner: s };
        return websocket(hub, replay);
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
    // pictures are for paired phones only
    let paired = || hub.token_ok(q("t"));
    match h.path.as_str() {
        "/" | "/index.html" => respond(s, "200 OK", "text/html; charset=utf-8", INDEX.as_bytes(), "no-cache"),
        "/manifest.webmanifest" => respond(s, "200 OK", "application/manifest+json", MANIFEST.as_bytes(), "no-cache"),
        "/app-icon-192.png" | "/apple-touch-icon.png" => match pics::app_icon_png(192) {
            Some(b) => respond(s, "200 OK", "image/png", &b, "max-age=86400"),
            None => respond(s, "404 Not Found", "text/plain", b"no", "no-cache"),
        },
        "/app-icon-512.png" => match pics::app_icon_png(512) {
            Some(b) => respond(s, "200 OK", "image/png", &b, "max-age=86400"),
            None => respond(s, "404 Not Found", "text/plain", b"no", "no-cache"),
        },
        "/minniondeck.cer" => match hub.cert_der() {
            Some(b) => respond(s, "200 OK", "application/x-x509-ca-cert", &b, "no-cache"),
            None => respond(s, "404 Not Found", "text/plain", b"no", "no-cache"),
        },
        "/font-400.ttf" => respond(s, "200 OK", "font/ttf", FONT_400, "max-age=86400"),
        "/font-600.ttf" => respond(s, "200 OK", "font/ttf", FONT_600, "max-age=86400"),
        "/font-700.ttf" => respond(s, "200 OK", "font/ttf", FONT_700, "max-age=86400"),
        "/icon" if paired() => {
            let c = q("c");
            let ink = u32::from_str_radix(c.trim_start_matches('#'), 16).ok().filter(|_| c.trim_start_matches('#').len() == 6).map(|v| [(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]).unwrap_or([0, 0, 0, 255]);
            match pics::icon_png(q("id"), ink) {
                Some(b) => respond(s, "200 OK", "image/png", &b, "max-age=3600"),
                None => respond(s, "404 Not Found", "text/plain", b"no", "no-cache"),
            }
        }
        "/media" if paired() => match media::picture(q("id")).and_then(|p| p.file).and_then(|f| media::read_bytes(&f).map(|b| (ctype_of(&f), b))) {
            Some((ct, b)) => respond(s, "200 OK", ct, &b, "max-age=3600"),
            None => respond(s, "404 Not Found", "text/plain", b"no", "no-cache"),
        },
        "/gif" if paired() => {
            let bytes = media::gif(q("id")).and_then(|g| g.src).and_then(|src| match src {
                media::GifSource::File(p) => media::read_bytes(&p),
                media::GifSource::Drawn { size, fps, frames } => pics::drawn_gif(size, fps, &frames),
            });
            match bytes {
                Some(b) => respond(s, "200 OK", "image/gif", &b, "max-age=3600"),
                None => respond(s, "404 Not Found", "text/plain", b"no", "no-cache"),
            }
        }
        "/icon" | "/media" | "/gif" => respond(s, "403 Forbidden", "text/plain", b"Pair this phone first.", "no-cache"),
        _ => respond(s, "404 Not Found", "text/plain", b"Not found", "no-cache"),
    }
}

fn send<S: Read + Write>(ws: &mut tungstenite::WebSocket<S>, v: Value) -> bool {
    ws.send(Message::text(v.to_string())).is_ok()
}

fn websocket<S: Read + Write + Timed>(hub: Arc<Hub>, stream: S) -> Option<()> {
    let mut ws = tungstenite::accept(stream).ok()?;
    // from here on a read waits a moment, so this thread can also send what the PC has to say
    ws.get_mut().read_timeout(Duration::from_millis(100));
    let (otx, orx): (_, Receiver<Out>) = channel();
    let conn = hub.register(otx);
    let mut sent_version = 0u64;
    let mut last_heard = Instant::now();
    let mut announced = false;
    'conn: loop {
        match ws.read() {
            Ok(Message::Text(t)) => {
                last_heard = Instant::now();
                if t.len() <= MAX_MESSAGE {
                    if let Ok(v) = serde_json::from_str::<Value>(t.as_str()) {
                        if !on_message(&hub, conn, &mut ws, &v) {
                            break 'conn;
                        }
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
        // a phone that is in sees the deck, and every change to it
        if hub.device_of(conn).is_some() {
            let (v, snap) = hub.snapshot();
            if v != sent_version || !announced {
                if !snap.is_empty() && ws.send(Message::text(format!("{{\"t\":\"snap\",\"deck\":{snap}}}"))).is_err() {
                    break;
                }
                sent_version = v;
                announced = true;
            }
        }
    }
    hub.unregister(conn);
    let _ = ws.close(None);
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
            Some(name) => send(ws, json!({ "t": "hello", "name": name })),
            None => send(ws, json!({ "t": "unpaired" })),
        },
        "pair" => {
            let (name, model, tablet) = (text(v, "name"), text(v, "model"), v.get("tablet").and_then(|b| b.as_bool()).unwrap_or(false));
            match hub.pair(conn, text(v, "code"), name, model, tablet) {
                PairResult::Waiting(number) => send(ws, json!({ "t": "waiting", "number": number })),
                PairResult::Approved { token, name } => send(ws, json!({ "t": "paired", "token": token, "name": name })),
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
