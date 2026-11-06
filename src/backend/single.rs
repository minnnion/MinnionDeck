//! One copy at a time. Each copy of the app runs its own plugins (the case lights over USB, a
//! Spotify login...), so a second launch must not start a second app: it hands what it was asked to
//! open (a .mdplugin, a .mdprofile, a minniondeck:// link) or just "show the window" to the copy
//! that is running, and quits.
//!
//! The running copy is found by its tray's hidden window, and told with a WM_COPYDATA message.
#![allow(dead_code)]

/// What starting up found.
#[derive(Debug, PartialEq)]
pub enum Claim {
    /// Nobody else is running: this is the app
    First,
    /// Another copy was told and this one should quit
    Handed,
}

const MUTEX: &str = "Local\\Minniondeck.Rust.SingleInstance";
/// Tells a hand-over from any other WM_COPYDATA
pub const MAGIC: usize = 0x4D44_0001;

/// The word handed to a running copy to ask it to close (the installer and uninstaller use it).
pub const QUIT: &str = "--quit";

/// Tells a running copy to save and close. Returns false when none is running.
pub fn ask_to_quit() -> bool {
    imp::hand_over(&[QUIT.to_string()], 2)
}

/// Claims the app for this process, or hands `args` to the copy that already has it. `silent`: a
/// launch from the sign-in entry (--tray) when the app is already running does nothing at all.
pub fn claim(args: &[String], silent: bool) -> Claim {
    claim_named(MUTEX, args, silent)
}

pub fn claim_named(name: &str, args: &[String], silent: bool) -> Claim {
    imp::claim(name, args, silent)
}

#[cfg(windows)]
mod imp {
    use std::ffi::c_void;
    use std::time::Duration;

    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, WPARAM};
    use windows::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, HWND_MESSAGE, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_COPYDATA};
    use windows::core::{HSTRING, w};

    use super::{Claim, MAGIC};

    pub fn claim(name: &str, args: &[String], silent: bool) -> Claim {
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
        if silent {
            return Claim::Handed;
        }
        if hand_over(args, 20) { Claim::Handed } else { Claim::First }
    }

    /// Tells the running copy; its tray window may not be up yet if it is just starting.
    pub fn hand_over(args: &[String], tries: u32) -> bool {
        for _ in 0..tries {
            let hwnd = unsafe { FindWindowExW(Some(HWND_MESSAGE), None, w!("MinniondeckTray"), None) };
            if let Ok(h) = hwnd {
                if !h.0.is_null() {
                    // one message for each thing to open, or one empty one: show the window
                    let list: Vec<String> = if args.is_empty() { vec![String::new()] } else { args.to_vec() };
                    return list.iter().all(|a| send(h, a));
                }
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
    pub fn claim(_: &str, _: &[String], _: bool) -> Claim {
        Claim::First
    }
    pub fn hand_over(_: &[String], _: u32) -> bool {
        false
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
