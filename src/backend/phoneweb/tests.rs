//! The phone server, tried with a real TLS and WebSocket client on this PC.
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use super::*;

/// A phone's browser after "Continue anyway": it takes any certificate.
#[derive(Debug)]
struct TakeAny;

impl ServerCertVerifier for TakeAny {
    fn verify_server_cert(&self, _: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(&self, _: &[u8], _: &CertificateDer<'_>, _: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
    }
}

type Tls = StreamOwned<ClientConnection, TcpStream>;

fn tls(port: u16) -> Tls {
    let cfg = ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(TakeAny))
        .with_no_client_auth();
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    StreamOwned::new(ClientConnection::new(Arc::new(cfg), ServerName::try_from("localhost").unwrap()).unwrap(), tcp)
}

/// GET over https: (status line, body)
fn get(port: u16, path: &str) -> (String, Vec<u8>) {
    let mut s = tls(port);
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut all = Vec::new();
    let _ = s.read_to_end(&mut all);
    let split = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(all.len());
    let head = String::from_utf8_lossy(&all[..split]).into_owned();
    (head.lines().next().unwrap_or("").to_string(), all.get(split + 4..).unwrap_or(&[]).to_vec())
}

fn ws(port: u16) -> WebSocket<Tls> {
    let (socket, _) = tungstenite::client(format!("ws://localhost:{port}/ws"), tls(port)).unwrap();
    socket
}

fn say(w: &mut WebSocket<Tls>, v: Value) {
    w.send(Message::text(v.to_string())).unwrap();
}

/// The next message of this type (others, like the deck, may come first).
fn want(w: &mut WebSocket<Tls>, t: &str) -> Value {
    for _ in 0..40 {
        let Message::Text(txt) = w.read().unwrap() else { continue };
        let v: Value = serde_json::from_str(txt.as_str()).unwrap();
        if v["t"] == t {
            return v;
        }
    }
    panic!("no {t} message");
}

fn server(tag: &str) -> (Arc<Hub>, Receiver<PhoneEvent>, PathBuf) {
    let root = std::env::temp_dir().join(format!("md-phone-{tag}-{}", now_ms()));
    let (tx, rx) = channel();
    let hub = start(&root, 49000 + (now_ms() % 500) as u16, tx, || {}).expect("the phone server starts");
    (hub, rx, root)
}

#[test]
fn the_page_is_served_and_pictures_need_a_paired_phone() {
    let (hub, _rx, root) = server("page");
    let (status, body) = get(hub.tls_port, "/");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(String::from_utf8_lossy(&body).contains("Pair with your PC"));
    assert_eq!(get(hub.tls_port, "/manifest.webmanifest").0, "HTTP/1.1 200 OK");
    let (s, icon) = get(hub.tls_port, "/app-icon-192.png");
    assert_eq!(s, "HTTP/1.1 200 OK");
    assert!(icon.starts_with(&[0x89, b'P', b'N', b'G']));
    assert_eq!(get(hub.tls_port, "/font-600.ttf").0, "HTTP/1.1 200 OK");
    // pictures: not without a paired phone's key
    assert_eq!(get(hub.tls_port, "/icon?id=play&c=ff0000").0, "HTTP/1.1 403 Forbidden");
    assert_eq!(get(hub.tls_port, "/media?id=x&t=nope").0, "HTTP/1.1 403 Forbidden");
    assert_eq!(get(hub.tls_port, "/nothing-here").0, "HTTP/1.1 404 Not Found");
    // http:// on the port goes to https://
    let mut plain = TcpStream::connect(("127.0.0.1", hub.tls_port)).unwrap();
    plain.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    plain.write_all(b"GET / HTTP/1.1\r\nHost: 192.168.1.5:47800\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = plain.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 301") && out.contains(&format!("Location: https://192.168.1.5:{}/", hub.tls_port)), "{out}");
    let _ = fs::remove_dir_all(&root);
}

/// GET over plain http: (status line, body)
fn http_get(port: u16, path: &str) -> (String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut all = Vec::new();
    let _ = s.read_to_end(&mut all);
    let split = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(all.len());
    (String::from_utf8_lossy(&all[..split]).lines().next().unwrap_or("").to_string(), all.get(split + 4..).unwrap_or(&[]).to_vec())
}

