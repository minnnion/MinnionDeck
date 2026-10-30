//! Speech: text to speech with Gradium's Phonon model (31M, English), run by the app for itself
//! and for plugins. The app is the hub: a plugin asks the app to speak (Host::speak in the kit)
//! and never runs the model. The model and voices come with the minnion_speech plugin, in its
//! data folder; this engine reads them from there.
//!
//! One worker thread owns the model and a queue, so lines are spoken one after another and never
//! overlap. The model loads on first use (about 70 ms) and is dropped when minnion_speech is
//! installed, reloaded or removed, so a new copy is read next time. Nothing is downloaded: the
//! `ptts` crate is built without its `hub` feature.
//!
//! Measured on a 16-thread Ryzen 7 5800X: 5–6×
//! real time; a short callout takes 0.3–0.4 s to make.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use ptts::checkpoint::read_config;
use ptts::preprocess::Lang;
use ptts::synth::{Quant, SpeechOptions, Synth, SynthBuilder};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter};

/// The plugin that brings the model and voices
pub const PLUGIN_ID: &str = "minnion_speech";
/// The longest line spoken, in characters
const MAX_TEXT: usize = 500;
/// Lines waiting to be spoken; more are refused, so a plugin can't pile up minutes of speech
const MAX_WAITING: usize = 8;
/// The voice that comes with the model
const DEFAULT_VOICE: &str = "default";
/// Worker threads for the model. Left alone it uses every core (16 on the test PC), which made
/// speaking busy ~9.5 cores. Measured on 12 lines (2026-10-27): 16 threads 5.9 s and 56 s of CPU,
/// 4 threads 4.7 s and 19 s, 2 threads 6.8 s and 15 s. 4 is faster than 16 and a third of the CPU.
const SPEECH_THREADS: usize = 4;

/// A voice the model can speak with
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Voice {
    /// The id the model knows it by ("default", or the catalogue's voice id)
    pub id: String,
    pub name: String,
    /// en-us, en-gb
    pub locale: String,
    pub gender: String,
    pub description: String,
    #[serde(skip)]
    file: PathBuf,
}

/// What the Testing page shows about the engine
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    /// minnion_speech is installed with its model
    installed: bool,
    loaded: bool,
    load_ms: Option<u64>,
    error: Option<String>,
    waiting: usize,
    voices: Vec<Voice>,
    /// Where the model is read from
    folder: String,
}

struct Job {
    id: u64,
    text: String,
    voice: String,
    /// Who asked: "Testing page" or a plugin's id
    from: String,
    /// Stop throws away lines asked for before it
    round: u64,
}

enum Msg {
    Say(Job),
    Forget,
}

struct Engine {
    app: AppHandle,
    /// <app data>\plugins\minnion_speech\data
    data: PathBuf,
    tx: Mutex<Sender<Msg>>,
    waiting: AtomicUsize,
    next_id: AtomicU64,
    round: AtomicU64,
    state: Mutex<LoadState>,
    /// Wakes the worker while a line plays, so Stop cuts it off at once
    wake: Condvar,
    wake_lock: Mutex<()>,
}

impl Engine {
    /// Wait while a line plays: true if Stop came, false when the time is up
    fn stopped_within(&self, round: u64, time: Duration) -> bool {
        let end = Instant::now() + time;
        let mut g = lock(&self.wake_lock);
        loop {
            if self.round.load(Ordering::Relaxed) != round {
                return true;
            }
            let now = Instant::now();
            if now >= end {
                return false;
            }
            g = self.wake.wait_timeout(g, end - now).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
        }
    }
}

