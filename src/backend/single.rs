//! One copy at a time. Each copy of the app runs its own plugins (the case lights over USB, a
//! Spotify login...), so a second launch must not start a second app: it hands what it was asked to
//! open (a .mdplugin, a .mdprofile, a minniondeck:// link) or just "show the window" to the copy
//! that is running, and quits.
//!
//! The running copy is found by its tray's hidden window, and told with a WM_COPYDATA message.
//! That window's title names the copy's data folder, so a launch only hands over to a copy that
//! uses the same folder. A copy from another folder (a second install, or the developer's build)
//! is never handed someone else's launch: the launch says which folder is open and stops.
#![allow(dead_code)]

/// What starting up found.
#[derive(Debug, PartialEq)]
pub enum Claim {
    /// Nobody else is running: this is the app
    First,
    /// Another copy was told and this one should quit
    Handed,
    /// A copy that uses another data folder is running (its folder): this one can't start
    Elsewhere(String),
}

/// The tray window's title: it names the data folder the copy uses.
pub fn tray_title(root: &std::path::Path) -> String {
    format!("MinnionDeck tray|{}", root.display().to_string().to_lowercase())
}

fn my_title() -> String {
    tray_title(&crate::backend::paths::Paths::find().root)
}

const MUTEX: &str = "Local\\Minniondeck.Rust.SingleInstance";
/// Tells a hand-over from any other WM_COPYDATA
pub const MAGIC: usize = 0x4D44_0001;

/// The word handed to a running copy to ask it to close (the installer and uninstaller use it).
pub const QUIT: &str = "--quit";

/// What starts a command handed to a running copy (see `Command`). Its lines are split by "\n".
pub const CMD: &str = "--md-cmd:";

/// The installer's helpers (--install-plugin, --add-profile, --add-profile-file). When the app is
/// already running they are handed to it, so the running copy does them on its own library and
/// plugins (a helper writing them behind its back would be saved over by the running copy).
#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    InstallPlugin(std::path::PathBuf),
    /// A profile file, and the pages picked if it has choices
    AddProfileFile(std::path::PathBuf, Option<Vec<String>>),
    /// A plugin's ready-made profile from the marketplace
    AddProfile(String, Option<Vec<String>>),
}

impl Command {
    pub fn encode(&self) -> String {
        let picked = |p: &Option<Vec<String>>| p.as_ref().map(|p| p.join(",")).unwrap_or_else(|| "-".into());
        match self {
            Command::InstallPlugin(f) => format!("{CMD}install-plugin\n{}", f.display()),
            Command::AddProfileFile(f, p) => format!("{CMD}add-profile-file\n{}\n{}", f.display(), picked(p)),
            Command::AddProfile(id, p) => format!("{CMD}add-profile\n{id}\n{}", picked(p)),
        }
    }

    pub fn decode(text: &str) -> Option<Command> {
        let mut lines = text.strip_prefix(CMD)?.split('\n');
        let (what, arg) = (lines.next()?, lines.next().filter(|a| !a.is_empty())?);
        let picked = lines.next().filter(|p| *p != "-").map(|p| p.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect());
        match what {
            "install-plugin" => Some(Command::InstallPlugin(arg.into())),
            "add-profile-file" => Some(Command::AddProfileFile(arg.into(), picked)),
            "add-profile" => Some(Command::AddProfile(arg.to_string(), picked)),
            _ => None,
        }
    }
}

/// Whether a copy using this data folder is running now (without claiming anything).
pub fn running_here() -> bool {
    imp::running(&my_title())
}

/// Hands a command to the copy that is running. True when it was taken.
pub fn hand(cmd: &Command) -> bool {
    imp::hand_over(&my_title(), &[cmd.encode()], 4)
}

/// Tells a running copy to save and close. Returns false when none is running.
pub fn ask_to_quit() -> bool {
    imp::hand_over(&my_title(), &[QUIT.to_string()], 2)
}

/// Claims the app for this process, or hands `args` to the copy that already has it. `silent`: a
/// launch from the sign-in entry (--tray) when the app is already running does nothing at all.
pub fn claim(args: &[String], silent: bool) -> Claim {
    claim_named(MUTEX, args, silent)
}

