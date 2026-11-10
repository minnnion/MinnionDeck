//! Edit a button's picture in the GIF maker and Images editor, then press Sync there to put it back
//! on the button. The editor ships inside the app (src/generated/editor_files.rs, from the website's
//! editor-dist) and opens in the browser from a small server here that only this PC can reach:
//!
//!   GET  /editor/…             the editor's files (/editor → /editor/, keeping the query)
//!
//!   GET  /v1/session?token=…   {"app":"MinnionDeck","kind":"image"|"gif","has_image":bool,"name":…}
//!   GET  /v1/image?token=…     the button's picture or GIF as it is now (404: it has none)
//!   GET  /v1/layers?token=…    the same, in layers, bottom first: the button (its colour or picture)
//!                              and its icon on a layer of its own, each a 256 x 256 PNG (base64)
//!   POST /v1/image?token=…     the edited picture (PNG, JPEG, WebP) or GIF as the body: put on the button
//!   GET  /v1/hello             {"ok":true,"app":"MinnionDeck","version":…}: how an open tab finds the port
//!   GET  /v1/events            Server-Sent Events: an open website tab listens here, so Edit image
//!                              can use that tab ("event: edit") instead of opening a new one
//!   POST /v1/ack?token=…       the tab took the edit; no new tab is opened
//!
//! The server runs as long as the app does (from `init`).
//!
//! Only the website (and localhost, for working on it) may call it, and only with the key of the Edit
//! window that is open. Browsers allow an https page to reach 127.0.0.1; the preflight answers the
//! Private Network Access question too.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::Duration;

use serde_json::json;

use super::media::{self, Kind};

/// The ports tried, in order (the first free one is used)
const PORTS: std::ops::RangeInclusive<u16> = 51735..=51744;
const MAX_BODY: usize = 25 * 1024 * 1024;
/// Who may call it
const SITE: &str = "https://minnnion.github.io";

/// What the website sends back reaches the window as this: (kind, the new picture, Edit window token, look).
pub type Done = (Kind, Result<serde_json::Value, String>, u64, usize);

struct Session {
    key: String,
    editor: u64,
    look: usize,
    kind: Kind,
    name: String,
    /// The picture or GIF as it is now, to start from
    current: Option<Vec<u8>>,
    /// The same in layers, bottom first
    layers: Vec<Layer>,
}

/// One layer of a look for the editor: its name, what it is ("background", "picture", "icon"), the
/// icon's id for an icon layer, and the layer as a PNG.
#[derive(Clone)]
pub struct Layer {
    pub name: String,
    pub role: &'static str,
    pub icon: String,
    pub png: Vec<u8>,
}

struct Server {
    port: u16,
    tx: Sender<Done>,
    repaint: Box<dyn Fn() + Send + Sync>,
    media_dir: std::path::PathBuf,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static SERVER: Mutex<Option<Server>> = Mutex::new(None);
/// The website tabs listening on /v1/events, oldest first
static CLIENTS: Mutex<Vec<TcpStream>> = Mutex::new(Vec::new());
/// The key a tab said yes to (/v1/ack)
static ACKED: Mutex<Option<String>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn new_key() -> String {
    let mut b = [0u8; 16];
    let _ = ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Starts the server (once). Returns its port.
fn ensure_server(media_dir: &std::path::Path, tx: Sender<Done>, repaint: Box<dyn Fn() + Send + Sync>) -> Result<u16, String> {
    let mut s = lock(&SERVER);
    if let Some(srv) = s.as_ref() {
        return Ok(srv.port);
    }
    let listener = PORTS.clone().find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok()).ok_or("Couldn't start the link to the website (no free port).")?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    *s = Some(Server { port, tx, repaint, media_dir: media_dir.to_path_buf() });
    std::thread::Builder::new()
        .name("websync".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let _ = handle(stream);
                });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(port)
}

/// Starts the link as the app starts, so an open website tab can find it.
pub fn init(media_dir: &std::path::Path, tx: Sender<Done>, repaint: impl Fn() + Send + Sync + 'static) -> Result<u16, String> {
    let port = ensure_server(media_dir, tx, Box::new(repaint))?;
    static PING: std::sync::Once = std::sync::Once::new();
    PING.call_once(|| {
        // every 15 s a comment line: a tab that went away shows up as a failed write, and is dropped
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(Duration::from_secs(15));
                lock(&CLIENTS).retain_mut(|c| c.write_all(b": ping\n\n").and_then(|_| c.flush()).is_ok());
            }
        });
    });
    Ok(port)
}

