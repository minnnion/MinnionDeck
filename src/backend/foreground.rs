//! Which app is in front, for Settings > Profiles and pages > Switch profiles automatically.
//! Ported from the Minniondeck app's foreground.rs and its rules (12-buttons.js).
//!
//! Once a second the program behind the foreground window is looked up. When it changes, the
//! window hears about it with its exe name and whether it fills its screen (for "Any full-screen
//! game"). This app itself is ignored, so using it never switches profiles.
use std::sync::mpsc::Sender;
use std::time::Duration;

#[derive(Clone, PartialEq, Debug)]
pub struct Front {
    pub exe: String,
    pub fullscreen: bool,
}

/// Starts the watcher thread. `repaint` wakes the window when something changed.
pub fn start(tx: Sender<Front>, repaint: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let mut last: Option<Front> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = current();
            if now.is_some() && now != last {
                if tx.send(now.clone().unwrap_or(Front { exe: String::new(), fullscreen: false })).is_err() {
                    return;
                }
                repaint();
                last = now;
            }
        }
    });
}

/// The programs each choice on a profile means (its "auto" name in library.json).
const APP_EXES: &[(&str, &[&str])] = &[
    ("OBS Studio", &["obs64.exe", "obs32.exe", "obs.exe"]),
    ("Microsoft Teams", &["ms-teams.exe", "teams.exe"]),
    ("Steam", &["steam.exe", "steamwebhelper.exe"]),
    ("Spotify", &["spotify.exe"]),
    ("Visual Studio Code", &["code.exe"]),
];

fn known(exe: &str) -> bool {
    APP_EXES.iter().any(|(_, exes)| exes.contains(&exe))
}

/// Whether a profile set to `auto` ("Never", an app's name or "Any full-screen game") wants the
/// app that is in front.
pub fn wants(auto: &str, front: &Front) -> bool {
    match auto {
        "" | "Never" => false,
        "Any full-screen game" => front.fullscreen && !known(&front.exe),
        name => APP_EXES.iter().find(|(n, _)| *n == name).is_some_and(|(_, exes)| exes.contains(&front.exe.as_str())),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn front(exe: &str, fullscreen: bool) -> Front {
        Front { exe: exe.into(), fullscreen }
    }

    #[test]
    fn a_profile_wants_its_apps() {
        assert!(wants("OBS Studio", &front("obs64.exe", false)));
        assert!(wants("Steam", &front("steamwebhelper.exe", false)));
        assert!(wants("Visual Studio Code", &front("code.exe", false)));
        assert!(!wants("Spotify", &front("code.exe", false)));
        assert!(!wants("Never", &front("obs64.exe", false)));
        assert!(!wants("", &front("obs64.exe", false)));
    }

    #[test]
    fn any_fullscreen_game_ignores_the_known_apps() {
        assert!(wants("Any full-screen game", &front("fenix.exe", true)));
        assert!(!wants("Any full-screen game", &front("fenix.exe", false)));
        // OBS or Steam in full screen is not "a game"
        assert!(!wants("Any full-screen game", &front("obs64.exe", true)));
        assert!(!wants("Any full-screen game", &front("steam.exe", true)));
    }
}
