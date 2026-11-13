//! The phone server, tried with a real TLS and WebSocket client on this PC.
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpStream};
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

/// A request over https: (the answer's headers, its body)
fn ask(port: u16, method: &str, path: &str) -> (String, Vec<u8>) {
    let mut s = tls(port);
    write!(s, "{method} {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut all = Vec::new();
    let _ = s.read_to_end(&mut all);
    let split = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(all.len());
    (String::from_utf8_lossy(&all[..split]).into_owned(), all.get(split + 4..).unwrap_or(&[]).to_vec())
}

/// GET over https: (status line, body)
fn get(port: u16, path: &str) -> (String, Vec<u8>) {
    let (head, body) = ask(port, "GET", path);
    (head.lines().next().unwrap_or("").to_string(), body)
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
    // each test server gets its own stretch of ports, so servers started at once don't crowd each other
    static NEXT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);
    let slot = NEXT.fetch_add(1, Ordering::Relaxed) % 40;
    let base = 41000 + (std::process::id() % 50) as u16 * 400 + slot * 10;
    let hub = start(&root, base, tx, || {}).expect("the phone server starts");
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
    assert_eq!(get(hub.tls_port, "/media?id=x&k=nope").0, "HTTP/1.1 403 Forbidden");
    assert_eq!(get(hub.tls_port, "/nothing-here").0, "HTTP/1.1 404 Not Found");
    // HEAD: the same headers, and no body
    let (head, body) = ask(hub.tls_port, "HEAD", "/");
    assert!(head.starts_with("HTTP/1.1 200 OK") && body.is_empty(), "{head} / {} bytes", body.len());
    assert!(head.contains(&format!("Content-Length: {}", super::server::index_len_for_tests())), "{head}");
    // the page can't be framed, sends no referrer, and runs only its own script
    let lower = head.to_ascii_lowercase();
    for want in ["x-frame-options: deny", "referrer-policy: no-referrer", "x-content-type-options: nosniff", "content-security-policy: default-src 'none'", "frame-ancestors 'none'", "script-src 'sha256-"] {
        assert!(lower.contains(want), "no {want} in {head}");
    }
    assert!(!lower.contains("unsafe-inline") && !lower.contains("unsafe-eval"), "{head}");
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
    // one address fills its share with connections that say nothing (half on each port)
    let held: Vec<TcpStream> = (0..super::server::MAX_PER_ADDRESS)
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
fn places_are_counted_for_each_address_and_all_together() {
    use super::server::{MAX_CONNECTIONS, MAX_PER_ADDRESS, Places};
    let places = Arc::new(Places::default());
    let ip = |n: u8| IpAddr::V4(Ipv4Addr::new(192, 168, 1, n));
    // one address can't take more than its share, and others still get in
    let mine: Vec<_> = (0..MAX_PER_ADDRESS).map(|_| places.take(ip(1)).expect("a place")).collect();
    assert!(places.take(ip(1)).is_none());
    let other = places.take(ip(2)).expect("another address still gets in");
    // all together there is a limit too
    let mut rest = vec![];
    let mut n = 3u8;
    while mine.len() + 1 + rest.len() < MAX_CONNECTIONS {
        rest.push(places.take(ip(n)).expect("a place"));
        n += 1;
    }
    assert!(places.take(ip(250)).is_none(), "full");
    // places come back when connections end
    drop(other);
    drop(mine);
    assert!(places.take(ip(250)).is_some() && places.take(ip(1)).is_some());
}

/// A connection straight to the hub from `ip`, as the server registers one.
fn conn_from(hub: &Hub, ip: [u8; 4]) -> (u64, Receiver<Out>) {
    let (tx, rx) = channel();
    (hub.register(tx, IpAddr::V4(Ipv4Addr::from(ip))), rx)
}

#[test]
fn wrong_codes_lock_out_only_the_address_they_came_from() {
    let (hub, _rx, root) = server("peer-lock");
    let (bad, _b) = conn_from(&hub, [192, 168, 1, 66]);
    let (good, _g) = conn_from(&hub, [192, 168, 1, 20]);
    let refused = |r: PairResult| match r {
        PairResult::Refused(m) => m,
        _ => panic!("it got in"),
    };
    for _ in 0..MAX_WRONG {
        refused(hub.pair(bad, "000000x", "x", "", false));
    }
    // that address is locked out, even with the right code...
    assert!(refused(hub.pair(bad, &hub.code(), "x", "", false)).contains("Wait"));
    // ...and a phone at another address pairs as usual
    assert!(matches!(hub.pair(good, &hub.code(), "Mine", "", false), PairResult::Waiting(_)));
    // guesses from many addresses never add up against one code: it is replaced
    let first = hub.code();
    for i in 0..MAX_WRONG_TOTAL {
        let (c, _r) = conn_from(&hub, [10, 0, (i / 200) as u8, (i % 200) as u8 + 1]);
        refused(hub.pair(c, "000000x", "x", "", false));
    }
    assert_ne!(hub.code(), first, "a new code after many wrong ones");
    // only so many phones wait for Allow at once
    let mut waiting = vec![];
    for i in 0..(MAX_PENDING as u8 + 2) {
        let (c, r) = conn_from(&hub, [172, 16, 0, i + 1]);
        waiting.push((matches!(hub.pair(c, &hub.code(), "p", "", false), PairResult::Waiting(_)), r));
    }
    assert_eq!(waiting.iter().filter(|(w, _)| *w).count(), MAX_PENDING - 1, "the list is full (one was waiting already)");
    assert_eq!(hub.pending().len(), MAX_PENDING);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_connection_that_is_not_a_phone_is_dropped_and_big_messages_are_refused() {
    let (hub, _rx, root) = server("idle");
    // pings alone don't keep a connection that never shows a key or asks to pair
    let mut idle = ws(hub.tls_port);
    let t0 = std::time::Instant::now();
    let closed = loop {
        if t0.elapsed() > super::server::AUTH_WITHIN + Duration::from_secs(8) {
            break false;
        }
        if idle.send(Message::text(json!({ "t": "ping" }).to_string())).is_err() {
            break true;
        }
        match idle.read() {
            Ok(Message::Close(_)) | Err(_) => break true,
            Ok(_) => std::thread::sleep(Duration::from_millis(300)),
        }
    };
    assert!(closed, "an unpaired connection stayed open");
    assert!(t0.elapsed() >= super::server::AUTH_WITHIN - Duration::from_secs(1), "it was closed too soon");
    // a message bigger than a phone ever sends closes the connection (and isn't read)
    let mut big = ws(hub.tls_port);
    let _ = big.send(Message::text(json!({ "t": "pair", "code": "1".repeat(super::server::MAX_MESSAGE * 4) }).to_string()));
    let gone = (0..20).any(|_| matches!(big.read(), Ok(Message::Close(_)) | Err(_)));
    assert!(gone, "a huge message was taken");
    // a request fed a byte at a time doesn't hold a place for long
    let mut slow = TcpStream::connect(("127.0.0.1", hub.port)).unwrap();
    slow.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    let t0 = std::time::Instant::now();
    let mut dropped = false;
    for b in b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".iter().cycle() {
        if t0.elapsed() > Duration::from_secs(25) {
            break;
        }
        if slow.write_all(&[*b]).is_err() {
            dropped = true;
            break;
        }
        let mut sink = [0u8; 8];
        if matches!(slow.read(&mut sink), Ok(0)) {
            dropped = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    assert!(dropped && t0.elapsed() < Duration::from_secs(20), "a slow request was waited for: {:?}", t0.elapsed());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn the_qr_opens_the_plain_page_with_its_key_or_the_secure_one() {
    let (hub, _rx, root) = server("links");
    // by default: the plain page, with the code and the link key after the # (never sent)
    let url = hub.url();
    let (page, frag) = url.split_once('#').unwrap();
    assert_eq!(page, hub.base_url());
    let (code, key) = frag.split_once('.').expect("code.key");
    assert_eq!(code, hub.code());
    assert_eq!(super::link::decode(key).unwrap().len(), 16);
    // Secure phone link: the start page, which goes on to https (no key needed there)
    hub.secure_link.store(true, Ordering::Relaxed);
    assert_eq!(hub.url(), format!("{}#{}", hub.start_url(), hub.code()));
    hub.secure_link.store(false, Ordering::Relaxed);
    // the plain door serves the page, the start page and the certificate; the secure page answers the start page's question
    let (status, body) = http_get(hub.port, "/");
    assert!(status.ends_with("200 OK") && String::from_utf8_lossy(&body).contains("Pair with your PC"));
    assert!(String::from_utf8_lossy(&http_get(hub.port, "/start").1).contains("One-time setup"));
    assert!(http_get(hub.port, "/minniondeck.cer").1.starts_with(&[0x30]));
    let info: Value = serde_json::from_slice(&http_get(hub.port, "/cert-info").1).unwrap();
    assert_eq!(info["tls"], hub.tls_port);
    let (head, _) = ask(hub.tls_port, "GET", "/cert-info");
    assert!(head.contains("Access-Control-Allow-Origin: *"), "{head}");
    // a page of another website can't open the phone's WebSocket
    let mut s = tls(hub.tls_port);
    write!(s, "GET /ws HTTP/1.1\r\nHost: localhost:{}\r\nOrigin: https://evil.example\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n", hub.tls_port).unwrap();
    let mut all = Vec::new();
    let _ = s.read_to_end(&mut all);
    assert!(!String::from_utf8_lossy(&all).contains("101"), "another site's page got a WebSocket");
    let _ = fs::remove_dir_all(&root);
}

/// A phone on the plain page, as its JavaScript does it: the PC's greeting, then the keyed link.
struct Plain {
    ws: WebSocket<TcpStream>,
    seal: Option<super::link::Seal>,
    /// every frame that came in, as it crossed the Wi-Fi
    seen: Vec<String>,
}

impl Plain {
    fn open(port: u16) -> Plain {
        let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let (ws, _) = tungstenite::client(format!("ws://localhost:{port}/ws"), tcp).unwrap();
        Plain { ws, seal: None, seen: vec![] }
    }

    /// Keys the link with `secret`, named `kid`.
    fn key(&mut self, kid: &str, secret: &[u8]) {
        let hi = self.raw();
        assert_eq!(hi["t"], "hi");
        let pc = super::link::decode(hi["n"].as_str().unwrap()).unwrap();
        let mine = [7u8; 16];
        self.ws.send(Message::text(json!({ "t": "key", "kid": kid, "n": super::link::encode(&mine) }).to_string())).unwrap();
        self.seal = Some(super::link::Seal::new(secret, &pc, &mine));
    }

    fn frame(&mut self) -> String {
        loop {
            if let Message::Text(t) = self.ws.read().unwrap() {
                self.seen.push(t.to_string());
                return t.to_string();
            }
        }
    }

    /// A message in the open (before the keys)
    fn raw(&mut self) -> Value {
        serde_json::from_str(&self.frame()).unwrap()
    }

    fn say(&mut self, v: Value) -> String {
        let f = self.seal.as_mut().unwrap().phone_seal(v);
        self.ws.send(Message::text(f.clone())).unwrap();
        f
    }

    /// The next sealed message of this type (others, like the deck, may come first).
    fn want(&mut self, t: &str) -> Value {
        for _ in 0..40 {
            let f = self.frame();
            let v = self.seal.as_mut().unwrap().phone_open(&f).unwrap_or_else(|| panic!("a frame that doesn't open: {f}"));
            if v["t"] == t {
                return v;
            }
        }
        panic!("no {t}");
    }
}

// the plain page's link: nothing readable crosses the Wi-Fi, and a recorded press can't be sent again
#[test]
fn the_plain_pages_link_is_encrypted_from_pairing_to_presses() {
    let (hub, rx, root) = server("sealed");
    let url = hub.url();
    let key = super::link::decode(url.split_once('#').unwrap().1.split_once('.').unwrap().1).unwrap();
    // pairing with the QR's link key
    let mut phone = Plain::open(hub.port);
    phone.key("pair", &key);
    phone.say(json!({ "t": "pair", "code": hub.code(), "name": "Plain phone" }));
    let number = phone.want("waiting")["number"].as_str().unwrap().to_string();
    assert_eq!(hub.pending()[0].number, number);
    hub.approve(hub.pending()[0].conn);
    let paired = phone.want("paired");
    let tok = paired["token"].as_str().unwrap().to_string();
    hub.set_snapshot(r#"{"profile":"Secret deck","pages":[]}"#.to_string());
    assert_eq!(phone.want("snap")["deck"]["profile"], "Secret deck");
    let press = phone.say(json!({ "t": "press", "button": "p-1" }));
    let got = loop {
        if let PhoneEvent::Press { button, .. } = rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            break button;
        }
    };
    assert_eq!(got, "p-1");
    // what crossed the Wi-Fi: no token, no deck, no number
    let wire = phone.seen.join("\n");
    assert!(!wire.contains(&tok) && !wire.contains("Secret deck") && !wire.contains("paired"), "{wire}");
    // the same press frame sent again: refused, and the connection closes
    phone.ws.send(Message::text(press)).unwrap();
    let mut refused = false;
    for _ in 0..10 {
        match phone.ws.read() {
            Ok(Message::Text(t)) if t.contains("refused") => refused = true,
            Ok(Message::Close(_)) | Err(_) => break,
            _ => {}
        }
    }
    assert!(refused, "a replayed press was taken");
    while let Ok(e) = rx.try_recv() {
        assert!(!matches!(e, PhoneEvent::Press { .. }), "the replay pressed again");
    }
    drop(phone);

    // the phone comes back with its own secret (SHA-256 of its token): the token isn't sent
    let secret = super::link::sha256(tok.as_bytes());
    let mut again = Plain::open(hub.port);
    again.key(&super::link::kid(&secret), &secret);
    again.say(json!({ "t": "hello" }));
    assert_eq!(again.want("hello")["name"], "Plain phone");
    assert!(!again.seen.join("\n").contains(&tok));
    drop(again);

    // a forgotten phone, or one that isn't this PC's, is told in the open and must pair again
    let mut stranger = Plain::open(hub.port);
    let _ = stranger.raw();
    stranger.ws.send(Message::text(json!({ "t": "key", "kid": "0011223344556677", "n": super::link::encode(&[1u8; 16]) }).to_string())).unwrap();
    assert_eq!(stranger.raw()["t"], "unpaired");

    // a typed code keys the link with the code; a wrong one counts as a wrong code
    let mut typed = Plain::open(hub.port);
    typed.key("code", &super::link::code_secret("000000"));
    typed.say(json!({ "t": "pair", "code": "000000", "name": "x" }));
    let r = typed.raw();
    assert_eq!(r["t"], "refused");
    let mut right = Plain::open(hub.port);
    right.key("code", &super::link::code_secret(&hub.code()));
    right.say(json!({ "t": "pair", "code": hub.code(), "name": "Typed" }));
    right.want("waiting");
    // an old QR's link key is refused (the code was renewed when the first phone paired)
    let mut old = Plain::open(hub.port);
    old.key("pair", &key);
    old.say(json!({ "t": "pair", "code": "123456", "name": "x" }));
    assert!(old.raw()["message"].as_str().unwrap().contains("old"));
    // the plain door never takes the unencrypted messages the https page uses
    let mut clear = Plain::open(hub.port);
    let _ = clear.raw();
    clear.ws.send(Message::text(json!({ "t": "hello", "token": tok }).to_string())).unwrap();
    assert!(matches!(clear.ws.read(), Ok(Message::Close(_)) | Err(_)) || matches!(clear.ws.read(), Ok(Message::Close(_)) | Err(_)));
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
    let ticket = paired["ticket"].as_str().unwrap().to_string();
    assert!(ticket.len() >= 20 && ticket != tok);
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
    // its pictures open with the ticket it was handed, never with its key in the address
    assert_eq!(get(hub.tls_port, &format!("/icon?id=play&c=ff0000&k={ticket}")).0, "HTTP/1.1 200 OK");
    assert_eq!(get(hub.tls_port, &format!("/icon?id=play&c=ff0000&t={tok}")).0, "HTTP/1.1 403 Forbidden");
    assert_eq!(get(hub.tls_port, &format!("/icon?id=play&c=ff0000&k={tok}")).0, "HTTP/1.1 403 Forbidden");
    drop(phone);

    // a new connection with the key comes straight in (and is handed its ticket); a wrong key doesn't
    let mut again = ws(hub.tls_port);
    say(&mut again, json!({ "t": "hello", "token": tok }));
    let hello = want(&mut again, "hello");
    assert_eq!(hello["name"], "Pixel 8");
    assert_eq!(hello["ticket"], ticket.as_str(), "the same ticket while it is young, so pictures stay cached");
    let mut stranger = ws(hub.tls_port);
    say(&mut stranger, json!({ "t": "hello", "token": "not-a-key" }));
    want(&mut stranger, "unpaired");
    // a press from a phone that isn't in does nothing
    say(&mut stranger, json!({ "t": "press", "button": "t-9" }));
    // Forget takes the phone away: it is told, and its key stops working
    hub.forget(&device);
    want(&mut again, "forgotten");
    assert!(hub.phones().is_empty());
    assert_eq!(get(hub.tls_port, &format!("/icon?id=play&c=ff0000&k={ticket}")).0, "HTTP/1.1 403 Forbidden", "a forgotten phone's ticket is gone");
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
    println!("DEMO http://localhost:{}/#{}  https://localhost:{}/  code {}", hub.port, hub.url().split_once('#').unwrap().1, hub.tls_port, hub.code());
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