#[derive(Default)]
struct LoadState {
    loaded: bool,
    load_ms: Option<u64>,
    error: Option<String>,
}

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Start the engine's thread. The model isn't read until the first line.
pub fn init(app: &AppHandle, root: &Path) {
    let (tx, rx) = mpsc::channel();
    let engine = Arc::new(Engine {
        app: app.clone(),
        data: root.join("plugins").join(PLUGIN_ID).join("data"),
        tx: Mutex::new(tx),
        waiting: AtomicUsize::new(0),
        next_id: AtomicU64::new(1),
        round: AtomicU64::new(0),
        state: Mutex::new(LoadState::default()),
        wake: Condvar::new(),
        wake_lock: Mutex::new(()),
    });
    if ENGINE.set(engine.clone()).is_ok() {
        let _ = std::thread::Builder::new().name("speech".into()).spawn(move || work(engine, rx));
    }
}

/// Queue a line. `voice` is a voice id or name; empty means the model's own voice. Returns the
/// line's id, which the speech_event messages carry.
pub fn say(text: &str, voice: &str, from: &str) -> Result<u64, String> {
    let e = ENGINE.get().ok_or("Speech isn't running.")?;
    let text: String = text.trim().chars().take(MAX_TEXT).collect();
    if text.is_empty() {
        return Err("There's nothing to say.".into());
    }
    if !model_dir(&e.data).join("model.q8.gguf").is_file() {
        return Err(format!("The speech model isn't installed. Install the {PLUGIN_ID} plugin."));
    }
    if e.waiting.load(Ordering::Relaxed) >= MAX_WAITING {
        return Err(format!("{MAX_WAITING} lines are already waiting to be spoken."));
    }
    let id = e.next_id.fetch_add(1, Ordering::Relaxed);
    let job = Job { id, text, voice: voice.trim().to_string(), from: from.chars().take(40).collect(), round: e.round.load(Ordering::Relaxed) };
    e.waiting.fetch_add(1, Ordering::Relaxed);
    if lock(&e.tx).send(Msg::Say(job)).is_err() {
        e.waiting.fetch_sub(1, Ordering::Relaxed);
        return Err("Speech stopped. Restart Minniondeck.".into());
    }
    Ok(id)
}

/// Stop speaking and throw away the lines waiting. Only tells the worker: Windows stops a sound
/// from the thread that started it (stopping it from the window's thread waited for the line to
/// end and froze the window).
pub fn stop() {
    if let Some(e) = ENGINE.get() {
        let _g = lock(&e.wake_lock);
        e.round.fetch_add(1, Ordering::Relaxed);
        e.wake.notify_all();
    }
}

/// Drop the model, so the next line reads the plugin's files again (it was installed, reloaded
/// or removed)
pub fn forget() {
    if let Some(e) = ENGINE.get() {
        let _ = lock(&e.tx).send(Msg::Forget);
    }
}

fn model_dir(data: &Path) -> PathBuf {
    data.join("model")
}

/// The model's own voice, then the catalogue's voices that are installed (voices.jsonl)
fn voices_in(data: &Path) -> Vec<Voice> {
    let mut out = Vec::new();
    let own = model_dir(data).join("default-voice.safetensors");
    if own.is_file() {
        out.push(Voice { id: DEFAULT_VOICE.into(), name: "Default".into(), locale: "en-us".into(), gender: String::new(), description: "The model's own voice.".into(), file: own });
    }
    let dir = data.join("voices");
    let Ok(text) = fs::read_to_string(dir.join("voices.jsonl")) else { return out };
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let (id, file) = (s("voice_id"), s("filename"));
        // Only plain names: a voice file must stay inside the voices folder
        let safe = |t: &str| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let Some(name) = file.strip_prefix("voices/").and_then(|f| f.strip_suffix(".safetensors")) else { continue };
        if !safe(&id) || !safe(name) || s("language") != "en" {
            continue;
        }
        let path = dir.join(format!("{name}.safetensors"));
        if path.is_file() {
            out.push(Voice { id, name: s("name"), locale: s("locale"), gender: s("gender"), description: s("description"), file: path });
        }
    }
    out
}

