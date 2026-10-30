// What a button does when it's pressed, on Windows.
//
// key    press keys together, e.g. "Ctrl+Shift+E" (names as the editor records them)
// text   type text into whatever is in front
// click  a left, right or middle mouse click where the pointer is
// run    open a program, folder or file, with optional arguments
// url    open a web address in the default browser
// vol    turn the volume up or down by a percentage
// mic    mute or unmute the default microphone
// delay  wait, between steps of a sequence
//
// Key presses and typing go to the window in front, like a keyboard would. Windows doesn't
// let an app type into windows running as administrator, or press Ctrl+Alt+Delete.
// Plugin actions ("plug:<id>:<action>") are run by plugins.rs.

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Clone, Debug)]
pub struct Step {
    pub kind: String,
    #[serde(default)]
    pub fields: Value,
}

fn field<'a>(s: &'a Step, key: &str) -> &'a str {
    s.fields.get(key).and_then(|v| v.as_str()).unwrap_or("").trim()
}

/// A key: its Windows virtual-key code, and whether it is an "extended" key
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub vk: u16,
    pub extended: bool,
}

const fn k(vk: u16) -> Key {
    Key { vk, extended: false }
}
const fn ext(vk: u16) -> Key {
    Key { vk, extended: true }
}

pub const CTRL: u16 = 0x11;
pub const SHIFT: u16 = 0x10;
pub const ALT: u16 = 0x12;
pub const WIN: u16 = 0x5B;

fn key_named(name: &str) -> Option<Key> {
    let lower = name.to_ascii_lowercase();
    let key = match lower.as_str() {
        "ctrl" | "control" => k(CTRL),
        "shift" => k(SHIFT),
        "alt" | "option" => k(ALT),
        "win" | "windows" | "meta" | "cmd" | "super" => ext(WIN),
        "enter" | "return" => k(0x0D),
        "escape" | "esc" => k(0x1B),
        "tab" => k(0x09),
        "space" | "spacebar" => k(0x20),
        "backspace" => k(0x08),
        "delete" | "del" => ext(0x2E),
        "insert" | "ins" => ext(0x2D),
        "home" => ext(0x24),
        "end" => ext(0x23),
        "pageup" | "pgup" => ext(0x21),
        "pagedown" | "pgdn" => ext(0x22),
        "arrowup" | "up" => ext(0x26),
        "arrowdown" | "down" => ext(0x28),
        "arrowleft" | "left" => ext(0x25),
        "arrowright" | "right" => ext(0x27),
        "printscreen" | "prtsc" => ext(0x2C),
        "capslock" => k(0x14),
        "contextmenu" | "menu" => ext(0x5D),
        "audiovolumeup" | "volumeup" => ext(0xAF),
        "audiovolumedown" | "volumedown" => ext(0xAE),
        "audiovolumemute" | "volumemute" | "mute" => ext(0xAD),
        "mediaplaypause" | "playpause" => ext(0xB3),
        "mediatracknext" | "nexttrack" => ext(0xB0),
        "mediatrackprevious" | "previoustrack" => ext(0xB1),
        "mediastop" => ext(0xB2),
        _ => {
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => return char_key(c),
                _ => {
                    // F1 to F24
                    let n: u16 = lower.strip_prefix('f')?.parse().ok()?;
                    if (1..=24).contains(&n) { k(0x70 + n - 1) } else { return None }
                }
            }
        }
    };
    Some(key)
}

/// A single character. With Shift held the editor records the shifted symbol ("!" for 1),
/// so those map back to the key they are on (US layout).
fn char_key(c: char) -> Option<Key> {
    let c = c.to_ascii_uppercase();
    let vk = match c {
        'A'..='Z' | '0'..='9' => c as u16,
        '!' => b'1' as u16, '@' => b'2' as u16, '#' => b'3' as u16, '$' => b'4' as u16, '%' => b'5' as u16,
        '^' => b'6' as u16, '&' => b'7' as u16, '*' => b'8' as u16, '(' => b'9' as u16, ')' => b'0' as u16,
        '-' | '_' => 0xBD,
        '=' | '+' => 0xBB,
        ',' | '<' => 0xBC,
        '.' | '>' => 0xBE,
        '/' | '?' => 0xBF,
        ';' | ':' => 0xBA,
        '\'' | '"' => 0xDE,
        '[' | '{' => 0xDB,
        ']' | '}' => 0xDD,
        '\\' | '|' => 0xDC,
        '`' | '~' => 0xC0,
        _ => return None,
    };
    Some(k(vk))
}

