// The minnion in the notification area, from the design's Tray section.
//
//   Click            shows the window and brings it to the front; if it's already in front, hides it
//   Double-click     always shows the window
//   Hover            "Minniondeck · 2 phones connected", kept up to date
//   Right-click      Open Minniondeck, Quit
//   Close (✕)        with "Keep running" (Settings > General) the window hides and the minnion hops
//   Quit             saves, then the window and the tray icon go away; phones disconnect
//
// The icon is drawn from the design's 16 × 16 pixel grid at the size Windows uses for tray icons,
// with a light outline on a dark taskbar.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, WindowEvent};

const TRAY_ID: &str = "main";

/// The design's tray icon. o outline, m body, d body shade, w eye white, p pupil, r antenna bulb.
pub const ICON: [&str; 16] = [
    "......rr........", "......rr........", ".......o........", ".....oooooo.....",
    "...oommmmmmoo...", "..ommmmmmmmmmo..", "..omwwmmmmwwmo..", ".ommwpmmmmwpmmo.",
    ".ommmmmmmmmmmmo.", ".ommmmmoommmmdo.", ".ommmmmmmmmmmdo.", "..ommmmmmmmmdo..",
    "...oommmmmmoo...", ".....oooooo.....", ".....oo..oo.....", ".....oo..oo.....",
];

fn colour(c: char, dark_taskbar: bool) -> Option<[u8; 4]> {
    Some(match c {
        'o' if dark_taskbar => [0xe4, 0xea, 0xf6, 255],
        'o' | 'p' => [0x1e, 0x24, 0x33, 255],
        'm' => [0x5f, 0xd0, 0xa8, 255],
        'd' => [0x3a, 0xa5, 0x85, 255],
        'w' => [0xff, 0xff, 0xff, 255],
        'r' => [0xff, 0x5a, 0x5f, 255],
        _ => return None,
    })
}

/// The icon as RGBA pixels, `size` pixels square, moved up by `lift` grid pixels (for the hop)
pub fn pixels(size: u32, dark_taskbar: bool, lift: i32) -> Vec<u8> {
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        let gy = (y * 16 / size) as i32 + lift;
        let Some(row) = usize::try_from(gy).ok().and_then(|gy| ICON.get(gy)) else { continue };
        for x in 0..size {
            let c = row.as_bytes()[(x * 16 / size) as usize] as char;
            if let Some(rgba) = colour(c, dark_taskbar) {
                let i = ((y * size + x) * 4) as usize;
                out[i..i + 4].copy_from_slice(&rgba);
            }
        }
    }
    out
}

pub fn tooltip(connected: usize) -> String {
    match connected {
        0 => "Minniondeck · No phones connected".into(),
        1 => "Minniondeck · 1 phone connected".into(),
        n => format!("Minniondeck · {n} phones connected"),
    }
}

/// What the tray remembers between clicks
#[derive(Default)]
pub struct TrayState {
    /// When the window last lost focus: clicking the tray takes focus away from it, so a window
    /// that had focus a moment ago still counts as "in front"
    blurred: Mutex<Option<Instant>>,
    /// A double-click is followed by one more click; that one must not hide the window again
    double: Mutex<Option<Instant>>,
    dark: AtomicBool,
    phones: AtomicUsize,
    hopping: AtomicBool,
}

#[cfg(windows)]
fn taskbar_is_dark() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
        .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
        .map(|light| light == 0)
        .unwrap_or(true)
}

#[cfg(not(windows))]
fn taskbar_is_dark() -> bool {
    false
}

/// The size Windows draws tray icons at for this screen's scaling (16 at 100%, 32 at 200%)
#[cfg(windows)]
fn icon_size() -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    (unsafe { GetSystemMetrics(SM_CXSMICON) } as u32).clamp(16, 64)
}

#[cfg(not(windows))]
fn icon_size() -> u32 {
    32
}

fn image(dark: bool, lift: i32) -> Image<'static> {
    let size = icon_size();
    Image::new_owned(pixels(size, dark, lift), size, size)
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

pub fn show(app: &AppHandle) {
    if let Some(w) = window(app) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn in_front(app: &AppHandle) -> bool {
    let Some(w) = window(app) else { return false };
    let state = app.state::<TrayState>();
    let just_blurred = state.blurred.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < Duration::from_millis(400));
    w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) && (w.is_focused().unwrap_or(false) || just_blurred)
}

