//! The minnion in the notification area (the tray), ported from the Minniondeck app's tray.rs.
//!
//!   Click            shows the window and brings it to the front; if it's already in front, hides it
//!   Double-click     always shows the window
//!   Hover            "Minniondeck · No phones connected"
//!   Right-click      Open Minniondeck, Quit
//!   Close (x)        with "Keep running" (Settings > General) the window hides and the minnion hops
//!   Quit             saves, then the window and the tray icon go away
//!
//! The icon is drawn from the design's 16 x 16 pixel grid at the size Windows uses for tray icons,
//! with a light outline on a dark taskbar. It runs on its own thread with its own (hidden) message
//! window, and shows and hides the main window with plain Windows calls, so it keeps working while
//! the window is hidden.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

/// What the tray asks of the window.
pub enum TrayMsg {
    /// Quit was chosen in the tray menu: save and close
    Quit,
    /// Another launch handed this copy something to open (a file or a link); empty: just the window
    Open(String),
}

/// Quit was chosen: closing the window now means closing the app, not going to the tray.
pub static QUITTING: AtomicBool = AtomicBool::new(false);

pub fn quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

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

/// Starts the tray icon on its own thread. `repaint` wakes the window up.
pub fn start(tx: Sender<TrayMsg>, repaint: impl Fn() + Send + Sync + 'static) {
    imp::start(tx, Box::new(repaint));
}

/// The minnion hops once (up, a little below, back): the window went to the tray.
pub fn hop() {
    imp::hop();
}

/// Whether Windows has the icon (the tray thread is up and the icon was accepted).
pub fn is_up() -> bool {
    imp::is_up()
}

/// The hover text of the icon: how many phones are connected.
pub fn set_phones(connected: usize) {
    imp::set_tooltip(&tooltip(connected));
}

/// Takes the icon out of the tray (the app is closing).
pub fn remove() {
    imp::remove();
}

/// Shows the window and brings it to the front.
pub fn show_window() {
    imp::show_window();
}