/// "Ctrl+Shift+E" → the keys to hold, in order. "Ctrl++" means Ctrl and the plus key.
pub fn parse_keys(text: &str) -> Result<Vec<Key>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("No keys are set. Open the editor and press the keys.".into());
    }
    let (body, plus) = match text.strip_suffix("++") { Some(rest) => (rest, true), None => (text, false) };
    let mut keys = Vec::new();
    for part in body.split('+').map(str::trim) {
        if part.is_empty() {
            if plus && body.is_empty() { continue; }
            return Err(format!("\"{text}\" is not a key combination. Write it like Ctrl+Shift+E."));
        }
        keys.push(key_named(part).ok_or_else(|| format!("\"{part}\" is not a key Minniondeck knows."))?);
    }
    if plus {
        keys.push(k(0xBB));
    }
    let modifier = |key: &Key| matches!(key.vk, CTRL | SHIFT | ALT | WIN);
    if keys.iter().all(modifier) {
        return Err("Add a key after Ctrl, Alt, Shift or Win.".into());
    }
    if keys.len() > 6 {
        return Err("That is too many keys at once.".into());
    }
    let has = |vk| keys.iter().any(|key| key.vk == vk);
    if has(CTRL) && has(ALT) && has(0x2E) {
        return Err("Windows doesn't let apps press Ctrl+Alt+Delete.".into());
    }
    Ok(keys)
}

/// Win+L can't be pressed by an app, so it locks the PC directly
fn is_lock(keys: &[Key]) -> bool {
    keys.len() == 2 && keys[0].vk == WIN && keys[1].vk == b'L' as u16
}

/// Volume keys move Windows' volume by 2% each
pub fn volume_presses(amount: &str) -> u32 {
    let pct: f64 = amount.trim().trim_end_matches('%').parse().unwrap_or(10.0);
    ((pct.clamp(1.0, 100.0) / 2.0).round() as u32).max(1)
}

pub fn check_url(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) || url.len() > 2000 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Only web addresses that start with http:// or https:// can be opened.".into());
    }
    Ok(())
}

/// Open a web page in the default browser (the website's pages, see market.rs)
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    check_url(url)?;
    win::open(url, "")
}

fn run_step(s: &Step, button: &crate::plugins::ButtonCtx) -> Result<String, String> {
    match s.kind.as_str() {
        "key" => {
            let keys = parse_keys(field(s, "keys"))?;
            if is_lock(&keys) {
                win::lock()?;
                return Ok("Locked the PC".into());
            }
            win::press(&keys)?;
            Ok(format!("Pressed {}", field(s, "keys")))
        }
        "text" => {
            let text = field(s, "text");
            if text.is_empty() {
                return Err("There is no text to type.".into());
            }
            win::type_text(&text.chars().take(500).collect::<String>())?;
            Ok("Typed the text".into())
        }
        "click" => {
            let button = match field(s, "button").to_ascii_lowercase().as_str() { "right" => 1, "middle" => 2, _ => 0 };
            win::click(button)?;
            Ok(format!("{} click", ["Left", "Right", "Middle"][button]))
        }
        "run" => {
            let path = field(s, "path");
            if path.is_empty() {
                return Err("No program is set.".into());
            }
            win::open(path, field(s, "args"))?;
            Ok(format!("Opened {}", path.rsplit(['\\', '/']).next().unwrap_or(path)))
        }
        "url" => {
            let url = field(s, "url");
            check_url(url)?;
            win::open(url, "")?;
            Ok(format!("Opened {url}"))
        }
        "vol" => {
            let up = !field(s, "direction").eq_ignore_ascii_case("down");
            let n = volume_presses(field(s, "amount"));
            win::press_times(ext(if up { 0xAF } else { 0xAE }), n)?;
            Ok(format!("Volume {} {}%", if up { "up" } else { "down" }, n * 2))
        }
        "mic" => Ok(if win::toggle_mic()? { "Microphone muted".into() } else { "Microphone on".into() }),
        "delay" => {
            let ms: u64 = field(s, "ms").parse().unwrap_or(500).min(60_000);
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Ok(format!("Waited {ms} ms"))
        }
        kind if kind.starts_with("plug:") => crate::plugins::press(kind, &s.fields, button),
        kind => Err(format!("\"{kind}\" is not an action Minniondeck knows.")),
    }
}

