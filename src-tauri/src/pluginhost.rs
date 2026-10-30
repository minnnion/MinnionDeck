//! The plugin host: Minniondeck.exe started with `--plugin-host` by the app (see plugins.rs).
//!
//! Every plugin DLL runs in this one helper process, each on its own thread, so a plugin that
//! crashes takes down only this process: the app notices, restarts it and loads the plugins again.
//! The app talks to it with one JSON message per line:
//!
//!   app → host  {"t":"load","req":1,"id":"obs","dll":"<copy of the DLL>","dir":"<plugin folder>","settings":{…}}
//!   app → host  {"t":"unload","req":2,"id":"obs"}
//!   app → host  {"t":"call","req":3,"id":"obs","msg":{…}}      a message for the plugin (see plugin-kit)
//!   app → host  {"t":"quit"}
//!   host → app  {"t":"reply","req":3,"body":{…}}               the answer to a request (req 0: nobody waits)
//!   host → app  {"t":"plugin","id":"obs","msg":{…}}            something the plugin sent (set a title, log…)
//!
//! The DLL interface (version 2) is documented in plugin-kit/src/lib.rs.

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

/// The argument that starts Minniondeck.exe as the plugin host
pub const ARG: &str = "--plugin-host";
/// The DLL interface version the host speaks
pub const API_VERSION: u32 = 2;

#[repr(C)]
struct HostApi {
    api: u32,
    ctx: *mut c_void,
    send: unsafe extern "C" fn(*mut c_void, *const c_char),
}

type Out = Arc<Mutex<std::io::Stdout>>;

fn write(out: &Out, v: &Value) {
    if let Ok(mut o) = out.lock() {
        let _ = writeln!(o, "{v}");
        let _ = o.flush();
    }
}

/// What a plugin's `send` calls get: which plugin it is and where to write
struct Ctx {
    id: String,
    out: Out,
}

unsafe extern "C" fn send(ctx: *mut c_void, msg: *const c_char) {
    if ctx.is_null() || msg.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const Ctx) };
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    if let Ok(msg) = serde_json::from_str::<Value>(&text) {
        write(&ctx.out, &json!({ "t": "plugin", "id": ctx.id, "msg": msg }));
    }
}

enum Job {
    Call { req: u64, msg: String },
    Stop(mpsc::Sender<()>),
}

type StartFn = unsafe extern "C" fn(*const HostApi, *const c_char) -> *mut c_void;
type CallFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char;
type FreeFn = unsafe extern "C" fn(*mut c_char);
type StopFn = unsafe extern "C" fn(*mut c_void);

/// Load a plugin on its own thread. Returns once it has started (or failed to).
fn start_plugin(id: String, dll: String, dir: String, settings: Value, out: Out) -> Result<mpsc::Sender<Job>, String> {
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let (tx, rx) = mpsc::channel::<Job>();
    thread::Builder::new()
        .name(format!("plugin {id}"))
        .spawn(move || worker(id, dll, dir, settings, out, ready_tx, rx))
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => Ok(tx),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("The plugin took more than 10 seconds to start.".into()),
    }
}

