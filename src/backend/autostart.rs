//! "Start with Windows": Minniondeck starts when this Windows account signs in. Ported from the
//! Minniondeck app's autostart.rs.
//!
//! The entry lives in the current user's Run key, so it needs no administrator rights and only
//! affects this account. It starts Minniondeck with --tray, so the app opens in the tray rather
//! than the window. The entry points at the exe's current location, so it is rewritten on each
//! start while it is on: moving the app and starting it again keeps the sign-in entry working.

pub const TRAY_ARG: &str = "--tray";
#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const VALUE: &str = "Minniondeck";

/// The command line the sign-in entry runs. The path is quoted because it may contain spaces.
pub fn command_line(exe: &str) -> String {
    format!("\"{exe}\" {TRAY_ARG}")
}

/// Whether the sign-in entry is there.
#[cfg(windows)]
pub fn is_on() -> bool {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY).and_then(|k| k.get_value::<String, _>(VALUE)).is_ok()
}

#[cfg(not(windows))]
pub fn is_on() -> bool {
    false
}

#[cfg(windows)]
pub fn set(enabled: bool) -> Result<(), String> {
    use std::io::ErrorKind;
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};

    let run = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE).map_err(|e| format!("Could not reach Windows sign-in settings: {e}"))?;
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        run.set_value(VALUE, &command_line(&exe.display().to_string())).map_err(|e| format!("Could not turn on Start with Windows: {e}"))
    } else {
        match run.delete_value(VALUE) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("Could not turn off Start with Windows: {e}")),
        }
    }
}

#[cfg(not(windows))]
pub fn set(_enabled: bool) -> Result<(), String> {
    Err("Start with Windows is only available on Windows.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sign_in_entry_starts_in_the_tray_from_any_path() {
        assert_eq!(command_line(r"C:\Program Files\Minniondeck\minniondeck.exe"), r#""C:\Program Files\Minniondeck\minniondeck.exe" --tray"#);
        assert_eq!(command_line(r"E:\apps\Minniondeck.exe"), r#""E:\apps\Minniondeck.exe" --tray"#);
    }
}
