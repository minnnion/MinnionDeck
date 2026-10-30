// What a phone shows, worked out from the saved library.
//
// A phone shows one profile: the one holding the page it follows, or, when it follows the PC,
// the profile open on the PC. It gets that profile's whole folder tree with every page's
// buttons (so it can swipe between pages), the designs, drawn GIFs, pictures and GIF files
// those buttons use, how it should lay pages out, and a few Settings. Built-in icons, GIFs and
// widgets are referred to by name; the phone app carries the same set. Icons and GIFs from
// plugins ("plug:<plugin>:<file>") are copied into the media folder and listed in pluginMedia.
//
// The layout's "rev" is a hash of all of that, so a phone is sent a layout only when something
// it shows has changed. Which page is open is sent on its own, so turning pages is cheap.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde_json::{Value, json};

use super::hub::PhoneRecord;
use crate::storage::{safe_name, sha256_hex};

/// What the phone's layout is built from: the latest saves, and the page open on the PC
#[derive(Default)]
pub struct Sources {
    pub library: Option<Value>,
    pub settings: Value,
    pub pc_profile: Option<String>,
    pub pc_page: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaFile {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

pub struct Layout {
    pub rev: String,
    pub page: String,
    pub body: Value,
    pub media: Vec<MediaFile>,
    /// Buttons the phone may press, and files it may download
    pub buttons: HashSet<String>,
}

fn walk<'a>(nodes: &'a Value, out: &mut Vec<&'a Value>) {
    for n in nodes.as_array().into_iter().flatten() {
        if n["kind"] == "folder" {
            walk(&n["children"], out);
        } else if n["kind"] == "page" {
            out.push(n);
        }
    }
}

fn pages_of(profile: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    walk(&profile["tree"], &mut out);
    out
}

/// The tree with each page's buttons put inside it, as the phone gets it
fn tree_with_buttons(nodes: &Value, boards: &Value, buttons: &mut Vec<Value>) -> Value {
    let out: Vec<Value> = nodes
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| match n["kind"].as_str() {
            Some("folder") => Some(json!({ "kind": "folder", "id": n["id"], "name": n["name"], "children": tree_with_buttons(&n["children"], boards, buttons) })),
            Some("page") => {
                let list = n["board"].as_str().and_then(|b| boards.get(b)).cloned().unwrap_or_else(|| json!([]));
                buttons.extend(list.as_array().into_iter().flatten().cloned());
                Some(json!({ "kind": "page", "id": n["id"], "name": n["name"], "cols": n["cols"], "rows": n["rows"], "buttons": list }))
            }
            _ => None,
        })
        .collect();
    Value::Array(out)
}

fn by_ids(list: &Value, ids: &HashSet<String>) -> Value {
    Value::Array(list.as_array().into_iter().flatten().filter(|x| x["id"].as_str().is_some_and(|id| ids.contains(id))).cloned().collect())
}

/// Size and hash of a media file, remembered by name (names already include a content hash)
fn media_file(dir: &Path, name: &str, cache: &mut HashMap<String, (u64, String)>) -> Option<MediaFile> {
    let name = safe_name(name)?;
    if !cache.contains_key(name) {
        let bytes = fs::read(dir.join(name)).ok()?;
        cache.insert(name.to_string(), (bytes.len() as u64, sha256_hex(&bytes)));
    }
    let (bytes, sha256) = cache.get(name)?.clone();
    Some(MediaFile { file: name.to_string(), bytes, sha256 })
}

/// Copy a plugin's icon or GIF ("plug:<plugin>:<file>") into the media folder, where phones can
/// download it. Returns its name there. The media folder is <app data>\library\media and plugins
/// are in <app data>\plugins.
fn plugin_file(media_dir: &Path, id: &str) -> Option<String> {
    let mut parts = id.splitn(3, ':').skip(1);
    let (plugin, rel) = (parts.next()?, parts.next()?);
    if safe_name(plugin).is_none() || rel.split('/').any(|p| p.is_empty() || p.starts_with('.') || safe_name(p).is_none()) {
        return None;
    }
    let source = media_dir.parent()?.parent()?.join("plugins").join(plugin).join(rel);
    let meta = fs::metadata(&source).ok().filter(|m| m.is_file() && m.len() <= 8 * 1024 * 1024)?;
    // The name carries the file's size and time, so a plugin's new picture (a new version, the
    // same file name) is a new name here: media_file's cache is by name, and so is a phone's copy,
    // and with the old name both kept the old picture
    let stamp = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let name = format!("plugin-{plugin}-{}-{stamp:x}-{}", meta.len(), rel.replace('/', "_"));
    let dest = media_dir.join(&name);
    if !dest.is_file() {
        fs::copy(&source, &dest).ok()?;
    }
    Some(name)
}