#[test]
fn too_many_connections_at_once_are_closed_and_places_come_back() {
    let (hub, _rx, root) = server("cap");
    // fill every place with connections that say nothing (half on each port)
    let held: Vec<TcpStream> = (0..super::server::MAX_CONNECTIONS)
        .map(|i| TcpStream::connect(("127.0.0.1", if i % 2 == 0 { hub.port } else { hub.tls_port })).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(300));
    // one more is closed straight away, on either port
    for port in [hub.port, hub.tls_port] {
        let mut extra = TcpStream::connect(("127.0.0.1", port)).unwrap();
        extra.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let _ = extra.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n");
        let mut out = Vec::new();
        let t0 = std::time::Instant::now();
        let _ = extra.read_to_end(&mut out);
        assert!(out.is_empty() && t0.elapsed() < Duration::from_secs(2), "the extra connection was served");
    }
    // once they go, the page is served again
    drop(held);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(http_get(hub.port, "/").0, "HTTP/1.1 200 OK");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_plain_page_has_no_certificate_and_pairs_the_same() {
    let (hub, rx, root) = server("plain");
    assert!(hub.tls_port > hub.port && hub.base_url().starts_with("http://") && hub.secure_url().unwrap().starts_with("https://"));
    // the QR opens the plain page, or the HTTPS one when Secure phone link is on; the code rides after #
    assert!(hub.url().starts_with(&hub.base_url()) && hub.url().ends_with(&format!("#{}", hub.code())));
    hub.secure_link.store(true, Ordering::Relaxed);
    assert_eq!(hub.page_url(), hub.secure_url().unwrap());
    assert!(hub.url().starts_with("https://") && hub.url().ends_with(&format!("#{}", hub.code())));
    hub.secure_link.store(false, Ordering::Relaxed);
    let (status, body) = http_get(hub.port, "/");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(String::from_utf8_lossy(&body).contains("Pair with your PC"));
    assert_eq!(http_get(hub.port, "/icon?id=play&c=ff0000").0, "HTTP/1.1 403 Forbidden");
    // pair and press over ws:// on the plain port
    let tcp = TcpStream::connect(("127.0.0.1", hub.port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let (mut phone, _) = tungstenite::client(format!("ws://localhost:{}/ws", hub.port), tcp).unwrap();
    let read_t = |w: &mut WebSocket<TcpStream>, t: &str| -> Value {
        for _ in 0..40 {
            let Message::Text(txt) = w.read().unwrap() else { continue };
            let v: Value = serde_json::from_str(txt.as_str()).unwrap();
            if v["t"] == t {
                return v;
            }
        }
        panic!("no {t}");
    };
    phone.send(Message::text(json!({ "t": "pair", "code": hub.code(), "name": "Plain phone" }).to_string())).unwrap();
    read_t(&mut phone, "waiting");
    hub.approve(hub.pending()[0].conn);
    let tok = read_t(&mut phone, "paired")["token"].as_str().unwrap().to_string();
    hub.set_snapshot(r#"{"profile":"Plain"}"#.to_string());
    assert_eq!(read_t(&mut phone, "snap")["deck"]["profile"], "Plain");
    phone.send(Message::text(json!({ "t": "press", "button": "p-1" }).to_string())).unwrap();
    let button = loop {
        if let PhoneEvent::Press { button, .. } = rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            break button;
        }
    };
    assert_eq!(button, "p-1");
    assert_eq!(http_get(hub.port, &format!("/icon?id=play&c=ff0000&t={tok}")).0, "HTTP/1.1 200 OK");
    // a TLS handshake on the plain port is not answered
    let mut wrong = tls(hub.port);
    let mut sink = [0u8; 1];
    assert!(wrong.read(&mut sink).map(|n| n == 0).unwrap_or(true));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_phone_pairs_with_the_code_is_allowed_and_then_uses_its_key() {
    let (hub, rx, root) = server("pair");
    let mut phone = ws(hub.tls_port);
    // a wrong code
    say(&mut phone, json!({ "t": "pair", "code": "000000x", "name": "Pixel 8" }));
    assert_eq!(want(&mut phone, "refused")["t"], "refused");
    // the right one: the PC is asked, with a number to compare
    say(&mut phone, json!({ "t": "pair", "code": hub.code(), "name": "Pixel 8", "model": "Android", "tablet": false }));
    let waiting = want(&mut phone, "waiting");
    let number = waiting["number"].as_str().unwrap().to_string();
    let pending = hub.pending();
    assert_eq!((pending.len(), pending[0].name.as_str(), pending[0].number.as_str()), (1, "Pixel 8", number.as_str()));
    hub.approve(pending[0].conn);
    let paired = want(&mut phone, "paired");
    let tok = paired["token"].as_str().unwrap().to_string();
    assert!(tok.len() >= 40);
    // it shows in the list, online, and the code is used up
    assert_eq!(hub.phones().len(), 1);
    assert!(hub.phones()[0].online);
    assert_eq!(hub.connected(), 1);
    // the deck, live: set after the phone is in
    hub.set_snapshot(r#"{"profile":"Demo","pages":[]}"#.to_string());
    let snap = want(&mut phone, "snap");
    assert_eq!(snap["deck"]["profile"], "Demo");
    // a press goes to the PC with the phone's id, and the answer goes back
    say(&mut phone, json!({ "t": "press", "button": "t-1" }));
    say(&mut phone, json!({ "t": "turn", "button": "t-2", "by": 3 }));
    let device = hub.phones()[0].id.clone();
    let mut got = vec![];
    while got.len() < 2 {
        if let PhoneEvent::Press { device: d, button, kind } = rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            assert_eq!(d, device);
            got.push((button, kind));
        }
    }
    assert_eq!(got, [("t-1".to_string(), Kind::Press), ("t-2".to_string(), Kind::Turn(3))]);
    hub.tell(&device, &json!({ "t": "result", "ok": false, "message": "Spotify isn't running" }));
    assert_eq!(want(&mut phone, "result")["message"], "Spotify isn't running");
    // its pictures now open with its key
    assert_eq!(get(hub.tls_port, &format!("/icon?id=play&c=ff0000&t={tok}")).0, "HTTP/1.1 200 OK");
    drop(phone);

    // a new connection with the key comes straight in; a wrong key doesn't
    let mut again = ws(hub.tls_port);
    say(&mut again, json!({ "t": "hello", "token": tok }));
    assert_eq!(want(&mut again, "hello")["name"], "Pixel 8");
    let mut stranger = ws(hub.tls_port);
    say(&mut stranger, json!({ "t": "hello", "token": "not-a-key" }));
    want(&mut stranger, "unpaired");
    // a press from a phone that isn't in does nothing
    say(&mut stranger, json!({ "t": "press", "button": "t-9" }));
    // Forget takes the phone away: it is told, and its key stops working
    hub.forget(&device);
    want(&mut again, "forgotten");
    assert!(hub.phones().is_empty());
    assert_eq!(get(hub.tls_port, &format!("/icon?id=play&c=ff0000&t={tok}")).0, "HTTP/1.1 403 Forbidden");
    while let Ok(e) = rx.try_recv() {
        assert!(!matches!(e, PhoneEvent::Press { ref button, .. } if button == "t-9"), "a phone that isn't paired can't press");
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn too_many_wrong_codes_lock_pairing_for_a_while_and_denied_phones_are_told() {
    let (hub, _rx, root) = server("lock");
    let mut phone = ws(hub.tls_port);
    for _ in 0..MAX_WRONG {
        say(&mut phone, json!({ "t": "pair", "code": "123456", "name": "x" }));
        want(&mut phone, "refused");
    }
    // even the right code is refused now
    say(&mut phone, json!({ "t": "pair", "code": hub.code(), "name": "x" }));
    assert!(want(&mut phone, "refused")["message"].as_str().unwrap().contains("Wait"));
    let _ = fs::remove_dir_all(&root);

    // a denied request is told so
    let (hub, _rx, root) = server("deny");
    let mut phone = ws(hub.tls_port);
    say(&mut phone, json!({ "t": "pair", "code": hub.code(), "name": "Tablet", "tablet": true }));
    want(&mut phone, "waiting");
    hub.deny(hub.pending()[0].conn);
    want(&mut phone, "denied");
    assert!(hub.phones().is_empty());
    // with "allow" switched off the code alone is enough
    hub.require_match.store(false, Ordering::Relaxed);
    say(&mut phone, json!({ "t": "pair", "code": hub.code(), "name": "Tablet", "tablet": true }));
    let p = want(&mut phone, "paired");
    assert!(p["token"].as_str().is_some());
    assert!(hub.phones()[0].tablet);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_phone_that_stops_listening_is_dropped_and_devices_are_remembered() {
    let (hub, _rx, root) = server("keep");
    let mut phone = ws(hub.tls_port);
    say(&mut phone, json!({ "t": "pair", "code": hub.code(), "name": "Remembered" }));
    want(&mut phone, "waiting");
    hub.approve(hub.pending()[0].conn);
    let tok = want(&mut phone, "paired")["token"].as_str().unwrap().to_string();
    drop(phone);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(hub.connected(), 0, "a closed connection is gone");
    // the list is kept on disk: another start knows the phone
    let raw = fs::read(root.join("phones").join("devices.mdsave")).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("Remembered"), "the list is sealed on disk");
    let saved = String::from_utf8(vault::read(&root.join("phones").join("devices.mdsave"), SealKind::Save).unwrap()).unwrap();
    assert!(saved.contains("Remembered") && !saved.contains(&tok), "only a hash of the key is kept");
    let _ = fs::remove_dir_all(&root);
}

/// A server with a sample deck for trying the phone page in a browser by hand:
///     cargo test --release phone_page_demo -- --ignored --nocapture
/// It prints the address and lets any phone in with the code (no Allow step), for 3 minutes or
/// until a button is pressed.
#[test]
#[ignore]
fn phone_page_demo() {
    let (hub, rx, root) = server("demo");
    hub.require_match.store(false, Ordering::Relaxed);
    let deck = json!({
        "profile": "Demo", "page": "p1", "follow": true,
        "settings": { "names": true, "vibe": true, "awake": true, "dark": true },
        "theme": { "ground": "#1e2433", "ink": "#e4eaf6", "text": "#e4eaf6", "panel": "#2d3142", "tile": "#2d3142", "shadow": "#10131c", "slate": "#9aa6c2", "mint": "#5fd0a8", "coral": "#ff5a5f" },
        "pages": [
            { "id": "p1", "name": "Home", "cols": 5, "rows": 3, "tiles": [
                { "id": "a", "x": 0, "y": 0, "w": 1, "h": 1, "label": "Play", "icon": "play", "colour": "#5fd0a8", "ink": "#1e2433", "run": true },
                { "id": "b", "x": 1, "y": 0, "w": 2, "h": 1, "label": "Mic", "icon": "mic", "colour": "#7cc4ff", "ink": "#1e2433", "run": true },
                { "id": "c", "x": 0, "y": 1, "w": 1, "h": 1, "label": "Second", "icon": "folder", "colour": "#2d3142", "ink": "#e4eaf6", "nav": { "page": "p2" }, "run": false },
                { "id": "k", "x": 3, "y": 0, "w": 1, "h": 1, "label": "Volume", "icon": "speaker", "colour": "#ffc93c", "ink": "#1e2433", "run": true, "knob": true }
            ] },
            { "id": "p2", "name": "Second", "cols": 5, "rows": 3, "tiles": [
                { "id": "d", "x": 0, "y": 0, "w": 1, "h": 1, "label": "Back", "icon": "up", "colour": "#ff8fab", "ink": "#1e2433", "nav": { "back": true }, "run": false }
            ] }
        ]
    });
    hub.set_snapshot(deck.to_string());
    println!("DEMO http://localhost:{}/  https://localhost:{}/  code {}", hub.port, hub.tls_port, hub.code());
    let t0 = std::time::Instant::now();
    while t0.elapsed() < Duration::from_secs(180) {
        if let Ok(PhoneEvent::Press { button, kind, .. }) = rx.recv_timeout(Duration::from_millis(200)) {
            println!("DEMO press {button} {kind:?}");
            if kind == Kind::Press {
                break;
            }
        }
        hub.set_snapshot(deck.to_string());
    }
    let _ = fs::remove_dir_all(&root);
}