/// A voice id or name (any case) to the id the model knows; unknown or empty is the default
fn pick_voice(voices: &[Voice], asked: &str) -> String {
    let asked = asked.trim();
    voices
        .iter()
        .find(|v| v.id == asked || v.name.eq_ignore_ascii_case(asked))
        .map(|v| v.id.clone())
        .unwrap_or_else(|| DEFAULT_VOICE.into())
}

fn load(data: &Path, voices: &[Voice]) -> Result<Synth, String> {
    let m = model_dir(data);
    let config = read_config(m.join("config.json")).map_err(|e| format!("The model's config.json can't be read: {e}"))?;
    let mut b = SynthBuilder::new(config, m.join("model.q8.gguf"), Lang::En).tokenizer_file(m.join("tokenizer.json")).quant(Quant::Q80);
    for v in voices {
        b = b.add_voice(v.id.clone(), v.file.clone());
    }
    b.build().map_err(|e| format!("The speech model didn't load: {e}"))
}

fn emit(app: &AppHandle, kind: &str, job: &Job, extra: Value) {
    let mut body = json!({ "kind": kind, "id": job.id, "text": job.text, "voice": job.voice, "from": job.from });
    if let (Some(b), Some(x)) = (body.as_object_mut(), extra.as_object()) {
        b.extend(x.clone());
    }
    let _ = app.emit("speech_event", body);
}

fn work(e: Arc<Engine>, rx: Receiver<Msg>) {
    // Before the model makes its thread pool (it does on the first line)
    xn::set_num_threads(SPEECH_THREADS.min(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)));
    let mut model: Option<(Synth, Vec<Voice>)> = None;
    while let Ok(msg) = rx.recv() {
        let mut job = match msg {
            Msg::Forget => {
                model = None;
                *lock(&e.state) = LoadState::default();
                continue;
            }
            Msg::Say(job) => job,
        };
        e.waiting.fetch_sub(1, Ordering::Relaxed);
        if job.round != e.round.load(Ordering::Relaxed) {
            emit(&e.app, "skipped", &job, json!({}));
            continue;
        }
        if model.is_none() {
            emit(&e.app, "loading", &job, json!({}));
            let voices = voices_in(&e.data);
            let t = Instant::now();
            let loaded = catch_unwind(AssertUnwindSafe(|| load(&e.data, &voices))).unwrap_or_else(|_| Err("The speech model crashed while loading.".into()));
            let ms = t.elapsed().as_millis() as u64;
            match loaded {
                Ok(synth) => {
                    *lock(&e.state) = LoadState { loaded: true, load_ms: Some(ms), error: None };
                    crate::applog::line(&format!("Speech: model loaded in {ms} ms with {} voices", voices.len()));
                    model = Some((synth, voices));
                }
                Err(why) => {
                    crate::applog::line(&format!("Speech: {why}"));
                    *lock(&e.state) = LoadState { loaded: false, load_ms: None, error: Some(why.clone()) };
                    emit(&e.app, "error", &job, json!({ "error": why }));
                    continue;
                }
            }
        }
        let Some((synth, voices)) = &model else { continue };
        job.voice = pick_voice(voices, &job.voice);
        let t = Instant::now();
        let opts = SpeechOptions::default().voice(job.voice.clone());
        let made = catch_unwind(AssertUnwindSafe(|| synth.say_with(&job.text, &opts)));
        let synth_ms = t.elapsed().as_millis() as u64;
        let pcm = match made {
            Ok(Ok(pcm)) => pcm,
            Ok(Err(err)) => {
                emit(&e.app, "error", &job, json!({ "error": format!("It couldn't say that: {err}") }));
                continue;
            }
            Err(_) => {
                // A panic may leave the model half-changed: read it again next time
                model = None;
                *lock(&e.state) = LoadState { loaded: false, load_ms: None, error: Some("The speech model crashed; it reloads for the next line.".into()) };
                emit(&e.app, "error", &job, json!({ "error": "The speech model crashed; it reloads for the next line." }));
                continue;
            }
        };
        let rate = synth.sample_rate();
        let audio_ms = (pcm.len() as u64 * 1000) / rate.max(1) as u64;
        // Stop may have come while it was being made
        if job.round != e.round.load(Ordering::Relaxed) {
            emit(&e.app, "skipped", &job, json!({}));
            continue;
        }
        emit(&e.app, "speaking", &job, json!({ "synthMs": synth_ms, "audioMs": audio_ms }));
        // The sound plays on its own; this thread keeps its buffer alive until it ends or Stop
        let sound = wav(&pcm, rate);
        if play(&sound) {
            let stopped = e.stopped_within(job.round, Duration::from_millis(audio_ms + 150));
            stop_sound();
            drop(sound);
            emit(&e.app, if stopped { "skipped" } else { "done" }, &job, json!({ "synthMs": synth_ms, "audioMs": audio_ms }));
        } else {
            // Windows refused the sound (no audio device?): say so instead of "Spoken"
            crate::applog::line("Speech: Windows couldn't play a line");
            emit(&e.app, "error", &job, json!({ "synthMs": synth_ms, "audioMs": audio_ms, "error": "Windows couldn't play the sound. Is a speaker or headset connected?" }));
        }
    }
}

