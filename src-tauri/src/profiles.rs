// Export and import of single profiles as .mdprofile files.
//
// Inside the (encrypted) zip:
//   manifest.json        read first: what is inside, checksums, plugins needed
//   profile.json         name, colour, icon, auto-switch app, and the folder tree
//   pages/<name>.json    one file per page: grid size and its buttons with their actions
//   designs.json         Button creator designs the buttons use
//   assets/assets.json   index of the pictures and GIFs below, and GIFs drawn in the GIF maker
//   assets/images/       uploaded pictures
//   assets/gifs/         imported GIF files
//   assets/icons/        (custom icons, for later; the built-in icons are part of the app)
//   plugins.json         plugins the buttons use
//
// The UI gathers what a profile uses and remaps IDs on import; this side packs, checks and unpacks.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::backups::{self, Reason};
use crate::package::{self, Kind, PackageWriter};
use crate::storage::{Store, safe_name, write_atomic};

pub const EXT: &str = "mdprofile";
const MAX_FILE_BYTES: u64 = 600 * 1024 * 1024;

#[derive(Default)]
struct Counts {
    pages: u64,
    folders: u64,
    buttons: u64,
}

/// "Design work" → "design-work", unique within the package
fn page_slug(name: &str, used: &mut HashSet<String>) -> String {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let base = slug.trim_matches('-').chars().take(40).collect::<String>();
    let base = if base.is_empty() { "page".to_string() } else { base };
    let mut slug = base.clone();
    let mut n = 2;
    while !used.insert(slug.clone()) {
        slug = format!("{base}-{n}");
        n += 1;
    }
    slug
}

/// Turn the UI's tree into the file's tree: each page's buttons go into its own file
fn pack_tree(nodes: &Value, boards: &Value, w: &mut PackageWriter, used: &mut HashSet<String>, counts: &mut Counts) -> Result<Value, String> {
    let mut out = Vec::new();
    for node in nodes.as_array().ok_or("The profile's pages could not be read.")? {
        match node["kind"].as_str() {
            Some("folder") => {
                counts.folders += 1;
                let mut folder = node.clone();
                folder["children"] = pack_tree(&node["children"], boards, w, used, counts)?;
                out.push(folder);
            }
            Some("page") => {
                counts.pages += 1;
                let name = node["name"].as_str().unwrap_or("Page");
                let buttons = node["board"].as_str().and_then(|b| boards.get(b)).cloned().unwrap_or_else(|| json!([]));
                counts.buttons += buttons.as_array().map_or(0, |a| a.len() as u64);
                let file = format!("pages/{}.json", page_slug(name, used));
                w.add_json(&file, &json!({ "id": node["id"], "name": name, "cols": node["cols"], "rows": node["rows"], "buttons": buttons }))?;
                out.push(json!({ "kind": "page", "id": node["id"], "name": name, "file": file }));
            }
            _ => return Err("The profile has an unknown kind of page.".into()),
        }
    }
    Ok(Value::Array(out))
}

/// Pack a media file from the library into the package, and point its index entry at it
fn pack_media(media_dir: &Path, items: &Value, folder: &str, w: &mut PackageWriter) -> Result<Value, String> {
    let mut out = Vec::new();
    for item in items.as_array().into_iter().flatten() {
        let Some(file) = item["media"].as_str().and_then(safe_name) else { continue };
        let bytes = fs::read(media_dir.join(file)).map_err(|_| format!("The picture {file} is missing from the library."))?;
        let path = format!("assets/{folder}/{file}");
        w.add(&path, &bytes)?;
        let mut entry = item.clone();
        entry["media"] = json!(path);
        out.push(entry);
    }
    Ok(Value::Array(out))
}