pub fn claim_named(name: &str, args: &[String], silent: bool) -> Claim {
    imp::claim(name, &my_title(), args, silent)
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::time::Duration;

    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, WPARAM};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowTextW, HWND_MESSAGE, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA};
    use windows::core::{HSTRING, w};

    use super::{Claim, MAGIC};

    pub fn claim(name: &str, title: &str, args: &[String], silent: bool) -> Claim {
        let handle = unsafe { CreateMutexW(None, true, &HSTRING::from(name)) };
        let Ok(handle) = handle else { return Claim::First };
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            // held for as long as this process runs
            let _ = handle;
            return Claim::First;
        }
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
        // which copy is running: this folder's, or another one's
        for _ in 0..20 {
            if find(Some(title)).is_some() {
                if silent {
                    return Claim::Handed;
                }
                return if hand_over(title, args, 4) { Claim::Handed } else { Claim::First };
            }
            if let Some(h) = find(None) {
                let mut buf = [0u16; 600];
                let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
                let other = String::from_utf16_lossy(&buf[..n]);
                // an older copy's window doesn't name its folder
                return Claim::Elsewhere(other.strip_prefix("MinnionDeck tray|").unwrap_or("(an older MinnionDeck)").to_string());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        // the other copy is still starting, or has just quit: this one starts
        Claim::First
    }

    pub fn running(title: &str) -> bool {
        find(Some(title)).is_some()
    }

    /// The tray window of the copy with this title (or of any copy, with None).
    fn find(title: Option<&str>) -> Option<HWND> {
        let t = title.map(HSTRING::from);
        let hwnd = match &t {
            Some(t) => unsafe { FindWindowExW(Some(HWND_MESSAGE), None, w!("MinniondeckTray"), t) },
            None => unsafe { FindWindowExW(Some(HWND_MESSAGE), None, w!("MinniondeckTray"), None) },
        };
        hwnd.ok().filter(|h| !h.0.is_null())
    }

    /// Tells the running copy; its tray window may not be up yet if it is just starting.
    pub fn hand_over(title: &str, args: &[String], tries: u32) -> bool {
        for _ in 0..tries {
            if let Some(h) = find(Some(title)) {
                // one message for each thing to open, or one empty one: show the window
                let list: Vec<String> = if args.is_empty() { vec![String::new()] } else { args.to_vec() };
                return list.iter().all(|a| send(h, a));
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        false
    }

    fn send(hwnd: HWND, text: &str) -> bool {
        let bytes = text.as_bytes();
        let cds = COPYDATASTRUCT { dwData: MAGIC, cbData: bytes.len() as u32, lpData: bytes.as_ptr() as *mut c_void };
        let mut result = 0usize;
        let sent = unsafe { SendMessageTimeoutW(hwnd, WM_COPYDATA, WPARAM(0), LPARAM(&cds as *const _ as isize), SMTO_ABORTIFHUNG, 3000, Some(&mut result)) };
        sent.0 != 0
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Claim;
    pub fn claim(_: &str, _: &str, _: &[String], _: bool) -> Claim {
        Claim::First
    }
    pub fn hand_over(_: &str, _: &[String], _: u32) -> bool {
        false
    }
    pub fn running(_: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn commands_go_to_the_running_copy_and_come_back_the_same() {
        let all = [
            Command::InstallPlugin(r"C:\Setup\plugins\Timer.mdplugin".into()),
            Command::AddProfileFile(r"C:\Setup\Streaming.mdprofile".into(), Some(vec!["obs".into(), "twitch".into()])),
            Command::AddProfileFile(r"C:\x.mdprofile".into(), None),
            Command::AddProfile("minnion_spotify".into(), None),
        ];
        for c in all {
            assert_eq!(Command::decode(&c.encode()), Some(c));
        }
        // other things handed over are not commands
        assert_eq!(Command::decode(r"C:\Downloads\Timer.mdplugin"), None);
        assert_eq!(Command::decode("--md-cmd:format-disk\nC:"), None);
        assert_eq!(Command::decode("--md-cmd:install-plugin\n"), None);
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::backend::tray::{self, TrayMsg};

    #[test]
    fn a_second_launch_hands_what_it_opens_to_the_first_and_quits() {
        let name = format!("Local\\Minniondeck.Test.{}", crate::backend::util::now_ms());
        // nobody has it yet
        assert_eq!(claim_named(&name, &[], false), Claim::First);
        // the running copy's tray window listens
        let (tx, rx) = std::sync::mpsc::channel();
        tray::start(tx, || {});
        std::thread::sleep(std::time::Duration::from_millis(800));
        // a second launch with two things to open
        let args = vec![r"C:\Downloads\Timer.mdplugin".to_string(), "minniondeck://install?id=minnion_timer".to_string()];
        assert_eq!(claim_named(&name, &args, false), Claim::Handed);
        let mut got = vec![];
        while let Ok(m) = rx.recv_timeout(std::time::Duration::from_secs(2)) {
            if let TrayMsg::Open(s) = m {
                got.push(s);
            }
            if got.len() == 2 {
                break;
            }
        }
        assert_eq!(got, args);
        // one with nothing to open just asks for the window
        assert_eq!(claim_named(&name, &[], false), Claim::Handed);
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(2)).ok().map(|m| matches!(m, TrayMsg::Open(s) if s.is_empty())), Some(true));
        // the sign-in entry's silent launch changes nothing
        assert_eq!(claim_named(&name, &[], true), Claim::Handed);
        assert!(rx.recv_timeout(std::time::Duration::from_millis(500)).is_err());
        tray::remove();
    }
}