/// The engine's state, the voices and where the model is read from
pub fn status() -> StatusView {
    let Some(e) = ENGINE.get() else {
        return StatusView { installed: false, loaded: false, load_ms: None, error: Some("Speech isn't running.".into()), waiting: 0, voices: vec![], folder: String::new() };
    };
    let s = lock(&e.state);
    StatusView {
        installed: model_dir(&e.data).join("model.q8.gguf").is_file(),
        loaded: s.loaded,
        load_ms: s.load_ms,
        error: s.error.clone(),
        waiting: e.waiting.load(Ordering::Relaxed),
        voices: voices_in(&e.data),
        folder: e.data.display().to_string(),
    }
}

/// 16-bit mono PCM in a WAV file, in memory, for PlaySound
fn wav(pcm: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (pcm.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

/// Start playing a WAV held in memory and return at once; false when Windows refused it. The
/// buffer must stay alive until the sound ends or stop_sound is called (the worker sees to it),
/// and both calls come from the worker thread.
#[cfg(windows)]
fn play(wav: &[u8]) -> bool {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
    use windows::core::PCWSTR;
    unsafe { PlaySoundW(PCWSTR(wav.as_ptr() as *const u16), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT).as_bool() }
}

/// Stop the sound this thread started
#[cfg(windows)]
fn stop_sound() {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_NODEFAULT};
    use windows::core::PCWSTR;
    unsafe {
        let _ = PlaySoundW(PCWSTR::null(), None, SND_NODEFAULT);
    }
}

#[cfg(not(windows))]
fn play(_wav: &[u8]) -> bool {
    false
}
#[cfg(not(windows))]
fn stop_sound() {}

#[tauri::command]
pub fn speech_status() -> StatusView {
    status()
}

#[tauri::command]
pub fn speech_say(text: String, voice: String) -> Result<u64, String> {
    say(&text, &voice, "Testing page")
}

#[tauri::command]
pub fn speech_stop() {
    stop();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_has_a_44_byte_header_and_16_bit_samples() {
        let w = wav(&[0.0, 1.0, -1.0, 2.0], 24000);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 24000);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8);
        assert_eq!(w.len(), 44 + 8);
        // Out-of-range samples are clipped, not wrapped
        assert_eq!(i16::from_le_bytes(w[46..48].try_into().unwrap()), 32767);
        assert_eq!(i16::from_le_bytes(w[50..52].try_into().unwrap()), 32767);
    }

    #[test]
    fn voices_come_from_the_list_and_stay_in_the_folder() {
        let dir = std::env::temp_dir().join(format!("md-speech-test-{}", crate::storage::now_ms()));
        fs::create_dir_all(dir.join("model")).unwrap();
        fs::create_dir_all(dir.join("voices")).unwrap();
        fs::write(dir.join("model/default-voice.safetensors"), b"x").unwrap();
        fs::write(dir.join("voices/AbC12.safetensors"), b"x").unwrap();
        fs::write(
            dir.join("voices/voices.jsonl"),
            [
                r#"{"filename":"voices/AbC12.safetensors","name":"Alfie","voice_id":"AbC12","language":"en","locale":"en-gb","gender":"male"}"#,
                r#"{"filename":"voices/Missing.safetensors","name":"Gone","voice_id":"Missing","language":"en"}"#,
                r#"{"filename":"voices/../x.safetensors","name":"Out","voice_id":"Out","language":"en"}"#,
                r#"{"filename":"voices/AbC12.safetensors","name":"Anton","voice_id":"De1","language":"de"}"#,
                "not json",
            ]
            .join("\n"),
        )
        .unwrap();
        let v = voices_in(&dir);
        assert_eq!(v.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["default", "AbC12"]);
        assert_eq!(pick_voice(&v, "alfie"), "AbC12");
        assert_eq!(pick_voice(&v, "AbC12"), "AbC12");
        assert_eq!(pick_voice(&v, "nobody"), "default");
        assert_eq!(pick_voice(&v, ""), "default");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Windows takes the WAV and an audio device plays it: a tenth of a second of silence, so
    /// nothing is heard. Needs a sound device: cargo test --release speech -- --ignored
    #[test]
    #[ignore]
    fn windows_plays_the_wav() {
        let sound = wav(&vec![0.0; 24000], 24000);
        assert!(play(&sound));
        // Stopping returns at once, not after the second of sound
        let t = Instant::now();
        stop_sound();
        assert!(t.elapsed() < Duration::from_millis(300), "stopping took {:?}", t.elapsed());
    }

    #[test]
    fn stop_wakes_a_playing_line_at_once() {
        let e = Arc::new((AtomicU64::new(0), Condvar::new(), Mutex::new(())));
        // The worker's wait, without an AppHandle: the same loop as Engine::stopped_within
        let waiter = {
            let e = e.clone();
            std::thread::spawn(move || {
                let end = Instant::now() + Duration::from_secs(5);
                let mut g = lock(&e.2);
                while e.0.load(Ordering::Relaxed) == 0 && Instant::now() < end {
                    g = e.1.wait_timeout(g, end - Instant::now()).map(|r| r.0).unwrap_or_else(|x| x.into_inner().0);
                }
                Instant::now()
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        let t = Instant::now();
        {
            let _g = lock(&e.2);
            e.0.fetch_add(1, Ordering::Relaxed);
            e.1.notify_all();
        }
        let woke = waiter.join().unwrap();
        assert!(woke.duration_since(t) < Duration::from_millis(200));
    }

    /// The real model: set PHONON_DIR to a folder laid out like the plugin's data folder
    /// (model\ and voices\), then: cargo test --release speech -- --ignored
    #[test]
    #[ignore]
    fn the_real_model_speaks() {
        let dir = PathBuf::from(std::env::var("PHONON_DIR").expect("PHONON_DIR"));
        let voices = voices_in(&dir);
        assert!(!voices.is_empty());
        let t = Instant::now();
        let synth = load(&dir, &voices).unwrap();
        println!("loaded {} voices in {:?}", voices.len(), t.elapsed());
        for line in ["100 knots.", "V1. Rotate.", "Climb to FL350.", "QNH 1013."] {
            let t = Instant::now();
            let pcm = synth.say_with(line, &SpeechOptions::default().voice(voices[voices.len() - 1].id.clone())).unwrap();
            println!("{line}: {:?} for {:.2} s of audio", t.elapsed(), pcm.len() as f64 / synth.sample_rate() as f64);
            assert_eq!(synth.sample_rate(), 24000);
            assert!(pcm.len() > 24000 / 2, "under half a second of audio for {line}");
        }
    }
}