fn click(app: &AppHandle) {
    let state = app.state::<TrayState>();
    if state.double.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < Duration::from_millis(600)) {
        return;
    }
    if in_front(app) {
        if let Some(w) = window(app) {
            let _ = w.hide();
        }
    } else {
        show(app);
    }
}

/// Ask the page to save, then quit. If the page doesn't answer in time, quit anyway.
fn quit(app: &AppHandle) {
    let _ = app.emit("app-quit-requested", ());
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(3));
        handle.exit(0);
    });
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let state = TrayState::default();
    state.dark.store(taskbar_is_dark(), Ordering::SeqCst);
    let dark = state.dark.load(Ordering::SeqCst);
    app.manage(state);

    let open = MenuItem::with_id(app, "open", "Open Minniondeck", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &PredefinedMenuItem::separator(app)?, &quit_item])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(image(dark, 0))
        .tooltip(tooltip(0))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => show(app),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            let app = tray.app_handle();
            match e {
                TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } => click(app),
                TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => {
                    *app.state::<TrayState>().double.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
                    show(app);
                }
                _ => {}
            }
        })
        .build(app)?;

    // Follow the taskbar if it switches between light and dark
    let handle = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let dark = taskbar_is_dark();
        let state = handle.state::<TrayState>();
        if state.dark.swap(dark, Ordering::SeqCst) != dark && !state.hopping.load(Ordering::SeqCst) {
            if let Some(tray) = handle.tray_by_id(TRAY_ID) {
                let _ = tray.set_icon(Some(image(dark, 0)));
            }
        }
    });
    Ok(())
}

/// Keep the hover text in step with how many phones are connected
pub fn set_phones(app: &AppHandle, connected: usize) {
    let Some(state) = app.try_state::<TrayState>() else { return };
    if state.phones.swap(connected, Ordering::SeqCst) != connected || connected == 0 {
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(tooltip(connected)));
        }
    }
}

/// Note when the window loses focus, so a click on the tray right after still counts it as in front
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if let WindowEvent::Focused(false) = event {
        if let Some(state) = window.app_handle().try_state::<TrayState>() {
            *state.blurred.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
    }
}

/// The window went to the tray: the minnion hops once (up, a little below, back)
#[tauri::command]
pub fn tray_hop(app: AppHandle) {
    let state = app.state::<TrayState>();
    if state.hopping.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        let dark = handle.state::<TrayState>().dark.load(Ordering::SeqCst);
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            for lift in [2, -1, 0] {
                let _ = tray.set_icon(Some(image(dark, lift)));
                std::thread::sleep(Duration::from_millis(107));
            }
        }
        handle.state::<TrayState>().hopping.store(false, Ordering::SeqCst);
    });
}

/// Everything is saved: close Minniondeck
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(px: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        px[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn the_icon_is_drawn_from_the_design_grid() {
        let px = pixels(16, false, 0);
        assert_eq!(at(&px, 16, 6, 0), [0xff, 0x5a, 0x5f, 255]); // antenna bulb
        assert_eq!(at(&px, 16, 0, 0), [0, 0, 0, 0]); // see-through around it
        assert_eq!(at(&px, 16, 5, 7), [0x1e, 0x24, 0x33, 255]); // pupil
        // At 32 px every grid pixel is a crisp 2 × 2 block
        let big = pixels(32, false, 0);
        assert_eq!(at(&big, 32, 12, 0), at(&big, 32, 13, 1));
    }

    #[test]
    fn the_outline_turns_light_on_a_dark_taskbar_but_pupils_stay_dark() {
        let dark = pixels(16, true, 0);
        assert_eq!(at(&dark, 16, 7, 2), [0xe4, 0xea, 0xf6, 255]);
        assert_eq!(at(&dark, 16, 5, 7), [0x1e, 0x24, 0x33, 255]);
    }

    #[test]
    fn lifting_moves_the_minnion_up() {
        let up = pixels(16, false, 2);
        // The outline row that was at y = 2 is now at y = 0
        assert_eq!(at(&up, 16, 7, 0), [0x1e, 0x24, 0x33, 255]);
        // The feet left the bottom two rows empty
        assert_eq!(at(&up, 16, 5, 15), [0, 0, 0, 0]);
    }

    #[test]
    fn hover_text_matches_the_design() {
        assert_eq!(tooltip(0), "Minniondeck · No phones connected");
        assert_eq!(tooltip(1), "Minniondeck · 1 phone connected");
        assert_eq!(tooltip(2), "Minniondeck · 2 phones connected");
    }
}