/// Edit image with a website tab already open: the newest listening tab is told to edit, and has
/// 2 s to say yes. True when it did (then no new tab is needed). The browser is brought forward.
pub fn send_to_open_tab(url: &str) -> bool {
    let (key, port) = {
        let s = lock(&SESSION);
        let Some(s) = s.as_ref() else { return false };
        (s.key.clone(), lock(&SERVER).as_ref().map(|x| x.port).unwrap_or(0))
    };
    let view = if url.ends_with("#gifs") { "gifs" } else { "images" };
    let msg = format!("event: edit\ndata: {}\n\n", json!({ "view": view, "token": key, "port": port }));
    *lock(&ACKED) = None;
    {
        let mut clients = lock(&CLIENTS);
        // the newest tab that is still there
        loop {
            let Some(mut c) = clients.pop() else { return false };
            if c.write_all(msg.as_bytes()).and_then(|_| c.flush()).is_ok() {
                clients.push(c);
                break;
            }
        }
    }
    let t0 = std::time::Instant::now();
    while t0.elapsed() < Duration::from_secs(2) {
        if lock(&ACKED).as_deref() == Some(key.as_str()) {
            focus_browser();
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

/// Best effort: the browser window whose tab is linked ("Edit image · MinnionDeck") comes forward.
#[cfg(windows)]
fn focus_browser() {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, IsIconic, IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow};
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
        let title = String::from_utf16_lossy(&buf[..n]);
        if unsafe { IsWindowVisible(hwnd) }.as_bool() && title.contains("Edit image") && title.contains("MinnionDeck") && !title.starts_with("MinnionDeck tray") {
            unsafe { *(lparam.0 as *mut isize) = hwnd.0 as isize };
            return false.into();
        }
        true.into()
    }
    let mut found: isize = 0;
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut isize as isize));
        if found != 0 {
            let h = HWND(found as *mut _);
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
    }
}

#[cfg(not(windows))]
fn focus_browser() {}

/// The website's address to edit look `look` of the Edit window `editor` with. `current` is the
/// picture (or GIF) it has now.
pub fn start(media_dir: &std::path::Path, tx: Sender<Done>, repaint: impl Fn() + Send + Sync + 'static, editor: u64, look: usize, kind: Kind, name: &str, current: Option<Vec<u8>>, layers: Vec<Layer>) -> Result<String, String> {
    let port = ensure_server(media_dir, tx, Box::new(repaint))?;
    let key = new_key();
    let page = if kind == Kind::Gif { "gifs" } else { "images" };
    *lock(&SESSION) = Some(Session { key: key.clone(), editor, look, kind, name: name.to_string(), current, layers });
    Ok(format!("http://127.0.0.1:{port}/editor/?token={key}#{page}"))
}

/// The Edit window closed: its key stops working.
pub fn stop(editor: u64) {
    let mut s = lock(&SESSION);
    if s.as_ref().is_some_and(|x| x.editor == editor) {
        *s = None;
    }
}

struct Request {
    method: String,
    path: String,
    query: String,
    origin: Option<String>,
    host: Option<String>,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    stream.set_read_timeout(Some(Duration::from_secs(20))).ok()?;
    let mut r = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    r.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let (path, query) = target.split_once('?').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((target, String::new()));
    let (mut len, mut origin, mut host) = (0usize, None, None);
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).ok()? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => len = v.trim().parse().ok()?,
                "origin" => origin = Some(v.trim().to_string()),
                "host" => host = Some(v.trim().to_ascii_lowercase()),
                _ => {}
            }
        }
    }
    if len > MAX_BODY {
        return None;
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).ok()?;
    Some(Request { method, path, query, origin, host, body })
}

/// The website, or a page on this PC (for working on the website).
fn allowed(origin: &str) -> bool {
    origin == SITE || origin.starts_with("http://localhost:") || origin.starts_with("http://127.0.0.1:") || origin == "http://localhost" || origin == "http://127.0.0.1"
}