#[cfg(windows)]
mod imp {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::sync::mpsc::Sender;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Shell::{NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW};
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIcon, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW, EnumWindows, GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics, GetWindowTextW,
        GetWindowThreadProcessId, HICON, HMENU, HWND_MESSAGE, IsIconic, IsWindowVisible, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, RegisterClassExW, RegisterWindowMessageW, SM_CXSMICON, SW_HIDE, SW_RESTORE,
        SW_SHOW, SetForegroundWindow, SetTimer, ShowWindow, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
        WM_COPYDATA, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW,
    };
    use windows::core::{PCWSTR, w};

    use super::{TrayMsg, pixels, tooltip};

    const WM_TRAY: u32 = 0x8000 + 1;
    const ID_OPEN: usize = 1;
    const ID_QUIT: usize = 2;
    const TIMER: usize = 1;

    static TX: Mutex<Option<Sender<TrayMsg>>> = Mutex::new(None);
    static REPAINT: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    /// The tray's own message window, and the main window once found
    static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
    static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
    static HOPPING: AtomicBool = AtomicBool::new(false);
    static IN_FRONT: Mutex<Option<Instant>> = Mutex::new(None);
    static DOUBLE: Mutex<Option<Instant>> = Mutex::new(None);
    static TASKBAR_CREATED: AtomicIsize = AtomicIsize::new(0);

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn taskbar_is_dark() -> bool {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
            .and_then(|k| k.get_value::<u32, _>("SystemUsesLightTheme"))
            .map(|light| light == 0)
            .unwrap_or(true)
    }

    /// The size Windows draws tray icons at for this screen's scaling (16 at 100%, 32 at 200%)
    fn icon_size() -> u32 {
        (unsafe { GetSystemMetrics(SM_CXSMICON) } as u32).clamp(16, 64)
    }

    fn make_icon(dark: bool, lift: i32) -> Option<HICON> {
        let size = icon_size();
        let rgba = pixels(size, dark, lift);
        // Windows wants BGRA, and an AND mask (all zero: the alpha decides what shows)
        let mut bgra = rgba.clone();
        for p in bgra.chunks_mut(4) {
            p.swap(0, 2);
        }
        let mask = vec![0u8; (size.div_ceil(16) * 2 * size) as usize];
        unsafe { CreateIcon(None, size as i32, size as i32, 1, 32, mask.as_ptr(), bgra.as_ptr()).ok() }
    }

    fn data(hwnd: HWND, icon: Option<HICON>, tip: Option<&str>) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, uFlags: NIF_MESSAGE, uCallbackMessage: WM_TRAY, ..Default::default() };
        if let Some(i) = icon {
            nid.uFlags |= NIF_ICON;
            nid.hIcon = i;
        }
        if let Some(t) = tip {
            nid.uFlags |= NIF_TIP;
            for (i, u) in t.encode_utf16().take(127).enumerate() {
                nid.szTip[i] = u;
            }
        }
        nid
    }

    fn add_icon(hwnd: HWND) {
        if let Some(icon) = make_icon(taskbar_is_dark(), 0) {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_ADD, &data(hwnd, Some(icon), Some(&tooltip(0))));
            }
        }
    }

    /// The main window: a top-level window of this process called "Minniondeck" (it may be hidden)
    fn main_window() -> Option<HWND> {
        let cached = MAIN_HWND.load(Ordering::SeqCst);
        if cached != 0 {
            return Some(HWND(cached as *mut _));
        }
        unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid == unsafe { GetCurrentProcessId() } {
                let mut buf = [0u16; 64];
                let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
                if String::from_utf16_lossy(&buf[..n]) == "Minniondeck" {
                    MAIN_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
                    unsafe { *(lparam.0 as *mut bool) = true };
                    return false.into();
                }
            }
            true.into()
        }
        let mut found = false;
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut found as *mut bool as isize));
        }
        found.then(|| HWND(MAIN_HWND.load(Ordering::SeqCst) as *mut _))
    }

    pub fn show_window() {
        if let Some(w) = main_window() {
            unsafe {
                if IsIconic(w).as_bool() {
                    let _ = ShowWindow(w, SW_RESTORE);
                } else {
                    let _ = ShowWindow(w, SW_SHOW);
                }
                let _ = SetForegroundWindow(w);
            }
        }
    }

    fn hide_window() {
        if let Some(w) = main_window() {
            unsafe {
                let _ = ShowWindow(w, SW_HIDE);
            }
        }
    }

    fn in_front() -> bool {
        let Some(w) = main_window() else { return false };
        unsafe {
            let fg = GetForegroundWindow() == w;
            // clicking the tray takes focus away first, so a window that had it a moment ago counts
            let just = lock(&IN_FRONT).is_some_and(|t| t.elapsed() < Duration::from_millis(400));
            IsWindowVisible(w).as_bool() && !IsIconic(w).as_bool() && (fg || just)
        }
    }

    fn click() {
        if lock(&DOUBLE).is_some_and(|t| t.elapsed() < Duration::from_millis(600)) {
            return;
        }
        if in_front() {
            hide_window();
        } else {
            show_window();
        }
    }

    fn quit() {
        super::QUITTING.store(true, Ordering::SeqCst);
        if let Some(tx) = lock(&TX).as_ref() {
            let _ = tx.send(TrayMsg::Quit);
        }
        if let Some(r) = REPAINT.get() {
            r();
        }
        // ask the window to close (it saves on the way out); if it can't, leave anyway
        if let Some(w) = main_window() {
            unsafe {
                let _ = PostMessageW(Some(w), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(4));
            remove();
            std::process::exit(0);
        });
    }

    fn menu(hwnd: HWND) {
        unsafe {
            let Ok(m) = CreatePopupMenu() else { return };
            let _ = AppendMenuW(m, MF_STRING, ID_OPEN, w!("Open Minniondeck"));
            let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(m, MF_STRING, ID_QUIT, w!("Quit"));
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            // the menu closes when something else is clicked only if our window is in front
            let _ = SetForegroundWindow(hwnd);
            let id = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, p.x, p.y, Some(0), hwnd, None).0 as usize;
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(m as HMENU);
            match id {
                ID_OPEN => show_window(),
                ID_QUIT => quit(),
                _ => {}
            }
        }
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        match msg {
            WM_TRAY => {
                match lparam.0 as u32 & 0xffff {
                    WM_LBUTTONUP => click(),
                    WM_LBUTTONDBLCLK => {
                        *lock(&DOUBLE) = Some(Instant::now());
                        show_window();
                    }
                    WM_RBUTTONUP => menu(hwnd),
                    _ => {}
                }
                LRESULT(0)
            }
            // a second launch of the app hands over what it was asked to open (backend/single.rs)
            WM_COPYDATA => {
                let cds = lparam.0 as *const COPYDATASTRUCT;
                if !cds.is_null() && unsafe { (*cds).dwData } == crate::backend::single::MAGIC {
                    let (len, ptr) = unsafe { ((*cds).cbData as usize, (*cds).lpData as *const u8) };
                    let text = if len == 0 || ptr.is_null() || len > 4096 { String::new() } else { String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned() };
                    if let Some(tx) = lock(&TX).as_ref() {
                        let _ = tx.send(TrayMsg::Open(text.clone()));
                    }
                    if let Some(r) = REPAINT.get() {
                        r();
                    }
                    // a request to close isn't a reason to put the window up
                    if text != crate::backend::single::QUIT {
                        show_window();
                    }
                }
                LRESULT(1)
            }
            WM_TIMER if wparam.0 == TIMER => {
                // remember when the window last had focus, for the click above
                if let Some(w) = main_window() {
                    if unsafe { GetForegroundWindow() } == w {
                        *lock(&IN_FRONT) = Some(Instant::now());
                    }
                }
                LRESULT(0)
            }
            m if m != 0 && m == TASKBAR_CREATED.load(Ordering::SeqCst) as u32 => {
                // Explorer restarted: the icon has to be put back
                add_icon(hwnd);
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    pub fn start(tx: Sender<TrayMsg>, repaint: Box<dyn Fn() + Send + Sync>) {
        *lock(&TX) = Some(tx);
        let _ = REPAINT.set(repaint);
        std::thread::Builder::new()
            .name("tray".into())
            .spawn(|| unsafe {
                let Ok(module) = GetModuleHandleW(None) else { return };
                let hinst = HINSTANCE(module.0);
                let class = w!("MinniondeckTray");
                let wc = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(proc), hInstance: hinst, lpszClassName: class, ..Default::default() };
                RegisterClassExW(&wc);
                let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("Minniondeck tray"), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(hinst), None) else { return };
                TRAY_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
                TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")) as isize, Ordering::SeqCst);
                add_icon(hwnd);
                SetTimer(Some(hwnd), TIMER, 200, None);
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .ok();
    }

    pub fn hop() {
        let h = TRAY_HWND.load(Ordering::SeqCst);
        if h == 0 || HOPPING.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::spawn(move || {
            let hwnd = HWND(h as *mut _);
            let dark = taskbar_is_dark();
            for lift in [2, -1, 0] {
                if let Some(icon) = make_icon(dark, lift) {
                    unsafe {
                        let _ = Shell_NotifyIconW(NIM_MODIFY, &data(hwnd, Some(icon), None));
                    }
                }
                std::thread::sleep(Duration::from_millis(107));
            }
            HOPPING.store(false, Ordering::SeqCst);
        });
    }

    pub fn set_tooltip(text: &str) {
        let h = TRAY_HWND.load(Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &data(HWND(h as *mut _), None, Some(text)));
            }
        }
    }

    pub fn is_up() -> bool {
        let h = TRAY_HWND.load(Ordering::SeqCst);
        // changing the hover text of an icon that isn't there fails
        h != 0 && unsafe { Shell_NotifyIconW(NIM_MODIFY, &data(HWND(h as *mut _), None, Some(&tooltip(0)))).as_bool() }
    }

    pub fn remove() {
        let h = TRAY_HWND.load(Ordering::SeqCst);
        if h != 0 {
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data(HWND(h as *mut _), None, None));
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::TrayMsg;
    use std::sync::mpsc::Sender;
    pub fn start(_: Sender<TrayMsg>, _: Box<dyn Fn() + Send + Sync>) {}
    pub fn hop() {}
    pub fn is_up() -> bool {
        false
    }
    pub fn set_tooltip(_: &str) {}
    pub fn remove() {}
    pub fn show_window() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Puts the real icon in the tray for a moment: cargo test --release the_tray_icon -- --ignored
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn the_tray_icon_goes_up_and_comes_down() {
        let (tx, _rx) = std::sync::mpsc::channel();
        start(tx, || {});
        std::thread::sleep(std::time::Duration::from_millis(1200));
        assert!(is_up(), "Windows should have the icon");
        hop();
        std::thread::sleep(std::time::Duration::from_millis(600));
        assert!(is_up());
        remove();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!is_up(), "the icon should be gone");
    }

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
        // At 32 px every grid pixel is a crisp 2 x 2 block
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
        assert_eq!(at(&up, 16, 7, 0), [0x1e, 0x24, 0x33, 255]);
        assert_eq!(at(&up, 16, 5, 15), [0, 0, 0, 0]);
    }

    #[test]
    fn hover_text_matches_the_design() {
        assert_eq!(tooltip(0), "Minniondeck · No phones connected");
        assert_eq!(tooltip(1), "Minniondeck · 1 phone connected");
        assert_eq!(tooltip(2), "Minniondeck · 2 phones connected");
    }
}
