// Export and import of single profiles as .mdprofile files. Ported from the Minniondeck app.
#![allow(dead_code)]
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
use serde_json::{Value, json};

use super::package::{self, Kind, PackageWriter};
use super::util::{safe_name, write_atomic};

pub const EXT: &str = "mdprofile";
/// The biggest .mdprofile opened. A profile is buttons and their pictures (25 MB each at most).
const MAX_FILE_BYTES: u64 = 200 * 1024 * 1024;

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
                let mut entry = json!({ "kind": "page", "id": node["id"], "name": name, "file": file });
                if let Some(c) = node.get("choice").and_then(|c| c.as_str()) {
                    entry["choice"] = json!(c);
                }
                out.push(entry);
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
        let bytes = super::media::read_named(media_dir, file).ok_or_else(|| format!("The picture {file} is missing from the library."))?;
        let path = format!("assets/{folder}/{file}");
        w.add(&path, &bytes)?;
        let mut entry = item.clone();
        entry["media"] = json!(path);
        out.push(entry);
    }
    Ok(Value::Array(out))
}

/// Build the encrypted .mdprofile bytes from what the UI gathered for one profile
pub fn build(media_dir: &Path, bundle: &Value) -> Result<Vec<u8>, String> {
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
    if s.is_empty() {
        return "Profile".into();
    }
    // names Windows keeps for devices can't be files ("CON.mdprofile" is the console)
    let base = s.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let reserved = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || (base.len() == 4 && (base.starts_with("COM") || base.starts_with("LPT")) && base[3..].chars().all(|c| c.is_ascii_digit() || "¹²³".contains(c)));
    if reserved { format!("{s} profile") } else { s }
}

fn with_ext(mut path: PathBuf) -> PathBuf {
    if path.extension().map_or(true, |e| !e.eq_ignore_ascii_case(EXT)) {
        let mut s = path.into_os_string();
        s.push(format!(".{EXT}"));
        path = PathBuf::from(s);
    }
    path
}

/// Export one profile into a file at `path`.
pub fn export_to(media_dir: &Path, bundle: &Value, path: &Path) -> Result<String, String> {
    let path = with_ext(path.to_path_buf());
    let bytes = build(media_dir, bundle)?;
    write_atomic(&path, &bytes)?;
    Ok(path.display().to_string())
}

/// A free file name in `folder` for a profile: "Name.mdprofile", then "Name (2).mdprofile"...
pub fn free_path(folder: &Path, name: &str) -> PathBuf {
    let stem = file_stem(name);
    let mut path = folder.join(format!("{stem}.{EXT}"));
    let mut n = 2;
    while path.exists() {
        path = folder.join(format!("{stem} ({n}).{EXT}"));
        n += 1;
    }
    path
}

pub fn default_file_name(name: &str) -> String {
    format!("{}.{EXT}", file_stem(name))
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
                let mut p = json!({
                    "kind": "page", "id": node["id"], "name": page["name"],
                    "cols": page["cols"], "rows": page["rows"],
                    "buttons": page.get("buttons").cloned().unwrap_or_else(|| json!([])),
                });
                // which of the profile's choices it belongs to ("obs"), when it has choices
                if let Some(c) = node.get("choice").and_then(|c| c.as_str()) {
                    p["choice"] = json!(c);
                }
                out.push(p);
            }
            _ => return Err("The file has an unknown kind of page.".into()),
        }
    }
    Ok(Value::Array(out))
}