fn worker(id: String, dll: String, dir: String, settings: Value, out: Out, ready: mpsc::Sender<Result<(), String>>, rx: mpsc::Receiver<Job>) {
    let fail = |e: String| {
        let _ = ready.send(Err(e));
    };
    let lib = match unsafe { libloading::Library::new(&dll) } {
        Ok(l) => l,
        Err(e) => return fail(format!("Could not load the DLL: {e}")),
    };
    let syms = unsafe {
        (|| -> Result<(u32, StartFn, CallFn, FreeFn, StopFn), String> {
            let api = *lib.get::<unsafe extern "C" fn() -> u32>(b"md_plugin_api\0").map_err(|_| "The DLL has no md_plugin_api.".to_string())?;
            let api = api();
            if api != API_VERSION {
                return Err(format!("Built for plugin interface {api}, but Minniondeck uses {API_VERSION}. Rebuild it with the current plugin kit."));
            }
            let get = |name: &[u8]| format!("The DLL has no {}.", String::from_utf8_lossy(&name[..name.len() - 1]));
            Ok((
                api,
                *lib.get::<StartFn>(b"md_plugin_start\0").map_err(|_| get(b"md_plugin_start\0"))?,
                *lib.get::<CallFn>(b"md_plugin_call\0").map_err(|_| get(b"md_plugin_call\0"))?,
                *lib.get::<FreeFn>(b"md_plugin_free\0").map_err(|_| get(b"md_plugin_free\0"))?,
                *lib.get::<StopFn>(b"md_plugin_stop\0").map_err(|_| get(b"md_plugin_stop\0"))?,
            ))
        })()
    };
    let (_, start, call, free, stop) = match syms {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    // Kept for the life of the process: a plugin thread may still send briefly while stopping
    let ctx = Box::into_raw(Box::new(Ctx { id: id.clone(), out: out.clone() })) as *mut c_void;
    let host = HostApi { api: API_VERSION, ctx, send };
    let init = CString::new(json!({ "id": id, "dir": dir, "settings": settings }).to_string()).unwrap_or_default();
    let inst = unsafe { start(&host, init.as_ptr()) };
    if inst.is_null() {
        return fail("The plugin failed to start. Its reason is in the log.".into());
    }
    let _ = ready.send(Ok(()));

    for job in rx.iter() {
        match job {
            Job::Call { req, msg } => {
                let body = match CString::new(msg) {
                    Ok(m) => unsafe {
                        let p = call(inst, m.as_ptr());
                        if p.is_null() {
                            json!({ "ok": false, "message": "The plugin gave no answer." })
                        } else {
                            let text = CStr::from_ptr(p).to_string_lossy().into_owned();
                            free(p);
                            serde_json::from_str(&text).unwrap_or_else(|_| json!({ "ok": false, "message": "The plugin gave an answer Minniondeck can't read." }))
                        }
                    },
                    Err(_) => json!({ "ok": false, "message": "Bad message." }),
                };
                write(&out, &json!({ "t": "reply", "req": req, "body": body }));
            }
            Job::Stop(done) => {
                unsafe { stop(inst) };
                drop(lib);
                let _ = done.send(());
                return;
            }
        }
    }
    // The host is shutting down without a Stop
    unsafe { stop(inst) };
}

fn stop_one(tx: mpsc::Sender<Job>) {
    let (done, wait) = mpsc::channel();
    if tx.send(Job::Stop(done)).is_ok() {
        // A plugin stuck in a call can't stop; give up on it rather than hang
        let _ = wait.recv_timeout(Duration::from_secs(3));
    }
}

/// Run as the plugin host until the app closes our input
pub fn main() {
    let out: Out = Arc::new(Mutex::new(std::io::stdout()));
    let mut plugins: HashMap<String, mpsc::Sender<Job>> = HashMap::new();
    let reply = |req: u64, body: Value| write(&out, &json!({ "t": "reply", "req": req, "body": body }));
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let req = msg.get("req").and_then(|v| v.as_u64()).unwrap_or(0);
        let s = |k: &str| msg.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        match msg.get("t").and_then(|v| v.as_str()).unwrap_or("") {
            "load" => {
                let id = s("id");
                if let Some(old) = plugins.remove(&id) {
                    stop_one(old);
                }
                let settings = msg.get("settings").cloned().unwrap_or_else(|| json!({}));
                match start_plugin(id.clone(), s("dll"), s("dir"), settings, out.clone()) {
                    Ok(tx) => {
                        plugins.insert(id, tx);
                        reply(req, json!({ "ok": true }));
                    }
                    Err(e) => reply(req, json!({ "ok": false, "error": e })),
                }
            }
            "unload" => {
                if let Some(tx) = plugins.remove(&s("id")) {
                    stop_one(tx);
                }
                reply(req, json!({ "ok": true }));
            }
            "call" => {
                let sent = plugins.get(&s("id")).is_some_and(|tx| tx.send(Job::Call { req, msg: msg.get("msg").cloned().unwrap_or_default().to_string() }).is_ok());
                if !sent {
                    reply(req, json!({ "ok": false, "message": "That plugin isn't running." }));
                }
            }
            "quit" => break,
            _ => {}
        }
    }
    for (_, tx) in plugins.drain() {
        stop_one(tx);
    }
    std::process::exit(0);
}
