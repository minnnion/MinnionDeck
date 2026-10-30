// Which app is in front, for Settings > Profiles and pages > Switch profiles automatically.
//
// Once a second, the program behind the foreground window is looked up. When it changes, the
// page hears "app_in_front" with its exe name and whether it fills its screen (for "Any
// full-screen game"); the page decides whether to switch profiles. Minniondeck itself is
// ignored, so using the app never switches profiles.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct Front {
    pub exe: String,
    pub fullscreen: bool,
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last: Option<Front> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = current();
            if now.is_some() && now != last {
                let _ = app.emit("app_in_front", now.clone());
                last = now;
            }
        }
    });
}

#[cfg(windows)]
fn current() -> Option<Front> {
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, RECT};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId};
    use windows::core::PWSTR;

    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let named = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        named.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let exe = path.rsplit('\\').next()?.to_ascii_lowercase();

        // Full screen: the window covers its whole monitor. The desktop does too, so it doesn't count.
        let mut rect = RECT::default();
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let covers = GetWindowRect(window, &mut rect).is_ok()
            && GetMonitorInfoW(MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST), &mut info).as_bool()
            && rect.left <= info.rcMonitor.left
            && rect.top <= info.rcMonitor.top
            && rect.right >= info.rcMonitor.right
            && rect.bottom >= info.rcMonitor.bottom;
        Some(Front { fullscreen: covers && exe != "explorer.exe", exe })
    }
}

#[cfg(not(windows))]
fn current() -> Option<Front> {
    None
}
