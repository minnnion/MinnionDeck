// Opening .mdprofile and .mdplugin files by double-click.
//
// Windows starts Minniondeck with the file's path. If the app is already running, the
// single-instance plugin hands the path to the running copy instead (see lib.rs). Paths that
// arrive before the UI is ready wait here until it asks for them. The UI imports a profile, or
// shows what a plugin is and asks before installing it.

use std::sync::Mutex;

use tauri::State;

use crate::profiles::EXT;

/// Plugin packs (see cli.rs and plugins.rs)
pub const PLUGIN_EXT: &str = "mdplugin";

#[derive(Default)]
pub struct PendingOpen(pub Mutex<Vec<String>>);

/// The .mdprofile and .mdplugin paths among a program's arguments (the first is the program itself)
pub fn profile_args(args: &[String]) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter(|a| {
            let a = a.to_ascii_lowercase();
            a.ends_with(&format!(".{EXT}")) || a.ends_with(&format!(".{PLUGIN_EXT}"))
        })
        .cloned()
        .collect()
}

/// What Minniondeck was asked to open: .mdprofile and .mdplugin paths, and marketplace install
/// links (minniondeck://install?id=…, see market.rs)
pub fn open_args(args: &[String]) -> Vec<String> {
    let mut out = profile_args(args);
    out.extend(args.iter().skip(1).filter(|a| crate::market::parse_link(a).is_some()).map(|a| a.trim().to_string()));
    out
}

#[tauri::command]
pub fn take_pending_open(pending: State<PendingOpen>) -> Vec<String> {
    std::mem::take(&mut *pending.0.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Tell Windows that .mdprofile and .mdplugin files open with this copy of Minniondeck. Only the
/// current user's settings are touched (no admin rights needed), and only when they point
/// elsewhere, for example after the app was moved.
#[cfg(windows)]
pub fn register_file_type() -> Result<(), String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?.display().to_string();
    let command = format!("\"{exe}\" \"%1\"");
    let classes = RegKey::predef(HKEY_CURRENT_USER).create_subkey("Software\\Classes").map_err(|e| e.to_string())?.0;
    let set = |path: &str, value: &str| -> Result<(), String> {
        let (key, _) = classes.create_subkey(path).map_err(|e| e.to_string())?;
        key.set_value("", &value.to_string()).map_err(|e| e.to_string())
    };
    for (class, label, ext) in [("Minniondeck.Profile", "Minniondeck profile", EXT), ("Minniondeck.Plugin", "Minniondeck plugin", PLUGIN_EXT)] {
        let (open, _) = classes.create_subkey(format!("{class}\\shell\\open\\command")).map_err(|e| e.to_string())?;
        if open.get_value::<String, _>("").ok().as_deref() == Some(command.as_str()) {
            continue;
        }
        set(class, label)?;
        set(&format!("{class}\\DefaultIcon"), &format!("\"{exe}\",0"))?;
        set(&format!(".{ext}"), class)?;
        open.set_value("", &command).map_err(|e| e.to_string())?;
    }
    // minniondeck:// links, so the marketplace website's Install button opens the app
    let scheme = crate::market::SCHEME;
    let (open, _) = classes.create_subkey(format!("{scheme}\\shell\\open\\command")).map_err(|e| e.to_string())?;
    if open.get_value::<String, _>("").ok().as_deref() != Some(command.as_str()) {
        set(scheme, "URL:Minniondeck link")?;
        classes.create_subkey(scheme).map_err(|e| e.to_string())?.0.set_value("URL Protocol", &"").map_err(|e| e.to_string())?;
        set(&format!("{scheme}\\DefaultIcon"), &format!("\"{exe}\",0"))?;
        open.set_value("", &command).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::profile_args;

    #[test]
    fn only_profile_files_are_taken_from_the_arguments() {
        let args = ["app.exe", "C:\\Users\\me\\Streaming.MDPROFILE", "--flag", "notes.txt", "D:\\obs-1.0.mdplugin"].map(String::from);
        assert_eq!(profile_args(&args), vec!["C:\\Users\\me\\Streaming.MDPROFILE".to_string(), "D:\\obs-1.0.mdplugin".to_string()]);
    }

    #[test]
    fn install_links_are_opened_but_pairing_links_are_not() {
        let args = ["app.exe", "minniondeck://install/?id=minnion_spotify", "minniondeck://pair?v=1&h=1.2.3.4", "D:\\obs-1.0.mdplugin"].map(String::from);
        assert_eq!(super::open_args(&args), vec!["D:\\obs-1.0.mdplugin".to_string(), "minniondeck://install/?id=minnion_spotify".to_string()]);
    }
}
