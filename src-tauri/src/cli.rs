//! Command-line tools for plugin authors:
//!
//!   Minniondeck.exe plugin new <id> [--dir <folder>]   a new plugin project from the template
//!   Minniondeck.exe plugin pack [<folder>]             dist\<id>-<version>.mdplugin from a built plugin
//!   Minniondeck.exe plugin install <file.mdplugin>     install it; the running Minniondeck loads it at once
//!   Minniondeck.exe plugin key [--name <author>]       make this PC's author key, for signing (once)
//!   Minniondeck.exe profile build <bundle.json> [<out.mdprofile>]
//!                                                      seal a profile made outside the app (such as a
//!                                                      plugin's ready-made pages), to import by double-click
//!
//! Minniondeck.exe is a windowed app, so a terminal doesn't wait for it. When its output is piped
//! (build.ps1 does, which also makes PowerShell wait) it writes there; otherwise it attaches to the
//! terminal it was started from and writes there.
//!
//! A .mdplugin is a zip of the plugin's folder as installed: manifest.json, actions.json, the DLL,
//! icons\ and gifs\.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::plugins::{Manifest, check_manifest, safe_name};
use crate::signing::{self, Trust};

const USAGE: &str = "Plugin commands:
  Minniondeck.exe plugin new <id> [--dir <folder>]   make a new plugin project (id: lower-case letters, digits and _)
  Minniondeck.exe plugin pack [<folder>]             pack a built plugin as dist\\<id>-<version>.mdplugin
  Minniondeck.exe plugin install <file.mdplugin>     install a plugin; a running Minniondeck loads it at once
  Minniondeck.exe plugin key [--name <author>]       make your author key once; pack then signs your plugins";

const PROFILE_USAGE: &str = "Profile commands:
  Minniondeck.exe profile build <bundle.json> [<out.mdprofile>]   seal a profile bundle; double-click the file to import it";

/// A profile bundle (the JSON the app exports, without pictures) sealed as a .mdprofile
fn build_profile(args: &[String]) -> Result<String, String> {
    let src = args.first().map(PathBuf::from).ok_or(PROFILE_USAGE)?;
    let text = fs::read_to_string(&src).map_err(|e| format!("Could not read {}: {e}", src.display()))?;
    let bundle: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{} is not valid JSON: {e}", src.display()))?;
    let out = args.get(1).map(PathBuf::from).unwrap_or_else(|| src.with_extension("mdprofile"));
    let path = crate::profiles::build_to_file(&bundle, &out)?;
    Ok(format!("Made {path}. Double-click it (or use Import in Profiles) to add it to Minniondeck."))
}

/// Largest single icon or GIF packed
const MAX_MEDIA_BYTES: u64 = 8 * 1024 * 1024;
/// Largest single file in a plugin's data folder (such as minnion_speech's speech model). The
/// whole pack still has to stay under the 200 MB the installer takes.
const MAX_DATA_BYTES: u64 = 100 * 1024 * 1024;

/// Every file under a plugin's data\ folder, sub folders too, as (name in the zip, file on disk)
fn data_files(folder: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![(folder.join("data"), "data".to_string())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            if name.starts_with('.') {
                continue;
            }
            if !safe_name(&name) {
                return Err(format!("{prefix}\\{name}: use only letters, digits, - _ and . in file names."));
            }
            if path.is_dir() {
                stack.push((path, format!("{prefix}/{name}")));
            } else if path.is_file() {
                if fs::metadata(&path).map(|md| md.len()).unwrap_or(0) > MAX_DATA_BYTES {
                    return Err(format!("{prefix}\\{name} is bigger than 100 MB."));
                }
                out.push((format!("{prefix}/{name}"), path));
            }
        }
    }
    out.sort();
    Ok(out)
}

