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

use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, CString, c_char, c_void};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

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

/// Messages a plugin may send each second, and how many at once after a quiet spell
const RATE: f64 = 60.0;
const BURST: f64 = 120.0;
/// A call that waited longer than this for a busy plugin is skipped (the app stopped waiting for
/// it at the same time): a stuck plugin doesn't fire a queue of old presses when it wakes up.
const CALL_TOO_OLD: Duration = Duration::from_secs(15);

/// How much a plugin may send: past its rate, a change to a button (its look, title, icon, data)
/// waits and only the newest one per button is sent once there is room; anything else is dropped.
pub(crate) struct Gate {
    tokens: f64,
    at: Instant,
    held: BTreeMap<(String, String), Value>,
}

impl Gate {
    pub(crate) fn new(now: Instant) -> Gate {
        Gate { tokens: BURST, at: now, held: BTreeMap::new() }
    }

    fn refill(&mut self, now: Instant) {
        self.tokens = (self.tokens + now.saturating_duration_since(self.at).as_secs_f64() * RATE).min(BURST);
        self.at = now;
    }

    /// A message from the plugin: Some when it goes now.
    pub(crate) fn offer(&mut self, msg: Value, now: Instant) -> Option<Value> {
        self.refill(now);
        let kind = msg.get("t").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let button = msg.get("button").and_then(|b| b.as_str()).unwrap_or("").to_string();
        let keep_newest = matches!(kind.as_str(), "set_state" | "set_title" | "set_icon" | "set_data" | "refresh_options");
        // one waiting already for this button: this newer one replaces it, in order
        if keep_newest && self.held.contains_key(&(kind.clone(), button.clone())) {
            self.held.insert((kind, button), msg);
            return None;
        }
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return Some(msg);
        }
        if keep_newest && self.held.len() < 1000 {
            self.held.insert((kind, button), msg);
        }
        None
    }

    /// What waited and may go now.
    pub(crate) fn drain(&mut self, now: Instant) -> Vec<Value> {
        self.refill(now);
        let mut out = vec![];
        while self.tokens >= 1.0 {
            let Some(k) = self.held.keys().next().cloned() else { break };
            if let Some(v) = self.held.remove(&k) {
                self.tokens -= 1.0;
                out.push(v);
            }
        }
        out
    }
}

/// What a plugin's `send` calls get: which plugin it is, where to write, and its rate gate
struct Ctx {
    id: String,
    out: Out,
    gate: Mutex<Gate>,
    stopped: AtomicBool,
}

unsafe extern "C" fn send(ctx: *mut c_void, msg: *const c_char) {
    if ctx.is_null() || msg.is_null() {
        return;
    }
    let ctx = unsafe { &*(ctx as *const Ctx) };
    let text = unsafe { CStr::from_ptr(msg) }.to_string_lossy();
    if let Ok(msg) = serde_json::from_str::<Value>(&text) {
        let go = ctx.gate.lock().unwrap_or_else(|e| e.into_inner()).offer(msg, Instant::now());
        if let Some(msg) = go {
            write(&ctx.out, &json!({ "t": "plugin", "id": ctx.id, "msg": msg }));
        }
    }
}

/// Sends what a plugin's gate held back, as room comes, until the plugin stops.
fn flush_held(ctx: &'static Ctx) {
    let _ = thread::Builder::new().name(format!("plugin {} out", ctx.id)).spawn(move || {
        while !ctx.stopped.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(100));
            let go = ctx.gate.lock().unwrap_or_else(|e| e.into_inner()).drain(Instant::now());
            for msg in go {
                write(&ctx.out, &json!({ "t": "plugin", "id": ctx.id, "msg": msg }));
            }
        }
    });
}

enum Job {
    Call { req: u64, msg: String, at: Instant },
    Stop(mpsc::Sender<()>),
}