pub fn layout(src: &Sources, phone: &PhoneRecord, media_dir: &Path, cache: &mut HashMap<String, (u64, String)>) -> Option<Layout> {
    let lib = src.library.as_ref()?;
    let profiles = lib["profiles"].as_array()?;
    let has_page = |p: &Value, page: &str| pages_of(p).iter().any(|n| n["id"] == page);

    // Which profile and page: the page this phone follows, else whatever the PC has open
    let followed = (phone.follows != "pc").then(|| profiles.iter().find(|p| has_page(p, &phone.follows))).flatten();
    let (profile, page) = match followed {
        Some(p) => (p, phone.follows.clone()),
        None => {
            let pc_prof = src.pc_profile.clone().or_else(|| lib["active"]["prof"].as_str().map(String::from));
            let p = profiles.iter().find(|p| p["id"].as_str() == pc_prof.as_deref()).or(profiles.first())?;
            let pc_page = src.pc_page.clone().or_else(|| lib["active"]["page"].as_str().map(String::from));
            let page = pc_page.filter(|pg| has_page(p, pg)).or_else(|| pages_of(p).first().and_then(|n| n["id"].as_str().map(String::from)))?;
            (p, page)
        }
    };

    let mut buttons = Vec::new();
    let tree = tree_with_buttons(&profile["tree"], &lib["boards"], &mut buttons);

    // Everything those buttons show, and nothing else
    let (mut designs, mut gifs, mut images) = (HashSet::new(), HashSet::new(), HashSet::new());
    let mut from_plugins = HashSet::new();
    let mut use_gif = |id: &Value| {
        if let Some(id) = id.as_str().filter(|id| *id != "none") {
            gifs.insert(id.to_string());
        }
    };
    for b in &buttons {
        for st in b["states"].as_array().into_iter().flatten() {
            if let Some(src) = st["bgImage"]["src"].as_str() {
                images.insert(src.to_string());
            }
            use_gif(&st["bgGif"]["id"]);
            use_gif(&st["gif"]["id"]);
            for id in [&st["glyph"], &st["gif"]["id"], &st["bgGif"]["id"]] {
                if let Some(id) = id.as_str().filter(|id| id.starts_with("plug:")) {
                    from_plugins.insert(id.to_string());
                }
            }
            if let Some(d) = st["design"].as_str() {
                designs.insert(d.to_string());
            }
        }
    }
    let used_designs = by_ids(&lib["designs"], &designs);
    // Each design has a face per button size: the open one (bg, layers) and the rest in "faces"
    let faces = used_designs.as_array().into_iter().flatten().flat_map(|d| std::iter::once(d).chain(d["faces"].as_object().into_iter().flat_map(|m| m.values())));
    for d in faces {
        if let Some(i) = d["bg"]["image"].as_str() {
            images.insert(i.to_string());
        }
        use_gif(&d["bg"]["gif"]);
        for l in d["layers"].as_array().into_iter().flatten() {
            if let Some(i) = l["image"].as_str() {
                images.insert(i.to_string());
            }
            use_gif(&l["gif"]);
            for id in [&l["glyph"], &l["gif"]] {
                if let Some(id) = id.as_str().filter(|id| id.starts_with("plug:")) {
                    from_plugins.insert(id.to_string());
                }
            }
        }
    }
    let images = by_ids(&lib["images"], &images);
    let gif_files = by_ids(&lib["gifFiles"], &gifs);
    let mut media = Vec::new();
    for item in images.as_array().into_iter().flatten().chain(gif_files.as_array().into_iter().flatten()) {
        if let Some(m) = item["media"].as_str().and_then(|n| media_file(media_dir, n, cache)) {
            if !media.contains(&m) {
                media.push(m);
            }
        }
    }

    // Plugin icons and GIFs, copied into the media folder so phones download them like any picture
    let mut plugin_media = serde_json::Map::new();
    let mut ids: Vec<&String> = from_plugins.iter().collect();
    ids.sort();
    for id in ids {
        if let Some(name) = plugin_file(media_dir, id) {
            if let Some(m) = media_file(media_dir, &name, cache) {
                if !media.contains(&m) {
                    media.push(m);
                }
                plugin_media.insert(id.clone(), json!(name));
            }
        }
    }

    let s = &src.settings;
    let body = json!({
        "profile": { "id": profile["id"], "name": profile["name"], "body": profile["body"], "icon": profile["icon"] },
        "tree": tree,
        "designs": used_designs,
        // GIFs drawn in the GIF maker, and the built-in ones the buttons use, drawn out as frames
        "drawnGifs": by_ids(&Value::Array(lib["drawnGifs"].as_array().into_iter().chain(lib["phoneGifs"].as_array()).flatten().cloned().collect()), &gifs),
        "images": images,
        "gifFiles": gif_files,
        "pluginMedia": plugin_media,
        "view": { "follows": phone.follows, "orient": phone.orient, "fit": phone.fit, "grid": phone.grid },
        "prefs": { "names": s["names"], "backButton": s["backBtn"], "keepAwake": s["awake"], "haptics": s["haptics"], "animations": s["anim"] },
    });
    let rev = sha256_hex(format!("{body}{}", serde_json::to_string(&media).unwrap_or_default()).as_bytes())[..16].to_string();
    let buttons = buttons.iter().filter_map(|b| b["id"].as_str().map(String::from)).collect();
    Some(Layout { rev, page, body, media, buttons })
}