fn reply(stream: &mut TcpStream, status: &str, origin: Option<&str>, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n", body.len());
    if let Some(o) = origin {
        head += &format!("Access-Control-Allow-Origin: {o}\r\nVary: Origin\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Allow-Private-Network: true\r\nAccess-Control-Max-Age: 600\r\n");
    }
    head += "\r\n";
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn json_reply(stream: &mut TcpStream, status: &str, origin: Option<&str>, v: serde_json::Value) -> std::io::Result<()> {
    reply(stream, status, origin, "application/json", v.to_string().as_bytes())
}

/// The Edit window's key and the one sent, compared in full whatever the first wrong character is
fn same_key(want: &str, got: &str) -> bool {
    !got.is_empty() && want.len() == got.len() && want.bytes().zip(got.bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

fn token_of(query: &str) -> String {
    query.split('&').find_map(|kv| kv.strip_prefix("token=")).unwrap_or("").to_string()
}

fn handle(mut stream: TcpStream) -> std::io::Result<()> {
    let Some(req) = read_request(&mut stream) else {
        return reply(&mut stream, "400 Bad Request", None, "text/plain", b"bad request");
    };
    // only the website may ask; a page from anywhere else gets nothing it can read
    let origin = match req.origin.as_deref() {
        Some(o) if allowed(o) => Some(o.to_string()),
        Some(_) => return reply(&mut stream, "403 Forbidden", None, "text/plain", b"not allowed"),
        None => None,
    };
    let o = origin.as_deref();
    // only this PC's own names: a web page can't reach the server through a name it controls
    let port = lock(&SERVER).as_ref().map(|s| s.port).unwrap_or(0);
    if !req.host.as_deref().is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}")) {
        return reply(&mut stream, "421 Misdirected Request", None, "text/plain", b"wrong host");
    }
    // the editor itself
    if req.method == "GET" && (req.path == "/editor" || req.path.starts_with("/editor/")) {
        if req.path == "/editor" {
            let to = if req.query.is_empty() { "/editor/".to_string() } else { format!("/editor/?{}", req.query) };
            stream.write_all(format!("HTTP/1.1 301 Moved Permanently\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes())?;
            return stream.flush();
        }
        let rel = req.path.trim_start_matches("/editor/");
        let rel = if rel.is_empty() { "index.html" } else { rel };
        // only the files that ship in the app: nothing else on the disk can be asked for
        return match crate::generated::editor_files::FILES.iter().find(|(name, _)| *name == rel) {
            Some((name, bytes)) => {
                let ctype = match name.rsplit('.').next().unwrap_or("") {
                    "html" => "text/html; charset=utf-8",
                    "js" => "text/javascript",
                    "css" => "text/css",
                    "woff2" => "font/woff2",
                    "png" => "image/png",
                    "svg" => "image/svg+xml",
                    "json" => "application/json",
                    _ => "application/octet-stream",
                };
                reply(&mut stream, "200 OK", o, ctype, bytes)
            }
            None => reply(&mut stream, "404 Not Found", o, "text/plain", b"not found"),
        };
    }
    if req.method == "OPTIONS" {
        return reply(&mut stream, "204 No Content", o, "text/plain", b"");
    }
    // no key needed: finding the app, and listening for Edit image
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/v1/hello") => return json_reply(&mut stream, "200 OK", o, json!({ "ok": true, "app": "MinnionDeck", "version": env!("CARGO_PKG_VERSION") })),
        ("GET", "/v1/events") => {
            let mut head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n".to_string();
            if let Some(o) = o {
                head += &format!("Access-Control-Allow-Origin: {o}\r\nVary: Origin\r\nAccess-Control-Allow-Private-Network: true\r\n");
            }
            head += "\r\n: hello\n\n";
            stream.write_all(head.as_bytes())?;
            stream.flush()?;
            stream.set_read_timeout(None)?;
            lock(&CLIENTS).push(stream);
            return Ok(());
        }
        _ => {}
    }
    let key = token_of(&req.query);
    let session = {
        let s = lock(&SESSION);
        s.as_ref().filter(|x| same_key(&x.key, &key)).map(|x| (x.editor, x.look, x.kind, x.name.clone(), x.current.clone(), x.layers.clone()))
    };
    let Some((editor, look, kind, name, current, layers)) = session else {
        return json_reply(&mut stream, "401 Unauthorized", o, json!({ "ok": false, "error": "This link has expired. Press Edit image in MinnionDeck again." }));
    };
    let word = if kind == Kind::Gif { "gif" } else { "image" };
    match (req.method.as_str(), req.path.as_str()) {
        ("POST", "/v1/ack") => {
            *lock(&ACKED) = Some(key.clone());
            json_reply(&mut stream, "200 OK", o, json!({ "ok": true }))
        }
        ("GET", "/v1/session") => json_reply(&mut stream, "200 OK", o, json!({ "ok": true, "app": "MinnionDeck", "version": env!("CARGO_PKG_VERSION"), "kind": word, "has_image": current.is_some(), "layers": layers.len(), "name": name })),
        ("GET", "/v1/layers") => {
            use base64::Engine as _;
            let list: Vec<serde_json::Value> = layers.iter().map(|l| json!({ "name": l.name, "role": l.role, "icon": l.icon, "png": base64::engine::general_purpose::STANDARD.encode(&l.png) })).collect();
            json_reply(&mut stream, "200 OK", o, json!({ "ok": true, "size": 256, "layers": list }))
        }
        ("GET", "/v1/image") => match current {
            Some(bytes) => {
                let ctype = match image::guess_format(&bytes) {
                    Ok(image::ImageFormat::Gif) => "image/gif",
                    Ok(image::ImageFormat::Jpeg) => "image/jpeg",
                    Ok(image::ImageFormat::WebP) => "image/webp",
                    _ => "image/png",
                };
                reply(&mut stream, "200 OK", o, ctype, &bytes)
            }
            None => json_reply(&mut stream, "404 Not Found", o, json!({ "ok": false, "error": "This button has no picture yet." })),
        },
        ("POST", "/v1/image") => {
            if req.body.is_empty() {
                return json_reply(&mut stream, "400 Bad Request", o, json!({ "ok": false, "error": "Nothing was sent." }));
            }
            // a GIF sent for a picture look is used as the look's GIF, and the other way round for a still picture
            let is_gif = matches!(image::guess_format(&req.body), Ok(image::ImageFormat::Gif));
            let as_kind = if is_gif && kind == Kind::Gif { Kind::Gif } else if kind == Kind::Gif && !is_gif { Kind::Picture } else { kind };
            let (media_dir, tx) = {
                let s = lock(&SERVER);
                let Some(srv) = s.as_ref() else { return json_reply(&mut stream, "500 Internal Server Error", o, json!({ "ok": false })) };
                (srv.media_dir.clone(), srv.tx.clone())
            };
            let label = if name.is_empty() { "From the website".to_string() } else { name.clone() };
            let r = media::import_bytes(&media_dir, &req.body, &label, as_kind);
            let answer = match &r {
                Ok(_) => json!({ "ok": true, "message": "Synced to MinnionDeck" }),
                Err(e) => json!({ "ok": false, "error": e }),
            };
            if r.is_ok() {
                // what it is now, for another round of editing
                if let Some(s) = lock(&SESSION).as_mut().filter(|x| x.key == key) {
                    s.current = Some(req.body.clone());
                }
            }
            let _ = tx.send((as_kind, r, editor, look));
            if let Some(srv) = lock(&SERVER).as_ref() {
                (srv.repaint)();
            }
            json_reply(&mut stream, if answer["ok"] == true { "200 OK" } else { "422 Unprocessable Entity" }, o, answer)
        }
        _ => json_reply(&mut stream, "404 Not Found", o, json!({ "ok": false, "error": "Unknown request." })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(port: u16, req: &str, body: &[u8]) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        s.write_all(body).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn the_website_gets_the_picture_and_sends_it_back() {
        let dir = std::env::temp_dir().join(format!("md-websync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let layers = vec![Layer { name: "Button".into(), role: "background", icon: String::new(), png: png.clone() }, Layer { name: "Icon".into(), role: "icon", icon: "play".into(), png: png.clone() }];
        let url = start(&dir, tx, || {}, 7, 1, Kind::Picture, "Mute", Some(png.clone()), layers).unwrap();
        assert!(url.starts_with("http://127.0.0.1:") && url.contains("/editor/?token=") && url.ends_with("#images"), "{url}");
        let port: u16 = url.trim_start_matches("http://127.0.0.1:").split('/').next().unwrap().parse().unwrap();
        let key = url.split("token=").nth(1).unwrap().split('#').next().unwrap().to_string();
        let site = &format!("Host: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\n");
        // the editor is served from the app, and only its own files
        let page = http(port, &format!("GET /editor/?token={key} HTTP/1.1\r\n{site}\r\n"), b"");
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("text/html"), "{}", &page[..page.len().min(200)]);
        assert!(http(port, &format!("GET /editor?token={key} HTTP/1.1\r\n{site}\r\n"), b"").contains(&format!("Location: /editor/?token={key}")));
        assert!(http(port, &format!("GET /editor/../Cargo.toml HTTP/1.1\r\n{site}\r\n"), b"").starts_with("HTTP/1.1 404"));
        // a page reaching it through another name (DNS rebinding) gets nothing
        assert!(http(port, &format!("GET /v1/hello HTTP/1.1\r\nHost: evil.example:{port}\r\n\r\n"), b"").starts_with("HTTP/1.1 421"));
        // the preflight lets the https page in
        let pre = http(port, &format!("OPTIONS /v1/image?token={key} HTTP/1.1\r\n{site}Access-Control-Request-Private-Network: true\r\n\r\n"), b"");
        assert!(pre.starts_with("HTTP/1.1 204") && pre.contains("Access-Control-Allow-Private-Network: true"), "{pre}");
        // the session, and the picture it has now
        let s = http(port, &format!("GET /v1/session?token={key} HTTP/1.1\r\n{site}\r\n"), b"");
        assert!(s.contains("\"has_image\":true") && s.contains("\"kind\":\"image\"") && s.contains("\"layers\":2"), "{s}");
        // the layers, bottom first: the button, then its icon
        let ly = http(port, &format!("GET /v1/layers?token={key} HTTP/1.1\r\n{site}\r\n"), b"");
        let body: serde_json::Value = serde_json::from_str(ly.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["layers"][0]["role"], "background");
        assert_eq!(body["layers"][1]["role"], "icon");
        assert_eq!(body["layers"][1]["icon"], "play");
        assert!(body["layers"][1]["png"].as_str().unwrap().starts_with("iVBOR"), "a PNG in base64");
        let img = http(port, &format!("GET /v1/image?token={key} HTTP/1.1\r\n{site}\r\n"), b"");
        assert!(img.contains("Content-Type: image/png"), "{img}");
        // another site, or a wrong key, gets nothing
        assert!(http(port, &format!("GET /v1/session?token={key} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: https://evil.example\r\n\r\n"), b"").starts_with("HTTP/1.1 403"));
        assert!(http(port, &format!("GET /v1/session?token=nope HTTP/1.1\r\n{site}\r\n"), b"").starts_with("HTTP/1.1 401"));
        // Sync: the edited picture goes onto look 1 of Edit window 7
        let mut green = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 255, 0, 255])).write_to(&mut std::io::Cursor::new(&mut green), image::ImageFormat::Png).unwrap();
        let post = http(port, &format!("POST /v1/image?token={key} HTTP/1.1\r\n{site}Content-Type: image/png\r\nContent-Length: {}\r\n\r\n", green.len()), &green);
        assert!(post.starts_with("HTTP/1.1 200") && post.contains("Synced"), "{post}");
        let (kind, r, editor, look) = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(kind == Kind::Picture && editor == 7 && look == 1);
        let entry = r.unwrap();
        assert!(media::file_for(&dir, entry["media"].as_str().unwrap()).is_some(), "kept, sealed, in the media folder");
        // something that isn't a picture is refused
        assert!(http(port, &format!("POST /v1/image?token={key} HTTP/1.1\r\n{site}Content-Length: 5\r\n\r\n"), b"hello").starts_with("HTTP/1.1 422"));
        // the site finds the app, and an open tab listening gets Edit image instead of a new tab
        assert!(http(port, &format!("GET /v1/hello HTTP/1.1\r\n{site}\r\n"), b"").contains("\"app\":\"MinnionDeck\""));
        let mut tab = TcpStream::connect(("127.0.0.1", port)).unwrap();
        tab.write_all(format!("GET /v1/events HTTP/1.1\r\n{site}\r\n").as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let u = url.clone();
        let asked = std::thread::spawn(move || send_to_open_tab(&u));
        let mut got = Vec::new();
        let mut buf = [0u8; 1024];
        tab.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        while !String::from_utf8_lossy(&got).contains("event: edit") {
            let n = tab.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
        }
        let text = String::from_utf8_lossy(&got).into_owned();
        assert!(text.contains("text/event-stream") && text.contains(&format!("\"token\":\"{key}\"")) && text.contains("\"view\":\"images\""), "{text}");
        assert!(http(port, &format!("POST /v1/ack?token={key} HTTP/1.1\r\n{site}Content-Length: 0\r\n\r\n"), b"").starts_with("HTTP/1.1 200"));
        assert!(asked.join().unwrap(), "the open tab took it");
        drop(tab);
        // no tab answers: a new one is opened instead
        assert!(!send_to_open_tab(&url));
        // the window closed: the key stops working
        stop(7);
        assert!(http(port, &format!("GET /v1/session?token={key} HTTP/1.1\r\n{site}\r\n"), b"").starts_with("HTTP/1.1 401"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
