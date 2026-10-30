// The phone server: always listening on the local network while Minniondeck runs.
// Each connection is TLS, then WebSocket, then the pairing or hello handshake from protocol.rs.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc};
use tokio::time::{Instant, interval, timeout};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

use super::hub::{Cmd, Hub, Refusal};
use super::protocol::{Incoming, Outgoing, VERSION};

const HANDSHAKE: Duration = Duration::from_secs(10);
const FIRST_MESSAGE: Duration = Duration::from_secs(15);
const PING_EVERY: Duration = Duration::from_secs(20);
/// A phone that sends nothing (not even a pong) for this long is treated as gone
const SILENT_LIMIT: Duration = Duration::from_secs(60);
const MAX_MESSAGE: usize = 1024 * 1024;
/// Pictures and GIF files go to the phone in parts this big (before base64)
const MEDIA_PART: usize = 512 * 1024;
/// Connections still in their handshake at once; paired phones don't count
const MAX_PENDING: usize = 32;

type Ws = WebSocketStream<tokio_rustls::server::TlsStream<TcpStream>>;

/// Listen on every network the PC is on. If the port is taken, the Phones page says so.
pub async fn start(hub: Arc<Hub>, port: u16, tls: Arc<rustls::ServerConfig>) {
    match TcpListener::bind(("0.0.0.0", port)).await {
        Ok(listener) => {
            hub.set_status(true, port, None);
            serve(hub, listener, TlsAcceptor::from(tls)).await;
        }
        Err(e) => hub.set_status(false, port, Some(format!("Phones can't connect: port {port} is in use ({e}). Choose another port in Settings → Phones, then restart Minniondeck."))),
    }
}