const T_CARGO: &str = include_str!("../plugin-template/Cargo.toml.tmpl");
const T_LIB: &str = include_str!("../plugin-template/lib.rs.tmpl");
const T_MANIFEST: &str = include_str!("../plugin-template/manifest.json.tmpl");
const T_ACTIONS: &str = include_str!("../plugin-template/actions.json.tmpl");
const T_BUILD_PS1: &str = include_str!("../plugin-template/build.ps1.tmpl");
const T_BUILD_CMD: &str = include_str!("../plugin-template/build.cmd");
const T_README: &str = include_str!("../plugin-template/README.md.tmpl");
const T_GITIGNORE: &str = include_str!("../plugin-template/gitignore");
const T_ICON: &[u8] = include_bytes!("../plugin-template/icon.png");
const KIT_CARGO: &str = include_str!("../../plugin-kit/Cargo.toml");
const KIT_LIB: &str = include_str!("../../plugin-kit/src/lib.rs");

/// Runs a plugin command if the arguments are one. Returns the exit code, or None to start the app.
pub fn run(args: &[String]) -> Option<i32> {
    let command = args.get(1).map(String::as_str);
    if command != Some("plugin") && command != Some("profile") && command != Some("license") {
        return None;
    }
    attach_console();
    let rest = &args[2..];
    let result = if command == Some("profile") {
        match rest.first().map(String::as_str) {
            Some("build") => build_profile(&rest[1..]),
            _ => Err(PROFILE_USAGE.into()),
        }
    } else if command == Some("license") {
        license_cmd(rest)
    } else {
        match rest.first().map(String::as_str) {
            Some("new") => new(&rest[1..]),
            Some("pack") => pack(&rest[1..]),
            Some("install") => install(&rest[1..]),
            Some("key") => key(&rest[1..]),
            None | Some("help" | "--help" | "-h" | "/?") => Ok(USAGE.into()),
            Some(other) => Err(format!("\"{other}\" isn't a plugin command.\n\n{USAGE}")),
        }
    };
    let code = match result {
        Ok(msg) => {
            println!("{msg}");
            0
        }
        Err(e) => {
            println!("Error: {e}");
            1
        }
    };
    let _ = std::io::stdout().flush();
    Some(code)
}

/// Write to the terminal that started us, unless our output is already piped somewhere
fn attach_console() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE};
        let piped = matches!(GetStdHandle(STD_OUTPUT_HANDLE), Ok(h) if !h.is_invalid() && !h.0.is_null());
        if !piped && AttachConsole(ATTACH_PARENT_PROCESS).is_ok() {
            // The terminal already printed its prompt; start on a fresh line
            println!();
        }
    }
}

/// %APPDATA%\com.minnion.deck, where the app keeps its data
fn app_data() -> Result<PathBuf, String> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("com.minnion.deck")).ok_or_else(|| "APPDATA isn't set.".into())
}

fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase()) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && id.len() <= 40
}

/// "my_plugin" → "My plugin"
fn display_name(id: &str) -> String {
    let words = id.split('_').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// "my_plugin" → "MyPlugin"
fn type_name(id: &str) -> String {
    let t: String = id
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect();
    if t.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) { t } else { format!("Plugin{t}") }
}