/// Copy a package's pictures or GIFs into the library under new names
fn unpack_media(media_dir: Option<&Path>, items: &Value, pkg: &package::Package, kind: super::media::Kind) -> Result<Value, String> {
    let mut out = Vec::new();
    for item in items.as_array().into_iter().flatten() {
        let Some(path) = item["media"].as_str() else { continue };
        let bytes = pkg.get(path).ok_or_else(|| format!("{path} is missing from the file."))?;
        // the same checks as a picture imported by hand: its kind, its size, and that it opens. A
        // picture that fails is left out (its buttons show their colour instead); the rest of the
        // profile still comes in, as it did before these checks
        if super::media::check(bytes, kind).is_err() {
            continue;
        }
        // only looking: nothing is written
        let Some(media_dir) = media_dir else {
            out.push(item.clone());
            continue;
        };
        let original = path.rsplit('/').next().and_then(safe_name).unwrap_or("file.png");
        let mut tag = [0u8; 4];
        OsRng.fill_bytes(&mut tag);
        let name = format!("imp-{}-{original}", tag.iter().map(|b| format!("{b:02x}")).collect::<String>());
        super::media::store(media_dir, &name, bytes)?;
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

/// Copy the package's pictures and GIFs into the media folder (None: only check them) and rebuild
/// the profile the way the UI holds it
fn unpack(media_dir: Option<&Path>, pkg: &package::Package) -> Result<Value, String> {
    let mut profile = pkg.json("profile.json")?;
    profile["tree"] = unpack_tree(&profile["tree"], pkg)?;
    let assets = pkg.json_or("assets/assets.json", json!({}))?;
    let images = unpack_media(media_dir, &assets["images"], pkg, super::media::Kind::Picture)?;
    let gif_files = unpack_media(media_dir, &assets["gifFiles"], pkg, super::media::Kind::Gif)?;
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

/// Open and check a .mdprofile and copy its pictures and GIFs into the media folder. Returns the
/// profile as the window holds it; the library gives everything new IDs and adds it.
pub fn import_from(media_dir: &Path, path: &Path) -> Result<Value, String> {
    unpack(Some(media_dir), &open_file(path)?)
}

/// Opens and checks a .mdprofile without writing anything: the profile as `import_from` gives it,
/// but its pictures stay in the file (for looking at a profile before deciding to add it).
pub fn inspect_from(path: &Path) -> Result<Value, String> {
    unpack(None, &open_file(path)?)
}

/// How many pictures and GIF files a profile file lists, and how many pass the checks (tests).
#[cfg(test)]
pub fn picture_counts(path: &Path) -> Result<(usize, usize), String> {
    let pkg = open_file(path)?;
    let assets = pkg.json_or("assets/assets.json", json!({}))?;
    let listed = ["images", "gifFiles"].iter().map(|k| assets[*k].as_array().map_or(0, |a| a.len())).sum();
    let look = unpack(None, &pkg)?;
    let kept = ["images", "gifFiles"].iter().map(|k| look[*k].as_array().map_or(0, |a| a.len())).sum();
    Ok((listed, kept))
}

/// Reads, decrypts and unzips a .mdprofile, letting go of each step's copy as soon as it is done.
fn open_file(path: &Path) -> Result<package::Package, String> {
    let size = fs::metadata(path).map_err(|_| "That file could not be opened.".to_string())?.len();
    if size > MAX_FILE_BYTES {
        return Err("That file is too large.".into());
    }
    let bytes = fs::read(path).map_err(|_| "That file could not be opened.".to_string())?;
    let zip = package::open(&bytes, Kind::Profile)?;
    drop(bytes);
    package::read(&zip, Kind::Profile)
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
        // names Windows keeps for devices
        for name in ["CON", "con", "Nul", "AUX.txt", "COM1", "lpt9", "PRN "] {
            assert!(file_stem(name).ends_with(" profile"), "{name}: {}", file_stem(name));
        }
        assert_eq!(file_stem("Console"), "Console");
        assert_eq!(file_stem("COM10"), "COM10");
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("minniondeck-test-{tag}-{}", crate::backend::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_profile_survives_export_and_import() {
        let (from, to) = (temp_dir("from"), temp_dir("to"));
        fs::write(from.join("u-1-abc.png"), crate::backend::media::tiny_png()).unwrap();
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

        // looking at it writes nothing, and gives the same profile
        let look = unpack(None, &pkg).unwrap();
        assert!(fs::read_dir(&to).unwrap().next().is_none(), "nothing written by looking");
        let out = unpack(Some(&to), &pkg).unwrap();
        assert_eq!(look["profile"], out["profile"]);
        let tree = &out["profile"]["tree"];
        assert_eq!(tree[0]["buttons"][0]["steps"][0]["f"]["scene"], "BRB");
        assert_eq!(tree[0]["cols"], 4);
        assert_eq!(tree[1]["children"][0]["cols"], 5);
        assert_eq!(out["designs"][0]["id"], "d1");
        assert_eq!(out["drawnGifs"][0]["id"], "g1");
        assert_eq!(out["plugins"], json!(["OBS"]));
        let media = out["images"][0]["media"].as_str().unwrap();
        assert!(media.starts_with("imp-") && media.ends_with("u-1-abc.png"));
        assert_eq!(crate::backend::media::read_named(&to, media).unwrap(), crate::backend::media::tiny_png());
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

/// Opens a .mdprofile the Minniondeck app made, to check the two apps read each other's files:
///     MD_PROFILE="E:\...\Spotify.mdprofile" cargo test --release real_mdprofile -- --ignored --nocapture
#[cfg(test)]
mod real_file {
    use super::*;

    #[test]
    #[ignore]
    fn real_mdprofile_opens() {
        let path = std::env::var("MD_PROFILE").expect("set MD_PROFILE to a .mdprofile file");
        let dir = std::env::temp_dir().join(format!("md-real-profile-{}", crate::backend::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        let out = import_from(&dir, Path::new(&path)).unwrap();
        let pages = out["profile"]["tree"].as_array().map_or(0, |t| t.len());
        println!("{}: {} top-level pages/folders, plugins {}", out["manifest"]["name"], pages, out["plugins"]);
        assert!(pages > 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