/// Run a button's steps in order. A sequence stops at the first step that fails.
pub fn run_steps(steps: &[Step], button: &crate::plugins::ButtonCtx) -> Result<String, String> {
    match steps {
        [] => Err("This button has nothing to do yet.".into()),
        [one] => run_step(one, button),
        many => {
            for (i, s) in many.iter().enumerate() {
                run_step(s, button).map_err(|e| format!("Step {}: {e}", i + 1))?;
            }
            Ok(format!("Ran {} steps", many.len()))
        }
    }
}

/// A button was pressed (in the inspector or on a phone): run it off the main thread
#[tauri::command]
pub async fn run_button(steps: Vec<Step>, button: Option<crate::plugins::ButtonCtx>) -> Result<String, String> {
    let button = button.unwrap_or_default();
    let result = tauri::async_runtime::spawn_blocking(move || run_steps(&steps, &button)).await.map_err(|e| e.to_string())?;
    if let Err(e) = &result {
        crate::applog::line(&format!("A button failed: {e}"));
    }
    result
}

/// Buttons from an imported profile can open programs and press keys. The first time one of
/// them runs, ask on the PC, in a Windows dialog a web page can't fake or click for you.
#[tauri::command]
pub async fn confirm_imported(app: tauri::AppHandle, name: String) -> bool {
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
    let name: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
    app.dialog()
        .message(format!(
            "The buttons in \"{name}\" came from an imported file. They can open programs and websites, press keys and type on this PC.\n\nOnly allow this if you trust where the file came from."
        ))
        .title("Allow imported buttons?")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom("Allow".into(), "Not now".into()))
        .blocking_show()
}

#[cfg(windows)]
mod win {
    use std::mem::size_of;

    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole};
    use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::System::Shutdown::LockWorkStation;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT, SendInput, VIRTUAL_KEY,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    use super::Key;

    fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } } }
    }

    fn send(inputs: &[INPUT]) -> Result<(), String> {
        let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return Err("Windows blocked the key presses. The window in front may be running as administrator.".into());
        }
        Ok(())
    }

    fn flags(key: &Key, up: bool) -> KEYBD_EVENT_FLAGS {
        let mut f = KEYBD_EVENT_FLAGS(0);
        if key.extended { f |= KEYEVENTF_EXTENDEDKEY; }
        if up { f |= KEYEVENTF_KEYUP; }
        f
    }

    /// Hold every key in order, then let go in reverse, like pressing them together
    pub fn press(keys: &[Key]) -> Result<(), String> {
        let mut inputs: Vec<INPUT> = keys.iter().map(|k| key_input(k.vk, 0, flags(k, false))).collect();
        inputs.extend(keys.iter().rev().map(|k| key_input(k.vk, 0, flags(k, true))));
        send(&inputs)
    }

    pub fn press_times(key: Key, times: u32) -> Result<(), String> {
        let inputs: Vec<INPUT> = (0..times).flat_map(|_| [key_input(key.vk, 0, flags(&key, false)), key_input(key.vk, 0, flags(&key, true))]).collect();
        send(&inputs)
    }

    pub fn type_text(text: &str) -> Result<(), String> {
        let mut inputs = Vec::new();
        for unit in text.replace("\r\n", "\n").encode_utf16() {
            if unit == '\n' as u16 {
                let enter = Key { vk: 0x0D, extended: false };
                inputs.push(key_input(enter.vk, 0, flags(&enter, false)));
                inputs.push(key_input(enter.vk, 0, flags(&enter, true)));
            } else {
                inputs.push(key_input(0, unit, KEYEVENTF_UNICODE));
                inputs.push(key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        }
        send(&inputs)
    }

    pub fn click(button: usize) -> Result<(), String> {
        let (down, up) = match button {
            1 => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
            2 => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
            _ => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        };
        let mouse = |f: MOUSE_EVENT_FLAGS| INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: f, time: 0, dwExtraInfo: 0 } } };
        send(&[mouse(down), mouse(up)])
    }

    /// Open a program, folder, file or web address the way Explorer would
    pub fn open(target: &str, args: &str) -> Result<(), String> {
        let file = HSTRING::from(target);
        let params = HSTRING::from(args);
        let result = unsafe { ShellExecuteW(None, w!("open"), &file, if args.is_empty() { PCWSTR::null() } else { PCWSTR(params.as_ptr()) }, PCWSTR::null(), SW_SHOWNORMAL) };
        if result.0 as isize <= 32 {
            return Err(format!("Windows could not open {target}."));
        }
        Ok(())
    }

    pub fn lock() -> Result<(), String> {
        unsafe { LockWorkStation() }.map_err(|e| format!("Could not lock the PC: {e}"))
    }

    /// Flip the default microphone's mute. Returns true when it is now muted.
    pub fn toggle_mic() -> Result<bool, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let fail = |e: windows::core::Error| format!("Could not reach the microphone: {}", e.message());
            let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(fail)?;
            let mic = devices.GetDefaultAudioEndpoint(eCapture, eConsole).map_err(|_| "There is no microphone.".to_string())?;
            let volume: IAudioEndpointVolume = mic.Activate(CLSCTX_ALL, None).map_err(fail)?;
            let muted = !volume.GetMute().map_err(fail)?.as_bool();
            volume.SetMute(muted, std::ptr::null()).map_err(fail)?;
            Ok(muted)
        }
    }
}