/// Whether `dll` is a DLL copy in the app's plugin-run folder: the only DLLs the host loads, so
/// nothing that can write to the host's input can make it load a DLL from anywhere else.
pub(crate) fn allowed_dll(dll: &Path, run: &Path) -> bool {
    let (Ok(dll), Ok(run)) = (dll.canonicalize(), run.canonicalize()) else { return false };
    dll.parent() == Some(run.as_path()) && dll.extension().is_some_and(|e| e.eq_ignore_ascii_case("dll"))
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
    let ctx_ref: &'static Ctx = Box::leak(Box::new(Ctx { id: id.clone(), out: out.clone(), gate: Mutex::new(Gate::new(Instant::now())), stopped: AtomicBool::new(false) }));
    flush_held(ctx_ref);
    let ctx = ctx_ref as *const Ctx as *mut c_void;
    let host = HostApi { api: API_VERSION, ctx, send };
    let init = CString::new(json!({ "id": id, "dir": dir, "settings": settings }).to_string()).unwrap_or_default();
    let inst = unsafe { start(&host, init.as_ptr()) };
    if inst.is_null() {
        ctx_ref.stopped.store(true, Ordering::Relaxed);
        return fail("The plugin failed to start. Its reason is in the log.".into());
    }
    let _ = ready.send(Ok(()));

    for job in rx.iter() {
        match job {
            Job::Call { req, msg, at } => {
                // the app stopped waiting for this long ago: it isn't done now
                if at.elapsed() > CALL_TOO_OLD {
                    if req != 0 {
                        write(&out, &json!({ "t": "reply", "req": req, "body": { "ok": false, "message": "The plugin was busy, so this was skipped." } }));
                    }
                    continue;
                }
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
                ctx_ref.stopped.store(true, Ordering::Relaxed);
                drop(lib);
                let _ = done.send(());
                return;
            }
        }
    }
    // The host is shutting down without a Stop
    unsafe { stop(inst) };
    ctx_ref.stopped.store(true, Ordering::Relaxed);
}

fn stop_one(tx: mpsc::Sender<Job>) {
    let (done, wait) = mpsc::channel();
    if tx.send(Job::Stop(done)).is_ok() {
        // A plugin stuck in a call can't stop; give up on it rather than hang
        let _ = wait.recv_timeout(Duration::from_secs(3));
    }
}

/// The plugins running: each one's queue, and how many times each id was loaded or unloaded (a
/// load that finishes after an unload or a newer load of the same plugin stops itself).
#[derive(Default)]
struct Running {
    queues: HashMap<String, mpsc::Sender<Job>>,
    turn: HashMap<String, u64>,
}

fn lock(r: &Mutex<Running>) -> std::sync::MutexGuard<'_, Running> {
    r.lock().unwrap_or_else(|e| e.into_inner())
}