fn new(args: &[String]) -> Result<String, String> {
    let mut id = None;
    let mut dir = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" => {
                dir = Some(PathBuf::from(args.get(i + 1).ok_or("--dir needs a folder.")?));
                i += 1;
            }
            a if id.is_none() => id = Some(a.to_string()),
            a => return Err(format!("Unexpected \"{a}\".\n\n{USAGE}")),
        }
        i += 1;
    }
    let id = id.ok_or_else(|| format!("Give the plugin an id, e.g. plugin new my_plugin\n\n{USAGE}"))?;
    if !valid_id(&id) {
        let hint: String = id.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        return Err(format!("\"{id}\" can't be a plugin id. Use lower-case letters, digits and _, starting with a letter (like \"{hint}\")."));
    }
    let base = match dir {
        Some(d) => d,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let folder = base.join(&id);
    if folder.exists() {
        return Err(format!("{} already exists. Pick another id or folder.", folder.display()));
    }
    let name = display_name(&id);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.to_string_lossy().replace('\'', "''");
    let fill = |t: &str| t.replace("{{id}}", &id).replace("{{name}}", &name).replace("{{type}}", &type_name(&id)).replace("{{exe}}", &exe);
    let files: Vec<(&str, Vec<u8>)> = vec![
        ("Cargo.toml", fill(T_CARGO).into_bytes()),
        ("src/lib.rs", fill(T_LIB).into_bytes()),
        ("manifest.json", fill(T_MANIFEST).into_bytes()),
        ("actions.json", fill(T_ACTIONS).into_bytes()),
        ("build.ps1", fill(T_BUILD_PS1).into_bytes()),
        ("build.cmd", T_BUILD_CMD.as_bytes().to_vec()),
        ("README.md", fill(T_README).into_bytes()),
        (".gitignore", T_GITIGNORE.as_bytes().to_vec()),
        ("icons/icon.png", T_ICON.to_vec()),
        ("gifs/.keep", Vec::new()),
        ("kit/Cargo.toml", KIT_CARGO.as_bytes().to_vec()),
        ("kit/src/lib.rs", KIT_LIB.as_bytes().to_vec()),
    ];
    for (rel, bytes) in &files {
        let path = folder.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
        }
        fs::write(&path, bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    }
    let f = folder.display();
    Ok(format!(
        "Made the plugin \"{name}\" in {f}\n\nNext:\n  1. Write what it does in {f}\\src\\lib.rs, and its actions in actions.json\n  2. Double-click {f}\\build.cmd to build it and install it into Minniondeck\n     (Rust is installed the first time if this PC doesn't have it)\n  3. In Minniondeck: a button's Edit window, What it does, Action, {name}"
    ))
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|_| format!("{} is missing.", path.display()))
}

/// The files that go in a pack, as (name in the zip, file on disk)
fn pack_files(folder: &Path, m: &Manifest) -> Result<Vec<(String, PathBuf)>, String> {
    let dll = [folder.join("target").join("release").join(&m.dll), folder.join(&m.dll)]
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| format!("{} isn't built yet. Run build.ps1 (or cargo build --release) first.", m.dll))?;
    let mut files = vec![("manifest.json".to_string(), folder.join("manifest.json")), (m.actions.replace('\\', "/"), folder.join(&m.actions)), (m.dll.replace('\\', "/"), dll)];
    // lib\ holds native DLLs the plugin loads itself (such as SimConnect.dll), next to its own
    for sub in ["icons", "gifs", "lib"] {
        let Ok(rd) = fs::read_dir(folder.join(sub)) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            if !path.is_file() || name.starts_with('.') {
                continue;
            }
            if !safe_name(&name) {
                return Err(format!("{sub}\\{name}: use only letters, digits, - _ and . in file names."));
            }
            if fs::metadata(&path).map(|md| md.len()).unwrap_or(0) > MAX_MEDIA_BYTES {
                return Err(format!("{sub}\\{name} is bigger than 8 MB."));
            }
            files.push((format!("{sub}/{name}"), path));
        }
    }
    // data\ holds whatever else the plugin brings, in sub folders if it likes (a model, voices)
    files.extend(data_files(folder)?);
    Ok(files)
}