pub async fn serve(hub: Arc<Hub>, listener: TcpListener, acceptor: TlsAcceptor) {
    let pending = Arc::new(Semaphore::new(MAX_PENDING));
    loop {
        match listener.accept().await {
            Ok((tcp, addr)) => {
                let Ok(permit) = pending.clone().try_acquire_owned() else { continue };
                let (hub, acceptor) = (hub.clone(), acceptor.clone());
                tokio::spawn(async move {
                    if let Err(e) = handle(hub, tcp, addr, acceptor, permit).await {
                        eprintln!("[phones] {addr}: {e}");
                    }
                });
            }
            Err(e) => {
                eprintln!("[phones] accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

/// A phone id from the network, short and plain enough for the log
fn clip_id(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(40).collect()
}

async fn send(ws: &mut Ws, out: &Outgoing) -> Result<(), String> {
    ws.send(Message::Text(out.text().into())).await.map_err(|e| e.to_string())
}

async fn refuse(mut ws: Ws, (code, message): Refusal) -> Result<(), String> {
    let _ = send(&mut ws, &Outgoing::error(code, message)).await;
    let _ = ws.close(None).await;
    Err(format!("refused: {code}"))
}

async fn handle(hub: Arc<Hub>, tcp: TcpStream, addr: SocketAddr, acceptor: TlsAcceptor, permit: tokio::sync::OwnedSemaphorePermit) -> Result<(), String> {
    let _ = tcp.set_nodelay(true);
    let tls = timeout(HANDSHAKE, acceptor.accept(tcp)).await.map_err(|_| "TLS timed out")?.map_err(|e| format!("TLS: {e}"))?;
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_MESSAGE);
    config.max_frame_size = Some(MAX_MESSAGE);
    let mut ws = timeout(HANDSHAKE, tokio_tungstenite::accept_async_with_config(tls, Some(config)))
        .await
        .map_err(|_| "WebSocket timed out")?
        .map_err(|e| format!("WebSocket: {e}"))?;
    let first = match timeout(FIRST_MESSAGE, ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => t,
        _ => return refuse(ws, ("bad_request", "The first message must be pair or hello.")).await,
    };
    let (phone_id, reply, has_rev, kept) = match serde_json::from_str::<Incoming>(first.as_str()) {
        Ok(Incoming::Pair { v, .. } | Incoming::Hello { v, .. }) if v != VERSION => {
            return refuse(ws, ("unsupported_version", "Update Minniondeck on the phone or the PC so both use the same version.")).await;
        }
        Ok(Incoming::Pair { code, device, .. }) => match hub.try_pair(&code, device) {
            Ok((record, token)) => (record.id.clone(), Outgoing::Paired { phone_id: record.id, token, pc_name: hub.pc_name.clone() }, None, (Vec::new(), false)),
            Err(r) => {
                crate::applog::line(&format!("Phone pairing refused ({}) from {}", r.0, addr.ip()));
                return refuse(ws, r).await;
            }
        },
        Ok(Incoming::Hello { phone_id, token, rev, has, gz, .. }) => match hub.check_hello(&phone_id, &token) {
            Ok(record) => (record.id.clone(), Outgoing::Welcome { phone_id: record.id, pc_name: hub.pc_name.clone() }, rev, (has, gz)),
            Err(r) => {
                // A phone turned away here goes unpaired-looking on the phone, so say why
                crate::applog::line(&format!("Phone refused ({}): {} from {}", r.0, clip_id(&phone_id), addr.ip()));
                return refuse(ws, r).await;
            }
        },
        _ => return refuse(ws, ("bad_request", "The first message must be pair or hello.")).await,
    };
    send(&mut ws, &reply).await?;
    drop(permit);

    let (tx, mut rx) = mpsc::unbounded_channel();
    // Connecting sends the phone its layout (unless it already has this one)
    let conn = hub.connect(&phone_id, addr.ip().to_string(), has_rev, kept.0, kept.1, tx);
    let result = connected(&hub, &phone_id, conn, &mut ws, &mut rx).await;
    if let Err(e) = &result {
        crate::applog::line(&format!("Phone link dropped: {phone_id}: {e}"));
    }
    hub.disconnect(&phone_id, conn);
    result
}

/// A paired phone stays here until it leaves, goes quiet, or is removed on the PC
async fn connected(hub: &Hub, phone_id: &str, conn: u64, ws: &mut Ws, rx: &mut mpsc::UnboundedReceiver<Cmd>) -> Result<(), String> {
    let mut ping = interval(PING_EVERY);
    ping.tick().await;
    let mut heard = Instant::now();
    loop {
        tokio::select! {
            msg = ws.next() => match msg {
                Some(Ok(Message::Text(t))) => {
                    heard = Instant::now();
                    match serde_json::from_str::<Incoming>(t.as_str()) {
                        Ok(Incoming::Status { battery }) => hub.set_battery(phone_id, conn, battery),
                        Ok(Incoming::Viewing { page }) => hub.viewing(phone_id, conn, page),
                        Ok(Incoming::Press { button_id, kind }) => {
                            if let Err((_, message)) = hub.press(phone_id, &button_id, kind.as_deref().unwrap_or("press")) {
                                send(ws, &Outgoing::Ran { button_id, ok: false, message: message.into() }).await?;
                            }
                        }
                        Ok(Incoming::GetMedia { file }) => send_media(hub, phone_id, ws, file).await?,
                        Ok(Incoming::View { button_id, data }) => hub.view(phone_id, &button_id, data),
                        Ok(Incoming::Turn { button_id, by }) => hub.turn(phone_id, &button_id, by),
                        Ok(Incoming::NeedLayout { rev }) => hub.need_layout(phone_id, conn, &rev),
                        _ => {}
                    }
                }
                Some(Ok(Message::Close(_))) | None => return Ok(()),
                Some(Ok(_)) => heard = Instant::now(),
                Some(Err(e)) => return Err(e.to_string()),
            },
            cmd = rx.recv() => match cmd {
                Some(Cmd::Send(text)) => {
                    let (big, started) = (text.len() > 256 * 1024, Instant::now());
                    let kb = text.len() / 1024;
                    ws.send(Message::Text(text.into())).await.map_err(|e| e.to_string())?;
                    if big {
                        crate::applog::line(&format!("Phone sent {kb} KB in {} ms", started.elapsed().as_millis()));
                    }
                }
                Some(Cmd::Binary(bytes)) => {
                    let (kb, started) = (bytes.len() / 1024, Instant::now());
                    ws.send(Message::Binary(bytes.into())).await.map_err(|e| e.to_string())?;
                    crate::applog::line(&format!("Phone sent {kb} KB in {} ms", started.elapsed().as_millis()));
                }
                Some(Cmd::Close(why)) => {
                    let _ = send(ws, &why).await;
                    let _ = ws.close(None).await;
                    return Ok(());
                }
                None => return Ok(()),
            },
            _ = ping.tick() => {
                if heard.elapsed() > SILENT_LIMIT {
                    return Err("the phone stopped answering".into());
                }
                ws.send(Message::Ping(Vec::new().into())).await.map_err(|e| e.to_string())?;
            }
        }
    }
}

/// Send a picture or GIF file the phone's layout uses, in parts
async fn send_media(hub: &Hub, phone_id: &str, ws: &mut Ws, file: String) -> Result<(), String> {
    use base64::Engine;
    let bytes = match hub.media_path(phone_id, &file) {
        Some(path) => tokio::task::spawn_blocking(move || std::fs::read(path)).await.ok().and_then(|r| r.ok()),
        None => None,
    };
    let Some(bytes) = bytes else { return send(ws, &Outgoing::MediaMissing { file }).await };
    let parts: Vec<&[u8]> = if bytes.is_empty() { vec![&[]] } else { bytes.chunks(MEDIA_PART).collect() };
    let total = parts.len();
    for (index, part) in parts.into_iter().enumerate() {
        let data = base64::engine::general_purpose::STANDARD.encode(part);
        send(ws, &Outgoing::Media { file: file.clone(), index, total, data }).await?;
    }
    Ok(())
}

// The tests play the phone: a real TLS client that pins the PC's fingerprint, over real sockets
#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::CryptoProvider;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{DigitallySignedStruct, SignatureScheme};
    use serde_json::{Value, json};
    use tokio_rustls::TlsConnector;

    use super::super::hub::{Event, PairingEvent, PhonePatch, PressEvent};
    use super::super::identity::Identity;
    use super::*;
    use crate::storage::sha256_hex;

    #[derive(Debug)]
    struct Pinned {
        pin: String,
        provider: Arc<CryptoProvider>,
    }

    impl ServerCertVerifier for Pinned {
        fn verify_server_cert(&self, cert: &CertificateDer<'_>, _: &[CertificateDer<'_>], _: &ServerName<'_>, _: &[u8], _: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
            if sha256_hex(cert.as_ref()) == self.pin { Ok(ServerCertVerified::assertion()) } else { Err(rustls::Error::General("fingerprint does not match".into())) }
        }
        fn verify_tls12_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(m, c, d, &self.provider.signature_verification_algorithms)
        }
        fn verify_tls13_signature(&self, m: &[u8], c: &CertificateDer<'_>, d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(m, c, d, &self.provider.signature_verification_algorithms)
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.provider.signature_verification_algorithms.supported_schemes()
        }
    }

    type PhoneWs = WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>;

    struct Rig {
        hub: Arc<Hub>,
        addr: SocketAddr,
        pin: String,
        events: Arc<Mutex<Vec<PairingEvent>>>,
        presses: Arc<Mutex<Vec<PressEvent>>>,
        dir: std::path::PathBuf,
    }

    async fn rig() -> Rig {
        let dir = std::env::temp_dir().join(format!("minniondeck-test-phones-{}-{}", crate::storage::now_ms(), rand_tag()));
        std::fs::create_dir_all(&dir).unwrap();
        let id = Identity::load_or_create(&dir.join("identity")).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let presses = Arc::new(Mutex::new(Vec::new()));
        let (seen, pressed) = (events.clone(), presses.clone());
        std::fs::create_dir_all(dir.join("media")).unwrap();
        let hub = Arc::new(Hub::load(dir.join("devices.json"), dir.join("media"), id.fingerprint.clone(), "TEST-PC".into(), Box::new(move |e| match e {
            Event::Pairing(p) => seen.lock().unwrap().push(p),
            Event::Press(p) => pressed.lock().unwrap().push(p),
            Event::Phones(_) | Event::View(_) | Event::Turn(_) | Event::Pages(_) | Event::PhonePage(_) => {}
        })));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        hub.set_status(true, addr.port(), None);
        tokio::spawn(serve(hub.clone(), listener, TlsAcceptor::from(id.server_config().unwrap())));
        Rig { hub, addr, pin: id.fingerprint, events, presses, dir }
    }

    fn rand_tag() -> String {
        super::super::pairing::new_id()
    }

    async fn phone(addr: SocketAddr, pin: &str) -> Result<PhoneWs, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pinned { pin: pin.into(), provider }))
            .with_no_client_auth();
        let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
        let tls = TlsConnector::from(Arc::new(config)).connect(ServerName::try_from("minniondeck").unwrap(), tcp).await.map_err(|e| e.to_string())?;
        let (ws, _) = tokio_tungstenite::client_async("wss://minniondeck/", tls).await.map_err(|e| e.to_string())?;
        Ok(ws)
    }

    async fn say(ws: &mut PhoneWs, msg: Value) -> Value {
        ws.send(Message::Text(msg.to_string().into())).await.unwrap();
        next(ws).await
    }

    async fn tell(ws: &mut PhoneWs, msg: Value) {
        ws.send(Message::Text(msg.to_string().into())).await.unwrap();
    }

    /// The next message from the PC
    async fn next(ws: &mut PhoneWs) -> Value {
        loop {
            match timeout(Duration::from_secs(5), ws.next()).await.expect("the PC did not answer") {
                Some(Ok(Message::Text(t))) => return serde_json::from_str(t.as_str()).unwrap(),
                Some(Ok(_)) => continue,
                other => panic!("connection ended: {other:?}"),
            }
        }
    }

    fn pair_msg(code: &str, device_id: &str) -> Value {
        json!({ "type": "pair", "v": 1, "code": code, "device": { "deviceId": device_id, "name": "Pixel 8", "kind": "phone", "os": "Android 15", "appVersion": "1.0.0", "aspect": 2.22 } })
    }

    async fn wait_until(what: impl Fn() -> bool) {
        for _ in 0..100 {
            if what() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out waiting");
    }

    #[tokio::test]
    async fn a_phone_pairs_with_the_code_then_comes_back_with_its_token() {
        let r = rig().await;
        let code = r.hub.start_pairing().unwrap().code;

        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        let wrong = if code == "000000" { "111111" } else { "000000" };
        assert_eq!(say(&mut ws, pair_msg(wrong, "dev-1")).await["code"], "bad_code");

        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        let paired = say(&mut ws, pair_msg(&format!("{} {}", &code[..3], &code[3..]), "dev-1")).await;
        assert_eq!(paired["type"], "paired");
        assert_eq!(paired["pcName"], "TEST-PC");
        let (id, token) = (paired["phoneId"].as_str().unwrap().to_string(), paired["token"].as_str().unwrap().to_string());
        wait_until(|| r.hub.list().iter().any(|p| p.id == id && p.online)).await;
        assert_eq!(r.events.lock().unwrap().last().unwrap().reason, "paired");

        // Saved, but only the hash of the token
        let saved = std::fs::read_to_string(r.dir.join("devices.json")).unwrap();
        assert!(saved.contains(&id) && saved.contains(&sha256_hex(token.as_bytes())) && !saved.contains(&token));

        // The code worked once
        let mut other = phone(r.addr, &r.pin).await.unwrap();
        assert_eq!(say(&mut other, pair_msg(&code, "dev-2")).await["code"], "no_pairing");

        // Battery from the phone shows on the PC; leaving marks it offline
        ws.send(Message::Text(json!({ "type": "status", "battery": 76 }).to_string().into())).await.unwrap();
        wait_until(|| r.hub.list()[0].battery == Some(76)).await;
        drop(ws);
        wait_until(|| !r.hub.list()[0].online).await;

        // Back again with its token
        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        let hello = say(&mut ws, json!({ "type": "hello", "v": 1, "phoneId": id, "token": token })).await;
        assert_eq!(hello["type"], "welcome");
        wait_until(|| r.hub.list()[0].online).await;

        let mut bad = phone(r.addr, &r.pin).await.unwrap();
        assert_eq!(say(&mut bad, json!({ "type": "hello", "v": 1, "phoneId": id, "token": "nope" })).await["code"], "bad_token");
        let mut stranger = phone(r.addr, &r.pin).await.unwrap();
        assert_eq!(say(&mut stranger, json!({ "type": "hello", "v": 1, "phoneId": "ph-x", "token": token })).await["code"], "unknown_phone");

        // Removing it on the PC closes its connection and its token stops working
        assert!(r.hub.forget(&id));
        match timeout(Duration::from_secs(5), ws.next()).await.unwrap() {
            Some(Ok(Message::Text(t))) => assert!(t.as_str().contains("\"removed\"")),
            other => panic!("expected the removed message, got {other:?}"),
        }
        let mut again = phone(r.addr, &r.pin).await.unwrap();
        assert_eq!(say(&mut again, json!({ "type": "hello", "v": 1, "phoneId": id, "token": token })).await["code"], "unknown_phone");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    /// Nothing more arrives for a moment
    async fn quiet(ws: &mut PhoneWs) {
        loop {
            match timeout(Duration::from_millis(300), ws.next()).await {
                Err(_) => return,
                Ok(Some(Ok(Message::Text(t)))) => panic!("expected nothing, got {t}"),
                Ok(Some(Ok(_))) => continue,
                Ok(other) => panic!("connection ended: {other:?}"),
            }
        }
    }

    fn library(label: &str) -> Value {
        json!({
            "profiles": [
                { "id": "pr1", "name": "Streaming", "body": "mint", "icon": "star", "tree": [
                    { "kind": "page", "id": "p1", "name": "Main", "board": "B1", "cols": 4, "rows": 3 },
                    { "kind": "folder", "id": "f1", "name": "Live", "open": true, "children": [
                        { "kind": "page", "id": "p2", "name": "Scenes", "board": "B2", "cols": 4, "rows": 3 }
                    ] }
                ] },
                { "id": "pr2", "name": "Work", "body": "sky", "icon": "monitor", "tree": [
                    { "kind": "page", "id": "p3", "name": "Meetings", "board": "B3", "cols": 3, "rows": 2 }
                ] }
            ],
            "boards": {
                "B1": [{ "id": "t1", "label": label, "steps": [{ "type": "key", "f": { "keys": "Ctrl+E" } }],
                         "states": [{ "name": "Default", "bg": "mint", "bgType": "image", "bgImage": { "src": "u:1", "fit": "cover" }, "gif": { "id": "none" } }] }],
                "B2": [{ "id": "t2", "label": "BRB", "states": [] }],
                "B3": [{ "id": "t3", "label": "Join", "states": [] }]
            },
            "designs": [{ "id": "d1", "name": "Unused", "layers": [] }],
            "drawnGifs": [],
            "gifFiles": [],
            "images": [{ "id": "u:1", "name": "Pic", "media": "u-1-abc.png", "ar": 1.5 }, { "id": "u:2", "name": "Other", "media": "u-2-zzz.png", "ar": 1 }],
            "active": { "prof": "pr1", "page": "p1" }
        })
    }

    /// The next message from the PC, a text frame or a gzipped binary one
    async fn next_any(ws: &mut PhoneWs) -> (bool, Value) {
        loop {
            match timeout(Duration::from_secs(5), ws.next()).await.expect("the PC did not answer") {
                Some(Ok(Message::Text(t))) => return (false, serde_json::from_str(t.as_str()).unwrap()),
                Some(Ok(Message::Binary(b))) => {
                    let mut text = String::new();
                    std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&b[..]), &mut text).unwrap();
                    return (true, serde_json::from_str(&text).unwrap());
                }
                Some(Ok(_)) => continue,
                other => panic!("connection ended: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn a_profile_the_phone_kept_is_named_and_a_new_one_comes_gzipped() {
        let r = rig().await;
        r.hub.set_library_value(library("Mute"));
        let code = r.hub.start_pairing().unwrap().code;
        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        let paired = say(&mut ws, pair_msg(&code, "dev-k")).await;
        let (id, token) = (paired["phoneId"].as_str().unwrap().to_string(), paired["token"].as_str().unwrap().to_string());
        let first = next(&mut ws).await;
        assert_eq!(first["type"], "layout");
        let rev1 = first["rev"].as_str().unwrap().to_string();
        drop(ws);
        wait_until(|| !r.hub.list()[0].online).await;

        // Back with that layout kept, taking gzip: nothing big is sent
        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        let hello = say(&mut ws, json!({ "type": "hello", "v": 1, "phoneId": id, "token": token, "rev": rev1, "has": [rev1], "gz": true })).await;
        assert_eq!(hello["type"], "welcome");
        assert_eq!(next(&mut ws).await, json!({ "type": "page", "page": "p1" }));
        quiet(&mut ws).await;

        // Another profile on the PC: the whole layout, gzipped
        r.hub.set_pc_page("pr2".into(), "p3".into());
        let (binary, l2) = next_any(&mut ws).await;
        assert!(binary);
        assert_eq!((l2["type"].as_str(), l2["page"].as_str()), (Some("layout"), Some("p3")));

        // Back to the first: named only
        r.hub.set_pc_page("pr1".into(), "p1".into());
        let (binary, known) = next_any(&mut ws).await;
        assert!(!binary);
        assert_eq!(known, json!({ "type": "known", "rev": rev1, "page": "p1" }));

        // The phone lost it: it asks, and gets it in full
        tell(&mut ws, json!({ "type": "needLayout", "rev": rev1 })).await;
        let (binary, again) = next_any(&mut ws).await;
        assert!(binary);
        assert_eq!((again["type"].as_str(), again["rev"].as_str()), (Some("layout"), Some(rev1.as_str())));
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn a_paired_phone_gets_its_pages_and_stays_in_sync() {
        use base64::Engine;
        let r = rig().await;
        std::fs::write(r.dir.join("media").join("u-1-abc.png"), [1u8, 2, 3, 4]).unwrap();
        r.hub.set_settings_value(json!({ "names": true, "awake": true, "haptics": false }));
        r.hub.set_library_value(library("Mute"));

        // Pairing is followed straight away by the layout of the profile open on the PC
        let code = r.hub.start_pairing().unwrap().code;
        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        let paired = say(&mut ws, pair_msg(&code, "dev-s")).await;
        let id = paired["phoneId"].as_str().unwrap().to_string();
        let token = paired["token"].as_str().unwrap().to_string();
        let l = next(&mut ws).await;
        assert_eq!(l["type"], "layout");
        assert_eq!(l["page"], "p1");
        let body = &l["layout"];
        assert_eq!(body["profile"]["name"], "Streaming");
        assert_eq!(body["tree"][0]["buttons"][0]["label"], "Mute");
        assert_eq!(body["tree"][1]["children"][0]["id"], "p2");
        assert_eq!(body["prefs"]["keepAwake"], true);
        // Only what the buttons use: one picture, no unused design
        assert_eq!(body["images"].as_array().unwrap().len(), 1);
        assert_eq!(body["designs"], json!([]));
        assert_eq!(l["media"], json!([{ "file": "u-1-abc.png", "bytes": 4, "sha256": sha256_hex(&[1, 2, 3, 4]) }]));
        let rev = l["rev"].as_str().unwrap().to_string();

        // Saving the same library sends nothing; a change sends the new layout
        r.hub.set_library_value(library("Mute"));
        quiet(&mut ws).await;
        r.hub.set_library_value(library("Mic"));
        let l = next(&mut ws).await;
        assert_eq!(l["layout"]["tree"][0]["buttons"][0]["label"], "Mic");
        assert_ne!(l["rev"], rev);
        let rev = l["rev"].as_str().unwrap().to_string();

        // The PC turns the page: only the page is sent
        r.hub.set_pc_page("pr1".into(), "p2".into());
        assert_eq!(next(&mut ws).await, json!({ "type": "page", "page": "p2" }));

        // Pictures: the one it uses comes in parts; anything else is refused
        let m = say(&mut ws, json!({ "type": "getMedia", "file": "u-1-abc.png" })).await;
        assert_eq!((m["type"].as_str(), m["index"].as_u64(), m["total"].as_u64()), (Some("media"), Some(0), Some(1)));
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(m["data"].as_str().unwrap()).unwrap(), [1u8, 2, 3, 4]);
        assert_eq!(say(&mut ws, json!({ "type": "getMedia", "file": "u-2-zzz.png" })).await["type"], "mediaMissing");
        assert_eq!(say(&mut ws, json!({ "type": "getMedia", "file": "../devices.json" })).await["type"], "mediaMissing");

        // Presses reach the PC; a button not on the phone is refused; the PC reports back
        tell(&mut ws, json!({ "type": "press", "buttonId": "t1" })).await;
        wait_until(|| r.presses.lock().unwrap().iter().any(|p| p.button_id == "t1" && p.phone_name == "Pixel 8" && p.phone_id == id)).await;
        let no = say(&mut ws, json!({ "type": "press", "buttonId": "t3" })).await;
        assert_eq!((no["type"].as_str(), no["ok"].as_bool()), (Some("ran"), Some(false)));
        assert!(r.hub.press_result(&id, "t1", true, "Pressed Ctrl+E"));
        assert_eq!(next(&mut ws).await, json!({ "type": "ran", "buttonId": "t1", "ok": true, "message": "Pressed Ctrl+E" }));

        // Following a page of another profile sends that profile
        r.hub.update(&id, PhonePatch { follows: Some("p3".into()), ..Default::default() }).unwrap();
        let l = next(&mut ws).await;
        assert_eq!((l["layout"]["profile"]["name"].as_str(), l["page"].as_str()), (Some("Work"), Some("p3")));
        let rev3 = l["rev"].as_str().unwrap().to_string();
        assert_ne!(rev3, rev);

        // The update notice reaches connected phones
        assert_eq!(r.hub.notify_update("1.1.0", "Faster"), 1);
        assert_eq!(next(&mut ws).await["type"], "updateAvailable");

        // A plugin shows something on its button: a title, and a screen sent once (an OFP)
        r.hub.set_live_title("t3", Some("TWR 123.500".into()));
        assert_eq!(next(&mut ws).await["title"], "TWR 123.500");
        r.hub.set_live_data("t3", json!({ "kind": "ofp", "text": "PLAN" }));
        assert_eq!(next(&mut ws).await["data"]["text"], "PLAN");

        // Back again with the layout it has: no layout is sent, only the page, then what plugins
        // show (it may have lost it, and a plugin doesn't send it again)
        drop(ws);
        wait_until(|| !r.hub.list()[0].online).await;
        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        assert_eq!(say(&mut ws, json!({ "type": "hello", "v": 1, "phoneId": id, "token": token, "rev": rev3 })).await["type"], "welcome");
        assert_eq!(next(&mut ws).await, json!({ "type": "page", "page": "p3" }));
        assert_eq!(next(&mut ws).await, json!({ "type": "live", "buttonId": "t3", "title": "TWR 123.500", "icon": null }));
        assert_eq!(next(&mut ws).await, json!({ "type": "data", "buttonId": "t3", "data": { "kind": "ofp", "text": "PLAN" } }));
        quiet(&mut ws).await;
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn codes_stop_after_five_wrong_tries_or_when_expired() {
        let r = rig().await;
        let code = r.hub.start_pairing().unwrap().code;
        let wrong = if code == "000000" { "111111" } else { "000000" };
        for _ in 0..4 {
            let mut ws = phone(r.addr, &r.pin).await.unwrap();
            assert_eq!(say(&mut ws, pair_msg(wrong, "d")).await["code"], "bad_code");
        }
        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        assert_eq!(say(&mut ws, pair_msg(wrong, "d")).await["code"], "too_many_tries");
        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        assert_eq!(say(&mut ws, pair_msg(&code, "d")).await["code"], "no_pairing");

        let code = r.hub.start_pairing().unwrap().code;
        r.hub.expire_pairing_now();
        let mut ws = phone(r.addr, &r.pin).await.unwrap();
        assert_eq!(say(&mut ws, pair_msg(&code, "d")).await["code"], "expired");
        let reasons: Vec<String> = r.events.lock().unwrap().iter().map(|e| e.reason.clone()).collect();
        assert_eq!(reasons, ["too_many_tries", "expired"]);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn pairing_the_same_phone_again_replaces_it() {
        let r = rig().await;
        for _ in 0..2 {
            let code = r.hub.start_pairing().unwrap().code;
            let mut ws = phone(r.addr, &r.pin).await.unwrap();
            assert_eq!(say(&mut ws, pair_msg(&code, "same-device")).await["type"], "paired");
        }
        wait_until(|| r.hub.list().len() == 1).await;
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn a_pc_with_another_certificate_is_refused_by_the_phone() {
        let r = rig().await;
        let err = phone(r.addr, &"0".repeat(64)).await.err().expect("the phone must refuse a PC it did not pair with");
        assert!(err.contains("fingerprint") || err.to_lowercase().contains("certificate"), "{err}");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[test]
    fn example_phones_from_earlier_versions_are_dropped() {
        let dir = std::env::temp_dir().join(format!("minniondeck-test-old-{}", crate::storage::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("devices.json");
        std::fs::write(&file, r#"{"schema_version":1,"saved_at":1,"data":{"phones":[{"id":"px","name":"Pixel 8","ver":"2.3.0","online":true}],"channel":"stable"}}"#).unwrap();
        let hub = Hub::load(file.clone(), dir.join("media"), "f".repeat(64), "PC".into(), Box::new(|_| {}));
        assert!(hub.list().is_empty());
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["data"]["phones"], json!([]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_paired_phone_saved_without_newer_fields_stays_paired() {
        let dir = std::env::temp_dir().join(format!("minniondeck-test-fields-{}", crate::storage::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("devices.json");
        let hash = sha256_hex(b"secret");
        // No os, aspect, follows, orient, fit or grid: as an older (or newer) version might save it
        std::fs::write(&file, json!({"schema_version":1,"saved_at":1,"data":{"phones":[{"id":"ph-1","deviceId":"d1","name":"Joker","tokenHash":hash}]}}).to_string()).unwrap();
        let hub = Hub::load(file.clone(), dir.join("media"), "f".repeat(64), "PC".into(), Box::new(|_| {}));
        let list = hub.list();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].follows.as_str(), list[0].fit.as_str(), list[0].grid.as_str()), ("pc", "turn", "pc"));
        assert!(hub.check_hello("ph-1", "secret").is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