#[cfg(not(windows))]
mod win {
    use super::Key;
    const NO: &str = "Button actions work on Windows.";
    pub fn press(_: &[Key]) -> Result<(), String> { Err(NO.into()) }
    pub fn press_times(_: Key, _: u32) -> Result<(), String> { Err(NO.into()) }
    pub fn type_text(_: &str) -> Result<(), String> { Err(NO.into()) }
    pub fn click(_: usize) -> Result<(), String> { Err(NO.into()) }
    pub fn open(_: &str, _: &str) -> Result<(), String> { Err(NO.into()) }
    pub fn lock() -> Result<(), String> { Err(NO.into()) }
    pub fn toggle_mic() -> Result<bool, String> { Err(NO.into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vks(text: &str) -> Vec<u16> {
        parse_keys(text).unwrap().iter().map(|k| k.vk).collect()
    }

    #[test]
    fn key_combinations_read_like_the_editor_writes_them() {
        assert_eq!(vks("Ctrl+Shift+E"), [CTRL, SHIFT, b'E' as u16]);
        assert_eq!(vks("Win+Shift+S"), [WIN, SHIFT, b'S' as u16]);
        assert_eq!(vks("Alt+F4"), [ALT, 0x73]);
        assert_eq!(vks("ArrowUp"), [0x26]);
        assert!(parse_keys("ArrowUp").unwrap()[0].extended);
        assert_eq!(vks("Ctrl+Shift+!"), [CTRL, SHIFT, b'1' as u16]);
        assert_eq!(vks("Ctrl++"), [CTRL, 0xBB]);
        assert_eq!(vks("Space"), [0x20]);
        assert!(is_lock(&parse_keys("Win+L").unwrap()));
    }

    #[test]
    fn bad_key_combinations_are_explained() {
        assert!(parse_keys("").unwrap_err().contains("No keys"));
        assert!(parse_keys("Ctrl+").unwrap_err().contains("not a key combination"));
        assert!(parse_keys("Ctrl+Shift").unwrap_err().contains("Add a key"));
        assert!(parse_keys("Ctrl+Alt+Delete").unwrap_err().contains("Ctrl+Alt+Delete"));
        assert!(parse_keys("Hyper+K").unwrap_err().contains("Hyper"));
    }

    #[test]
    fn volume_moves_in_two_percent_steps() {
        assert_eq!(volume_presses("10"), 5);
        assert_eq!(volume_presses("5"), 3);
        assert_eq!(volume_presses("1"), 1);
        assert_eq!(volume_presses("100"), 50);
        assert_eq!(volume_presses(""), 5);
    }

    #[test]
    fn only_web_addresses_open_as_websites() {
        assert!(check_url("https://example.com").is_ok());
        assert!(check_url("http://meet.example.com/x?y=1").is_ok());
        assert!(check_url("file:///C:/Windows").is_err());
        assert!(check_url("javascript:alert(1)").is_err());
        assert!(check_url("https://a b").is_err());
    }

    #[test]
    fn steps_that_need_a_plugin_or_are_empty_say_so() {
        let step = |kind: &str| Step { kind: kind.into(), fields: serde_json::json!({}) };
        assert!(run_steps(&[], &Default::default()).unwrap_err().contains("nothing to do"));
        assert!(run_steps(&[step("plug:none:x")], &Default::default()).unwrap_err().contains("Plugin"));
        assert!(run_steps(&[step("nope")], &Default::default()).unwrap_err().contains("not an action"));
        assert_eq!(run_steps(&[step("delay")].map(|mut s| { s.fields = serde_json::json!({ "ms": "1" }); s }), &Default::default()).unwrap(), "Waited 1 ms");
        // A sequence stops at the step that fails, and says which
        let err = run_steps(&[Step { kind: "delay".into(), fields: serde_json::json!({ "ms": "0" }) }, step("nope")], &Default::default()).unwrap_err();
        assert!(err.starts_with("Step 2:") && err.contains("not an action"), "{err}");
    }
}