fn pack(args: &[String]) -> Result<String, String> {
    let folder = match args.first() {
        Some(f) => PathBuf::from(f),
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let m = check_manifest(&read_text(&folder.join("manifest.json"))?)?;
    // Check actions.json before packing it, so mistakes show up here rather than in the app
    let actions: serde_json::Value = serde_json::from_str(&read_text(&folder.join(&m.actions))?).map_err(|e| format!("{} is not valid JSON: {e}", m.actions))?;
    if !actions.is_array() {
        return Err(format!("{} must be a list of actions: [ {{ … }}, … ]", m.actions));
    }
    let files = pack_files(&folder, &m)?;
    let version = if m.version.is_empty() { "0.0.0".to_string() } else { m.version.clone() };
    let dist = folder.join("dist");
    fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    let out = dist.join(format!("{}-{}.mdplugin", m.id, version.replace(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-'), "_")));
    let file = fs::File::create(&out).map_err(|e| format!("Could not write {}: {e}", out.display()))?;
    let mut contents = Vec::new();
    for (name, path) in &files {
        contents.push((name.clone(), fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?));
    }
    // Signed with this PC's author key, when it has one
    let signed = signing::sign(&app_data()?, &contents)?;
    let count = contents.len();
    if let Some((_, sig)) = &signed {
        contents.push((signing::SIG_FILE.to_string(), sig.clone()));
    }
    let mut zip = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in &contents {
        zip.start_file(name.as_str(), opts).map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    let how = match &signed {
        Some((me, _)) => format!("Signed by {} (key {}).", me.name, signing::fingerprint(&me.key)),
        None => "Not signed: this PC has no author key. Make one with: Minniondeck.exe plugin key --name \"Your name\"".into(),
    };
    Ok(format!("Packed {} {version} ({count} files) into {}\n{how}", m.name, out.display()))
}

fn install(args: &[String]) -> Result<String, String> {
    let file = PathBuf::from(args.first().ok_or_else(|| format!("Which .mdplugin file?\n\n{USAGE}"))?);
    let root = app_data()?;
    let (m, dest, trust) = crate::plugins::install_file(&root, &file)?;
    let cfg = crate::plugins::read_config(&root);
    let runs = crate::plugins::may_run(&trust, &cfg);
    let signed = match &trust {
        Trust::Trusted { author } | Trust::User { author } => format!("Signed by {author}, a trusted author."),
        Trust::Untrusted { author } => format!("Signed by {author}, who this PC doesn't trust yet."),
        _ => "Not signed.".into(),
    };
    // Tell the running Minniondeck; with none running this copy just ends (lib.rs setup) and
    // nothing starts. Its output must not hold our pipe open.
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let started = std::process::Command::new(exe)
        .args([crate::plugins::RELOAD_ARG, &m.id])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok();
    let next = match runs {
        Err(why) => format!("It won't run yet: {why}\n(In Minniondeck, Settings › Plugins: turn on Developer mode, or add your .mdkey under Trusted authors.)"),
        Ok(()) if started => "A running Minniondeck loads it now; otherwise it loads when Minniondeck starts.".into(),
        Ok(()) => "Start Minniondeck to load it.".into(),
    };
    Ok(format!("Installed {} {} into {}\n{signed}\n{next}", m.name, m.version, dest.display()))
}

/// Make this PC's author key: the private part stays here, the .mdkey goes to whoever trusts you
fn key(args: &[String]) -> Result<String, String> {
    let mut name = std::env::var("USERNAME").unwrap_or_default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                name = args.get(i + 1).ok_or("--name needs your author name.")?.clone();
                i += 1;
            }
            a => return Err(format!("Unexpected \"{a}\".\n\n{USAGE}")),
        }
        i += 1;
    }
    let out = std::env::current_dir().map_err(|e| e.to_string())?;
    let file = signing::make_key(&app_data()?, &name, &out)?;
    let me = signing::author(&app_data()?).ok_or("The key wasn't saved.")?;
    Ok(format!(
        "Made the author key for {} (key {}).\n\nYour public key: {}\n  Send this file to whoever should trust your plugins; they add it in Settings › Plugins › Trusted authors.\n  Add it on this PC too, to run your own signed plugins without Developer mode.\nYour private key stays on this PC, locked to your Windows account. plugin pack now signs with it.",
        me.name,
        signing::fingerprint(&me.key),
        file.display()
    ))
}

const LICENSE_USAGE: &str = "Minniondeck licensing (owner only):\n  license show                   this PC's machine code\n  license init                   make the licensing key once (prints the public key to paste into license.rs)\n  license new <machine-code> [--name \"Who it's for\"]   sign a licence for that PC → <code>.mdlicense";

/// license show / init / new — the owner's key tools (see license.rs)
fn license_cmd(rest: &[String]) -> Result<String, String> {
    match rest.first().map(String::as_str) {
        Some("show") => Ok(format!("This PC's machine code:\n\n  {}\n\nSend this to get a licence key.", crate::license::machine_code_display())),
        Some("init") => crate::license::owner_init(&app_data()?),
        Some("new") => {
            let mut code = None;
            let mut name = String::new();
            let mut i = 1;
            while i < rest.len() {
                match rest[i].as_str() {
                    "--name" => {
                        name = rest.get(i + 1).ok_or("--name needs a value.")?.clone();
                        i += 1;
                    }
                    a if code.is_none() => code = Some(a.to_string()),
                    a => return Err(format!("Unexpected \"{a}\".\n\n{LICENSE_USAGE}")),
                }
                i += 1;
            }
            let code = code.ok_or_else(|| format!("Which machine code?\n\n{LICENSE_USAGE}"))?;
            let out = std::env::current_dir().map_err(|e| e.to_string())?;
            crate::license::owner_issue(&app_data()?, &code, &name, &out)
        }
        None | Some("help" | "--help" | "-h" | "/?") => Ok(LICENSE_USAGE.into()),
        Some(other) => Err(format!("\"{other}\" isn't a license command.\n\n{LICENSE_USAGE}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_ids_and_names() {
        assert!(valid_id("my_plugin2"));
        assert!(!valid_id("My Plugin"));
        assert!(!valid_id("2fast"));
        assert!(!valid_id("a-b"));
        assert_eq!(display_name("my_obs_plugin"), "My obs plugin");
        assert_eq!(type_name("my_obs_plugin"), "MyObsPlugin");
    }

    #[test]
    fn new_pack_and_unpack_round_trip() {
        let root = std::env::temp_dir().join(format!("md-cli-test-{}", crate::storage::now_ms()));
        fs::create_dir_all(&root).unwrap();
        let msg = new(&["demo_plugin".into(), "--dir".into(), root.to_string_lossy().into()]).unwrap();
        assert!(msg.contains("Demo plugin"));
        let folder = root.join("demo_plugin");
        for f in ["Cargo.toml", "src/lib.rs", "manifest.json", "actions.json", "build.ps1", "build.cmd", "icons/icon.png", "kit/src/lib.rs"] {
            assert!(folder.join(f).is_file(), "{f}");
        }
        assert!(fs::read_to_string(folder.join("src/lib.rs")).unwrap().contains("struct DemoPlugin"));
        assert!(new(&["demo_plugin".into(), "--dir".into(), root.to_string_lossy().into()]).unwrap_err().contains("already exists"));
        // Not built yet
        assert!(pack(&[folder.to_string_lossy().into()]).unwrap_err().contains("isn't built"));
        // Pretend it was built
        fs::create_dir_all(folder.join("target/release")).unwrap();
        fs::write(folder.join("target/release/demo_plugin.dll"), b"dll").unwrap();
        assert!(pack(&[folder.to_string_lossy().into()]).unwrap().contains("demo_plugin-0.1.0.mdplugin"));
        let out = root.join("unpacked");
        let m = crate::plugins::unpack(&folder.join("dist/demo_plugin-0.1.0.mdplugin"), &out).unwrap();
        assert_eq!(m.id, "demo_plugin");
        assert!(out.join("demo_plugin.dll").is_file() && out.join("icons/icon.png").is_file() && out.join("actions.json").is_file());
        let _ = fs::remove_dir_all(&root);
    }
}