/// Run as the plugin host until the app closes our input
pub fn main() {
    let out: Out = Arc::new(Mutex::new(std::io::stdout()));
    let running: Arc<Mutex<Running>> = Arc::new(Mutex::new(Running::default()));
    let reply = |req: u64, body: Value| write(&out, &json!({ "t": "reply", "req": req, "body": body }));
    // only the DLL copies the app makes, in its plugin-run folder
    let run_dir = crate::backend::paths::Paths::find().root.join("plugin-run");
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
        let req = msg.get("req").and_then(|v| v.as_u64()).unwrap_or(0);
        let s = |k: &str| msg.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        match msg.get("t").and_then(|v| v.as_str()).unwrap_or("") {
            "load" => {
                let (id, dll) = (s("id"), s("dll"));
                if !allowed_dll(Path::new(&dll), &run_dir) {
                    reply(req, json!({ "ok": false, "error": "That DLL isn't one Minniondeck prepared, so it wasn't loaded." }));
                    continue;
                }
                let (old, mine) = {
                    let mut r = lock(&running);
                    let t = r.turn.entry(id.clone()).or_insert(0);
                    *t += 1;
                    let mine = *t;
                    (r.queues.remove(&id), mine)
                };
                if let Some(old) = old {
                    stop_one(old);
                }
                // a plugin's start can take seconds: it starts on its own thread, so presses and
                // other plugins aren't held up meanwhile
                let settings = msg.get("settings").cloned().unwrap_or_else(|| json!({}));
                let (running, out, dir) = (running.clone(), out.clone(), s("dir"));
                thread::spawn(move || {
                    let r = start_plugin(id.clone(), dll, dir, settings, out.clone());
                    let body = match r {
                        Ok(tx) => {
                            let mut g = running.lock().unwrap_or_else(|e| e.into_inner());
                            if g.turn.get(&id) == Some(&mine) {
                                g.queues.insert(id, tx);
                                json!({ "ok": true })
                            } else {
                                drop(g);
                                stop_one(tx);
                                json!({ "ok": false, "error": "It was unloaded while it started." })
                            }
                        }
                        Err(e) => json!({ "ok": false, "error": e }),
                    };
                    write(&out, &json!({ "t": "reply", "req": req, "body": body }));
                });
            }
            "unload" => {
                let id = s("id");
                let tx = {
                    let mut r = lock(&running);
                    *r.turn.entry(id.clone()).or_insert(0) += 1;
                    r.queues.remove(&id)
                };
                if let Some(tx) = tx {
                    stop_one(tx);
                }
                reply(req, json!({ "ok": true }));
            }
            "call" => {
                let job = Job::Call { req, msg: msg.get("msg").cloned().unwrap_or_default().to_string(), at: Instant::now() };
                let sent = lock(&running).queues.get(&s("id")).is_some_and(|tx| tx.send(job).is_ok());
                if !sent {
                    reply(req, json!({ "ok": false, "message": "That plugin isn't running." }));
                }
            }
            "quit" => break,
            _ => {}
        }
    }
    let all: Vec<mpsc::Sender<Job>> = lock(&running).queues.drain().map(|(_, tx)| tx).collect();
    for tx in all {
        stop_one(tx);
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    // a plugin that sends too much can't flood the app; button changes aren't lost, only squashed
    #[test]
    fn a_chatty_plugin_is_slowed_and_only_the_newest_change_per_button_waits() {
        let t0 = Instant::now();
        let mut g = Gate::new(t0);
        let set = |b: &str, n: u64| json!({ "t": "set_title", "button": b, "title": n.to_string() });
        let mut sent = 0;
        for n in 0..1000 {
            if g.offer(set("a", n), t0).is_some() {
                sent += 1;
            }
        }
        assert_eq!(sent, BURST as usize, "the burst goes, the rest waits");
        assert!(g.offer(json!({ "t": "log", "text": "x" }), t0).is_none(), "a log line past the rate is dropped");
        assert!(g.offer(set("b", 1), t0).is_none());
        // a moment later: what waited goes, the newest of each, and nothing older
        let later = g.drain(t0 + Duration::from_millis(500));
        assert_eq!(later.len(), 2);
        assert!(later.contains(&set("a", 999)) && later.contains(&set("b", 1)));
        assert!(g.drain(t0 + Duration::from_secs(5)).is_empty());
        // a change waiting for a button isn't overtaken by a newer one sent straight away
        let mut g = Gate { tokens: 0.0, at: t0, held: BTreeMap::new() };
        assert!(g.offer(set("a", 1), t0).is_none());
        assert!(g.offer(set("a", 2), t0 + Duration::from_secs(1)).is_none());
        assert_eq!(g.drain(t0 + Duration::from_secs(1)), vec![set("a", 2)]);
    }

    #[test]
    fn only_dll_copies_in_plugin_run_are_loaded() {
        let root = std::env::temp_dir().join(format!("md-host-dll-{}", crate::backend::util::now_ms()));
        let run = root.join("plugin-run");
        std::fs::create_dir_all(run.join("sub")).unwrap();
        for f in ["plugin-run/a-1.dll", "plugin-run/sub/b.dll", "plugin-run/c.txt", "evil.dll"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
        assert!(allowed_dll(&run.join("a-1.dll"), &run));
        assert!(allowed_dll(&run.join("..").join("plugin-run").join("a-1.dll"), &run));
        assert!(!allowed_dll(&root.join("evil.dll"), &run));
        assert!(!allowed_dll(&run.join("..").join("evil.dll"), &run));
        assert!(!allowed_dll(&run.join("sub").join("b.dll"), &run));
        assert!(!allowed_dll(&run.join("c.txt"), &run));
        assert!(!allowed_dll(&run.join("missing.dll"), &run));
        let _ = std::fs::remove_dir_all(&root);
    }
}