/// Build the encrypted .mdprofile bytes from what the UI gathered for one profile
fn build(media_dir: &Path, bundle: &Value) -> Result<Vec<u8>, String> {
    let mut profile = bundle["profile"].clone();
    let name = profile["name"].as_str().unwrap_or("Profile").to_string();
    let mut w = PackageWriter::new();
    let mut used = HashSet::new();
    let mut counts = Counts::default();
    profile["tree"] = pack_tree(&profile["tree"], &bundle["boards"], &mut w, &mut used, &mut counts)?;
    w.add_json("profile.json", &profile)?;
    let designs = bundle.get("designs").cloned().unwrap_or_else(|| json!([]));
    w.add_json("designs.json", &designs)?;
    let images = pack_media(media_dir, &bundle["images"], "images", &mut w)?;
    let gif_files = pack_media(media_dir, &bundle["gifFiles"], "gifs", &mut w)?;
    let drawn = bundle.get("drawnGifs").cloned().unwrap_or_else(|| json!([]));
    w.add_dir("assets/icons/")?;
    let contents = json!({
        "pages": counts.pages, "folders": counts.folders, "buttons": counts.buttons,
        "designs": designs.as_array().map_or(0, |a| a.len()),
        "images": images.as_array().map_or(0, |a| a.len()),
        "gifs": gif_files.as_array().map_or(0, |a| a.len()) + drawn.as_array().map_or(0, |a| a.len()),
    });
    w.add_json("assets/assets.json", &json!({ "images": images, "gifFiles": gif_files, "drawnGifs": drawn }))?;
    let plugins: Vec<String> = bundle["plugins"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(String::from)).collect();
    w.add_json("plugins.json", &json!({ "plugins": plugins.iter().map(|p| json!({ "name": p })).collect::<Vec<_>>() }))?;
    let zip = w.finish(Kind::Profile, &name, contents, plugins)?;
    package::seal(Kind::Profile, &zip)
}

/// Seal a profile bundle (as the UI gathers it for Export) into a .mdprofile file. For the
/// command line ("Minniondeck.exe profile build"): such a bundle carries no pictures or GIF files.
pub(crate) fn build_to_file(bundle: &Value, out: &Path) -> Result<String, String> {
    if bundle["images"].as_array().is_some_and(|a| !a.is_empty()) || bundle["gifFiles"].as_array().is_some_and(|a| !a.is_empty()) {
        return Err("A profile made on the command line can't carry pictures or GIF files.".into());
    }
    let bytes = build(Path::new("."), bundle)?;
    let out = with_ext(out.to_path_buf());
    fs::write(&out, bytes).map_err(|e| format!("Could not write {}: {e}", out.display()))?;
    Ok(out.display().to_string())
}

/// Make a name safe to use as a Windows file name
fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { '-' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "Profile".into() } else { s }
}

fn with_ext(mut path: PathBuf) -> PathBuf {
    if path.extension().map_or(true, |e| !e.eq_ignore_ascii_case(EXT)) {
        let mut s = path.into_os_string();
        s.push(format!(".{EXT}"));
        path = PathBuf::from(s);
    }
    path
}

