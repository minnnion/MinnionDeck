// Phones: pairing, the always-on connection, and keeping phones in sync with the PC.
//
//   protocol.rs  the messages a phone and the PC exchange
//   identity.rs  this PC's TLS certificate, pinned by phones
//   pairing.rs   one-time codes and the QR code
//   hub.rs       paired phones, who is connected, the pairing session, sync
//   sync.rs      what each phone shows, worked out from the saved library
//   server.rs    the network side

pub mod hub;
pub mod identity;
pub mod pairing;
pub mod protocol;
pub mod server;
pub mod sync;

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use hub::{Event, Hub, PairingInfo, PhoneInfo, PhonePatch, ServerStatus};

pub const DEFAULT_PORT: u16 = 47800;

/// The running phone server, so it can be moved to another port without restarting the app
pub struct Server {
    task: std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    tls: Option<Arc<rustls::ServerConfig>>,
}

/// Start listening for phones. Runs for as long as the app does. If something stops the
/// server (no certificate, port in use), the app still works and the Phones page says why.
pub fn start(app: &AppHandle, store: &crate::storage::Store) -> (Arc<Hub>, Server) {
    let handle = app.clone();
    let notify: hub::Notify = Box::new(move |e| {
        let _ = match e {
            Event::Phones(list) => {
                crate::tray::set_phones(&handle, list.iter().filter(|p| p.online).count());
                handle.emit("phones_changed", list)
            }
            Event::Pairing(p) => handle.emit("pairing_changed", p),
            // The UI runs the button, then reports back with phone_press_result
            Event::Press(p) => handle.emit("phone_press", p),
            // The UI passes it to the button's plugin
            Event::View(v) => handle.emit("phone_view", v),
            Event::Turn(t) => handle.emit("phone_turn", t),
            Event::Pages(p) => handle.emit("phone_pages", p),
            Event::PhonePage(p) => handle.emit("phone_page", p),
        };
    });
    let port = crate::storage::setting_value(store, "port")
        .and_then(|v| v.as_str().and_then(|s| s.trim().parse::<u16>().ok()).or_else(|| v.as_u64().and_then(|n| u16::try_from(n).ok())))
        .filter(|p| *p >= 1024)
        .unwrap_or(DEFAULT_PORT);
    let identity = identity::Identity::load_or_create(&store.paths.root.join("identity"));
    let fingerprint = identity.as_ref().map(|i| i.fingerprint.clone()).unwrap_or_default();
    let hub = Arc::new(Hub::load(store.paths.devices_file(), store.paths.media.clone(), fingerprint, pairing::pc_name(), notify));
    // What phones show starts from the last save; each new save updates it (storage.rs)
    if let Some(v) = crate::storage::read_data(&store.paths.settings_file()) {
        hub.set_settings_value(v);
    }
    if let Some(v) = crate::storage::read_data(&store.paths.library_file()) {
        hub.set_library_value(v);
    }
    let mut srv = Server { task: std::sync::Mutex::new(None), tls: None };
    match identity.and_then(|i| i.server_config()) {
        Ok(tls) => {
            hub.set_status(false, port, None);
            srv.tls = Some(tls.clone());
            *srv.task.lock().unwrap_or_else(|e| e.into_inner()) = Some(tauri::async_runtime::spawn(server::start(hub.clone(), port, tls)));
        }
        Err(e) => hub.set_status(false, port, Some(format!("Phones can't connect: {e}"))),
    }
    (hub, srv)
}

/// Settings > Phones > Port: move the phone server straight away. Connected phones are told
/// first so they reconnect on the new port. If the new port is taken, nothing changes.
#[tauri::command]
pub async fn set_phone_port(hub: State<'_, Arc<Hub>>, srv: State<'_, Server>, port: u16) -> Result<ServerStatus, String> {
    if port < 1024 {
        return Err("Choose a port from 1024 to 65535.".into());
    }
    let now = hub.status();
    if now.listening && now.port == port {
        return Ok(now);
    }
    let tls = srv.tls.clone().ok_or("Phones can't connect: this PC has no certificate.")?;
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await.map_err(|_| format!("Port {port} is in use by another app. Choose another port."))?;
    hub.notify_port(port);
    if let Some(old) = srv.task.lock().unwrap_or_else(|e| e.into_inner()).take() {
        old.abort();
    }
    hub.set_status(true, port, None);
    let task = tauri::async_runtime::spawn(server::serve(hub.inner().clone(), listener, tokio_rustls::TlsAcceptor::from(tls)));
    *srv.task.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
    Ok(hub.status())
}

#[tauri::command]
pub fn get_phones(hub: State<Arc<Hub>>) -> Vec<PhoneInfo> {
    hub.list()
}

#[tauri::command]
pub fn phone_server_status(hub: State<Arc<Hub>>) -> ServerStatus {
    hub.status()
}

#[tauri::command]
pub fn start_pairing(hub: State<Arc<Hub>>) -> Result<PairingInfo, String> {
    hub.start_pairing()
}

#[tauri::command]
pub fn cancel_pairing(hub: State<Arc<Hub>>) {
    hub.cancel_pairing()
}

#[tauri::command]
pub fn forget_phone(hub: State<Arc<Hub>>, id: String) -> bool {
    hub.forget(&id)
}

#[tauri::command]
pub fn update_phone(hub: State<Arc<Hub>>, id: String, patch: PhonePatch) -> Result<(), String> {
    hub.update(&id, patch)
}

/// The page open on the PC changed (phones that follow the PC turn to it)
#[tauri::command]
pub fn set_pc_page(hub: State<Arc<Hub>>, profile: String, page: String) {
    hub.set_pc_page(profile, page)
}

/// How a button pressed on a phone went, sent back to that phone
#[tauri::command]
pub fn phone_press_result(hub: State<Arc<Hub>>, phone_id: String, button_id: String, ok: bool, message: String) -> bool {
    hub.press_result(&phone_id, &button_id, ok, &message)
}

/// Tell connected phones a newer phone app is out (for the update source, later)
#[tauri::command]
pub fn notify_phone_update(hub: State<Arc<Hub>>, version: String, notes: String) -> usize {
    hub.notify_update(&version, &notes)
}