/// Export one profile. Asks where to save it; None means the user cancelled.
#[tauri::command]
pub async fn export_profile(app: AppHandle, name: String, bundle: String) -> Result<Option<String>, String> {
    let picked = app
        .dialog()
        .file()
        .set_title("Export profile")
        .add_filter("Minniondeck profile", &[EXT])
        .set_file_name(format!("{}.{EXT}", file_stem(&name)))
        .blocking_save_file();
    let Some(picked) = picked else { return Ok(None) };
    let path = with_ext(picked.into_path().map_err(|e| e.to_string())?);
    tauri::async_runtime::spawn_blocking(move || {
        let bundle: Value = serde_json::from_str(&bundle).map_err(|_| "The profile could not be read.".to_string())?;
        let bytes = build(&app.state::<Store>().paths.media, &bundle)?;
        write_atomic(&path, &bytes)?;
        crate::applog::line(&format!("Exported a profile to {}", path.display()));
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Deserialize)]
pub struct NamedBundle {
    name: String,
    bundle: String,
}

#[derive(Serialize)]
pub struct ExportedAll {
    folder: String,
    count: usize,
}

/// Export every profile into one folder, one .mdprofile each. Existing files are never replaced.
#[tauri::command]
pub async fn export_all_profiles(app: AppHandle, profiles: Vec<NamedBundle>) -> Result<Option<ExportedAll>, String> {
    let Some(folder) = app.dialog().file().set_title("Choose a folder for the profiles").blocking_pick_folder() else { return Ok(None) };
    let folder = folder.into_path().map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        for p in &profiles {
            let bundle: Value = serde_json::from_str(&p.bundle).map_err(|_| format!("{} could not be read.", p.name))?;
            let bytes = build(&store.paths.media, &bundle)?;
            let stem = file_stem(&p.name);
            let mut path = folder.join(format!("{stem}.{EXT}"));
            let mut n = 2;
            while path.exists() {
                path = folder.join(format!("{stem} ({n}).{EXT}"));
                n += 1;
            }
            write_atomic(&path, &bytes)?;
        }
        Ok(Some(ExportedAll { folder: folder.display().to_string(), count: profiles.len() }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn pick_profile_file(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app.dialog().file().set_title("Import a profile").add_filter("Minniondeck profile", &[EXT]).blocking_pick_file();
    match picked {
        Some(p) => Ok(Some(p.into_path().map_err(|e| e.to_string())?.display().to_string())),
        None => Ok(None),
    }
}

/// Turn the file's tree back into the UI's: each page gets its grid and buttons inline
fn unpack_tree(nodes: &Value, pkg: &package::Package) -> Result<Value, String> {
    let mut out = Vec::new();
    for node in nodes.as_array().ok_or("The profile's pages could not be read.")? {
        match node["kind"].as_str() {
            Some("folder") => {
                let mut folder = node.clone();
                folder["children"] = unpack_tree(&node["children"], pkg)?;
                out.push(folder);
            }
            Some("page") => {
                let file = node["file"].as_str().ok_or("A page in the file has no data.")?;
                let page = pkg.json(file)?;
                out.push(json!({
                    "kind": "page", "id": node["id"], "name": page["name"],
                    "cols": page["cols"], "rows": page["rows"],
                    "buttons": page.get("buttons").cloned().unwrap_or_else(|| json!([])),
                }));
            }
            _ => return Err("The file has an unknown kind of page.".into()),
        }
    }
    Ok(Value::Array(out))
}

/// Copy a package's pictures or GIFs into the library under new names
fn unpack_media(media_dir: &Path, items: &Value, pkg: &package::Package) -> Result<Value, String> {
    let mut out = Vec::new();
    for item in items.as_array().into_iter().flatten() {
        let Some(path) = item["media"].as_str() else { continue };
        let bytes = pkg.get(path).ok_or_else(|| format!("{path} is missing from the file."))?;
        let original = path.rsplit('/').next().and_then(safe_name).unwrap_or("file.png");
        let mut tag = [0u8; 4];
        OsRng.fill_bytes(&mut tag);
        let name = format!("imp-{}-{original}", tag.iter().map(|b| format!("{b:02x}")).collect::<String>());
        write_atomic(&media_dir.join(&name), bytes)?;
        let mut entry = item.clone();
        entry["media"] = json!(name);
        out.push(entry);
    }
    Ok(Value::Array(out))
}

/// Decrypt a .mdprofile and check every file in it against the manifest. Changes nothing.
fn read_profile(bytes: &[u8]) -> Result<package::Package, String> {
    package::read(&package::open(bytes, Kind::Profile)?, Kind::Profile)
}

/// Copy the package's pictures and GIFs into the media folder and rebuild the profile the way the UI holds it
fn unpack(media_dir: &Path, pkg: &package::Package) -> Result<Value, String> {
    let mut profile = pkg.json("profile.json")?;
    profile["tree"] = unpack_tree(&profile["tree"], pkg)?;
    let assets = pkg.json_or("assets/assets.json", json!({}))?;
    let images = unpack_media(media_dir, &assets["images"], pkg)?;
    let gif_files = unpack_media(media_dir, &assets["gifFiles"], pkg)?;
    Ok(json!({
        "manifest": { "name": pkg.manifest.name, "contents": pkg.manifest.contents, "created": pkg.manifest.created_ms },
        "profile": profile,
        "designs": pkg.json_or("designs.json", json!([]))?,
        "drawnGifs": assets.get("drawnGifs").cloned().unwrap_or_else(|| json!([])),
        "images": images,
        "gifFiles": gif_files,
        "plugins": pkg.manifest.plugins,
    }))
}

/// Open and check a .mdprofile, back up the library, copy its pictures and GIFs in,
/// and hand the profile to the UI, which gives everything new IDs and adds it.
#[tauri::command]
pub async fn import_profile(app: AppHandle, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let size = fs::metadata(&path).map_err(|_| "That file could not be opened.".to_string())?.len();
        if size > MAX_FILE_BYTES {
            return Err("That file is too large.".into());
        }
        let bytes = fs::read(&path).map_err(|_| "That file could not be opened.".to_string())?;
        let pkg = read_profile(&bytes)?;
        // Everything is checked; now keep a way back before anything changes
        backups::create(&store, Reason::BeforeImport)?;
        crate::applog::line(&format!("Imported {}", pkg.manifest.name));
        Ok(unpack(&store.paths.media, &pkg)?.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_files_get_unique_names() {
        let mut used = HashSet::new();
        assert_eq!(page_slug("Main", &mut used), "main");
        assert_eq!(page_slug("Design work!", &mut used), "design-work");
        assert_eq!(page_slug("main", &mut used), "main-2");
        assert_eq!(page_slug("★★", &mut used), "page");
    }

    #[test]
    fn exported_names_are_safe_file_names() {
        assert_eq!(file_stem("Live: show/1?"), "Live- show-1-");
        assert_eq!(file_stem("   "), "Profile");
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("minniondeck-test-{tag}-{}", crate::storage::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_profile_survives_export_and_import() {
        let (from, to) = (temp_dir("from"), temp_dir("to"));
        fs::write(from.join("u-1-abc.png"), [1u8, 2, 3]).unwrap();
        let bundle = json!({
            "profile": { "id": "pr1", "name": "Streaming", "body": "mint", "icon": "star", "auto": "OBS Studio", "tree": [
                { "kind": "page", "id": "p1", "name": "Main", "board": "B1", "cols": 4, "rows": 3 },
                { "kind": "folder", "id": "f1", "name": "Live", "open": true, "children": [
                    { "kind": "page", "id": "p2", "name": "Main", "board": "B2", "cols": 5, "rows": 3 }
                ] }
            ] },
            "boards": { "B1": [{ "id": "t1", "label": "Mute", "steps": [{ "type": "obs-scene", "f": { "scene": "BRB" } }] }], "B2": [] },
            "designs": [{ "id": "d1", "name": "Logo" }],
            "drawnGifs": [{ "id": "g1", "name": "Blink", "size": 16, "fps": 8, "frames": [[null]] }],
            "images": [{ "id": "u:1", "name": "Pic", "media": "u-1-abc.png", "ar": 1.5 }],
            "gifFiles": [],
            "plugins": ["OBS"]
        });
        let bytes = build(&from, &bundle).unwrap();
        let pkg = read_profile(&bytes).unwrap();
        let mut files: Vec<&String> = pkg.paths().collect();
        files.sort();
        assert_eq!(files, ["assets/assets.json", "assets/images/u-1-abc.png", "designs.json", "pages/main-2.json", "pages/main.json", "plugins.json", "profile.json"]);
        assert_eq!(pkg.manifest.contents["pages"], 2);
        assert_eq!(pkg.manifest.contents["folders"], 1);
        assert_eq!(pkg.manifest.contents["buttons"], 1);
        // The file's tree points at page files instead of holding buttons
        assert_eq!(pkg.json("profile.json").unwrap()["tree"][0]["file"], "pages/main.json");

        let out = unpack(&to, &pkg).unwrap();
        let tree = &out["profile"]["tree"];
        assert_eq!(tree[0]["buttons"][0]["steps"][0]["f"]["scene"], "BRB");
        assert_eq!(tree[0]["cols"], 4);
        assert_eq!(tree[1]["children"][0]["cols"], 5);
        assert_eq!(out["designs"][0]["id"], "d1");
        assert_eq!(out["drawnGifs"][0]["id"], "g1");
        assert_eq!(out["plugins"], json!(["OBS"]));
        let media = out["images"][0]["media"].as_str().unwrap();
        assert!(media.starts_with("imp-") && media.ends_with("u-1-abc.png"));
        assert_eq!(fs::read(to.join(media)).unwrap(), [1u8, 2, 3]);
        let _ = (fs::remove_dir_all(from), fs::remove_dir_all(to));
    }

    #[test]
    fn a_sealed_package_opens_and_checks() {
        let mut w = PackageWriter::new();
        w.add_json("profile.json", &json!({ "name": "Test" })).unwrap();
        let zip = w.finish(Kind::Profile, "Test", json!({}), vec!["OBS".into()]).unwrap();
        let sealed = package::seal(Kind::Profile, &zip).unwrap();
        let pkg = package::read(&package::open(&sealed, Kind::Profile).unwrap(), Kind::Profile).unwrap();
        assert_eq!(pkg.json("profile.json").unwrap()["name"], "Test");
        assert_eq!(pkg.manifest.plugins, vec!["OBS".to_string()]);
        // Wrong kind, and a changed byte, are both refused
        assert!(package::open(&sealed, Kind::Backup).is_err());
        let mut bad = sealed.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert!(package::open(&bad, Kind::Profile).is_err());
    }
}
