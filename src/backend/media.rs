//! Your pictures and GIFs: the ones made on the Minniondeck website (or anywhere) and brought in
//! with Import, kept in library\media, and listed in library.json the way the Minniondeck app
//! lists them:
//!
//!   images:    { id: "u:…", name, media: "<file in library\media>", ar }
//!   gifFiles:  { id: "f…", name, desc, kind: "gif", group: "GIFs", file: true, w, h, n: 1, ms: 100, media }
//!   drawnGifs: { id: "g…", name, size, fps, frames: [[ "#rrggbb" | null, … size × size ], …] }
//!
//! A button look points at them: a picture with bgType "image" and bgImage { src: <image id>,
//! fit: "cover" | "contain" }, a GIF with gif { id, place: "bg" | "icon", speed }.
//!
//! The lists are kept here for the Edit button window and for drawing; the library fills them.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde_json::{Value, json};

use super::util::{now_ms, safe_name, sha256_hex};

/// The Minniondeck website, where pictures and GIFs are made.
pub const SITE: &str = "https://minniondeck.github.io";
const MAX_MEDIA_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct Picture {
    pub id: String,
    pub name: String,
    /// The file in library\media, or None when it is missing
    pub file: Option<PathBuf>,
    /// Width / height
    pub ar: f32,
}

#[derive(Clone, Debug)]
pub enum GifSource {
    /// A .gif file in library\media (or a plugin's folder)
    File(PathBuf),
    /// Drawn in the GIF maker: size × size pixels, `fps` frames a second
    Drawn { size: usize, fps: f32, frames: Vec<Vec<Option<[u8; 4]>>> },
}

#[derive(Clone, Debug)]
pub struct Gif {
    pub id: String,
    pub name: String,
    pub src: Option<GifSource>,
    /// "GIFs", or a plugin's name
    pub group: String,
}

struct Lists {
    pictures: Vec<Picture>,
    gifs: Vec<Gif>,
    stamp: u64,
}

static LISTS: RwLock<Lists> = RwLock::new(Lists { pictures: Vec::new(), gifs: Vec::new(), stamp: 0 });

pub fn pictures() -> Vec<Picture> {
    LISTS.read().map(|l| l.pictures.clone()).unwrap_or_default()
}

pub fn gifs() -> Vec<Gif> {
    LISTS.read().map(|l| l.gifs.clone()).unwrap_or_default()
}

pub fn picture(id: &str) -> Option<Picture> {
    LISTS.read().ok()?.pictures.iter().find(|p| p.id == id).cloned()
}

pub fn gif(id: &str) -> Option<Gif> {
    LISTS.read().ok()?.gifs.iter().find(|g| g.id == id).cloned()
}

/// Changes whenever the lists do (for caches).
pub fn stamp() -> u64 {
    LISTS.read().map(|l| l.stamp).unwrap_or(0)
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

/// The file for a media name ("pic.png"): the sealed one ("pic.png.mdmedia"), or a plain one an
/// earlier version left.
pub fn file_for(media_dir: &Path, name: &str) -> Option<PathBuf> {
    let name = safe_name(name)?;
    let sealed = media_dir.join(format!("{name}.{}", super::vault::MEDIA_EXT));
    if sealed.is_file() {
        return Some(sealed);
    }
    let plain = media_dir.join(name);
    plain.is_file().then_some(plain)
}

fn media_file(media_dir: &Path, name: &str) -> Option<PathBuf> {
    file_for(media_dir, name)
}

/// What is in a media file: opened when it is sealed, as it is when it isn't (a plugin's own GIF).
pub fn read_bytes(path: &Path) -> Option<Vec<u8>> {
    if path.extension().and_then(|e| e.to_str()) == Some(super::vault::MEDIA_EXT) {
        super::vault::read(path, super::package::Kind::Media).ok()
    } else {
        fs::read(path).ok()
    }
}

/// What is in the media file called `name`.
pub fn read_named(media_dir: &Path, name: &str) -> Option<Vec<u8>> {
    read_bytes(&file_for(media_dir, name)?)
}

/// Keeps a picture or GIF sealed in the media folder under its name.
pub fn store(media_dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let name = safe_name(name).ok_or("That picture's name isn't allowed.")?;
    fs::create_dir_all(media_dir).map_err(|e| e.to_string())?;
    super::vault::write(&media_dir.join(format!("{name}.{}", super::vault::MEDIA_EXT)), super::package::Kind::Media, bytes)
}

/// The name of the editable design kept beside the picture `name`.
fn design_name(name: &str) -> String {
    format!("{name}.design.json")
}

/// Keeps the design (its layers, as the editor has them) a picture was made from, so the picture
/// can be edited in layers again.
pub fn store_design(media_dir: &Path, name: &str, design: &[u8]) -> Result<(), String> {
    store(media_dir, &design_name(name), design)
}

/// The design kept beside this picture file, if it was made from one.
pub fn design_for(picture: &Path) -> Option<Vec<u8>> {
    let file = picture.file_name()?.to_str()?;
    let name = file.strip_suffix(&format!(".{}", super::vault::MEDIA_EXT)).unwrap_or(file);
    read_named(picture.parent()?, &design_name(name))
}

/// Every picture and GIF there is as (its name, its bytes), for a backup.
pub fn all_files(media_dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = vec![];
    let Ok(rd) = fs::read_dir(media_dir) else { return Ok(out) };
    for e in rd.flatten() {
        let file = e.file_name().to_string_lossy().into_owned();
        if !e.path().is_file() || file.ends_with(".tmp") {
            continue;
        }
        let name = file.strip_suffix(&format!(".{}", super::vault::MEDIA_EXT)).unwrap_or(&file).to_string();
        if safe_name(&name).is_none() {
            continue;
        }
        let bytes = read_bytes(&e.path()).ok_or_else(|| format!("Could not read {file}."))?;
        out.push((name, bytes));
    }
    out.sort();
    Ok(out)
}

/// Seals the pictures an earlier version left as plain files. Returns what couldn't be changed.
pub fn seal_folder(media_dir: &Path) -> Vec<String> {
    let mut problems = vec![];
    let Ok(rd) = fs::read_dir(media_dir) else { return problems };
    for e in rd.flatten() {
        let path = e.path();
        let file = e.file_name().to_string_lossy().into_owned();
        if !path.is_file() || file.ends_with(".tmp") || super::vault::is_sealed(&path) || safe_name(&file).is_none() {
            continue;
        }
        if let Err(err) = super::vault::migrate(&path, &media_dir.join(format!("{file}.{}", super::vault::MEDIA_EXT)), super::package::Kind::Media) {
            problems.push(err);
        }
    }
    problems
}

/// "#rrggbb" (or "#rgb", "#rrggbbaa") to RGBA
pub fn hex(c: &str) -> Option<[u8; 4]> {
    let h = c.strip_prefix('#')?;
    let n = |i: usize, l: usize| u8::from_str_radix(h.get(i..i + l)?, 16).ok();
    match h.len() {
        3 => Some([n(0, 1)? * 17, n(1, 1)? * 17, n(2, 1)? * 17, 255]),
        6 => Some([n(0, 2)?, n(2, 2)?, n(4, 2)?, 255]),
        8 => Some([n(0, 2)?, n(2, 2)?, n(4, 2)?, n(6, 2)?]),
        _ => None,
    }
}

fn drawn(v: &Value) -> Option<GifSource> {
    let size = v.get("size").and_then(|x| x.as_u64()).unwrap_or(16).clamp(1, 128) as usize;
    let fps = v.get("fps").and_then(|x| x.as_f64()).unwrap_or(8.0).clamp(0.5, 60.0) as f32;
    let frames: Vec<Vec<Option<[u8; 4]>>> = v
        .get("frames")?
        .as_array()?
        .iter()
        .take(1000)
        .map(|f| {
            let mut px: Vec<Option<[u8; 4]>> = f.as_array().map(|a| a.iter().take(size * size).map(|c| c.as_str().and_then(hex)).collect()).unwrap_or_default();
            px.resize(size * size, None);
            px
        })
        .collect();
    (!frames.is_empty()).then_some(GifSource::Drawn { size, fps, frames })
}

/// Makes the lists from library.json's "data" and the plugins' own GIFs.
pub fn set_from_doc(doc: &Value, media_dir: &Path, plugins_dir: &Path) {
    let arr = |k: &str| doc.get(k).and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let pictures = arr("images")
        .iter()
        .filter(|e| !s(e, "id").is_empty())
        .map(|e| Picture {
            id: s(e, "id"),
            name: match s(e, "name") { n if n.is_empty() => "Picture".into(), n => n },
            file: media_file(media_dir, &s(e, "media")),
            ar: e.get("ar").and_then(|a| a.as_f64()).filter(|a| a.is_finite() && *a > 0.0).unwrap_or(1.0) as f32,
        })
        .collect();
    let mut gifs: Vec<Gif> = vec![];
    for e in arr("drawnGifs").iter().chain(arr("phoneGifs").iter()) {
        if s(e, "id").is_empty() || gifs.iter().any(|g| g.id == s(e, "id")) {
            continue;
        }
        gifs.push(Gif { id: s(e, "id"), name: s(e, "name"), src: drawn(e), group: "GIFs".into() });
    }
    for e in arr("gifFiles").iter().filter(|e| !s(e, "id").is_empty()) {
        gifs.push(Gif { id: s(e, "id"), name: s(e, "name"), src: media_file(media_dir, &s(e, "media")).map(GifSource::File), group: "GIFs".into() });
    }
    // a plugin's GIFs (manifest "gifs"): plug:<plugin>:<file>
    if let Ok(rd) = fs::read_dir(plugins_dir) {
        let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for dir in dirs {
            let Some(id) = dir.file_name().and_then(|n| n.to_str()).filter(|n| safe_name(n).is_some()).map(String::from) else { continue };
            let Ok(text) = fs::read_to_string(dir.join("manifest.json")) else { continue };
            let Ok(m) = serde_json::from_str::<Value>(&text) else { continue };
            let pname = match s(&m, "name") { n if n.is_empty() => id.clone(), n => n };
            for f in m.get("gifs").and_then(|g| g.as_array()).into_iter().flatten().filter_map(|f| f.as_str()) {
                if !super::plugins::safe_rel(f) || !f.to_ascii_lowercase().ends_with(".gif") {
                    continue;
                }
                let path = dir.join(f);
                let name = Path::new(f).file_stem().and_then(|x| x.to_str()).unwrap_or(f).replace(['_', '-'], " ");
                gifs.push(Gif { id: format!("plug:{id}:{f}"), name, src: path.is_file().then_some(GifSource::File(path)), group: pname.clone() });
            }
        }
    }
    if let Ok(mut l) = LISTS.write() {
        l.pictures = pictures;
        l.gifs = gifs;
        l.stamp = l.stamp.wrapping_add(1).max(now_ms());
    }
}

/// What Import brings in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Picture,
    Gif,
}

/// Copies a picture or GIF into library\media and returns its entry for library.json
/// (images or gifFiles). Pictures may be PNG, JPEG, GIF or WebP; GIFs must be GIF files.
pub fn import(media_dir: &Path, path: &Path, kind: Kind) -> Result<Value, String> {
    let size = fs::metadata(path).map_err(|_| "That file could not be opened.".to_string())?.len();
    if size > MAX_MEDIA_BYTES {
        return Err("That file is larger than 25 MB.".into());
    }
    let bytes = fs::read(path).map_err(|_| "That file could not be opened.".to_string())?;
    let name: String = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Picture").chars().take(24).collect();
    import_bytes(media_dir, &bytes, &name, kind)
}

/// The same, for a picture or GIF that arrived as bytes (the website's Sync), called `name`.
pub fn import_bytes(media_dir: &Path, bytes: &[u8], name: &str, kind: Kind) -> Result<Value, String> {
    if bytes.len() as u64 > MAX_MEDIA_BYTES {
        return Err("That file is larger than 25 MB.".into());
    }
    let ext = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => "png",
        Ok(image::ImageFormat::Jpeg) => "jpg",
        Ok(image::ImageFormat::Gif) => "gif",
        Ok(image::ImageFormat::WebP) => "webp",
        _ => return Err("That isn't a picture Minniondeck can use. Use a PNG or GIF (the website makes both), a JPEG or a WebP.".into()),
    };
    if kind == Kind::Gif && ext != "gif" {
        return Err("That isn't a GIF file. Make one on the website with Download GIF.".into());
    }
    // it must open, so nothing is kept that can't be drawn
    let (w, h) = image::load_from_memory(bytes).map(|i| (i.width(), i.height())).map_err(|_| "That picture is damaged and couldn't be opened.".to_string())?;
    let name: String = name.chars().take(24).collect();
    let id = match kind {
        Kind::Picture => format!("u:{}", base36(now_ms())),
        Kind::Gif => format!("f{}", base36(now_ms())),
    };
    let stem: String = id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).take(40).collect();
    let file = format!("{stem}-{}.{ext}", &sha256_hex(bytes)[..12]);
    if file_for(media_dir, &file).is_none() {
        store(media_dir, &file, bytes)?;
    }
    Ok(match kind {
        Kind::Picture => json!({ "id": id, "name": name, "media": file, "ar": w as f64 / h.max(1) as f64 }),
        Kind::Gif => json!({ "id": id, "name": name, "desc": "Imported GIF file", "kind": "gif", "group": "GIFs", "file": true, "w": w, "h": h, "n": 1, "ms": 100, "media": file }),
    })
}

fn base36(mut t: u64) -> String {
    let mut s = String::new();
    while t > 0 {
        s.insert(0, char::from_digit((t % 36) as u32, 36).unwrap_or('0'));
        t /= 36;
    }
    s
}


/// Tests that fill the shared lists take turns.
#[cfg(test)]
pub static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 200, 30, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn pictures_and_gifs_import_into_media_with_the_apps_fields() {
        let _one = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("md-media-test-{}", now_ms()));
        let media = dir.join("media");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("Sunset.png"), png_bytes(32, 16)).unwrap();
        let e = import(&media, &dir.join("Sunset.png"), Kind::Picture).unwrap();
        assert!(e["id"].as_str().unwrap().starts_with("u:"));
        assert_eq!(e["name"], "Sunset");
        assert_eq!(e["ar"], 2.0);
        let file = e["media"].as_str().unwrap();
        assert!(file.ends_with(".png") && media.join(format!("{file}.mdmedia")).is_file() && !media.join(file).exists(), "kept sealed");
        assert_eq!(read_named(&media, file).unwrap(), png_bytes(32, 16));
        // a PNG is not a GIF; text is not a picture
        assert!(import(&media, &dir.join("Sunset.png"), Kind::Gif).unwrap_err().contains("GIF"));
        fs::write(dir.join("notes.txt"), b"hello").unwrap();
        assert!(import(&media, &dir.join("notes.txt"), Kind::Picture).is_err());
        // a GIF file
        let mut gbuf = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255])).write_to(&mut gbuf, image::ImageFormat::Gif).unwrap();
        fs::write(dir.join("Blink.gif"), gbuf.into_inner()).unwrap();
        let g = import(&media, &dir.join("Blink.gif"), Kind::Gif).unwrap();
        assert!(g["id"].as_str().unwrap().starts_with('f'));
        assert_eq!((g["kind"].as_str(), g["file"].as_bool(), g["w"].as_u64(), g["n"].as_u64(), g["ms"].as_u64()), (Some("gif"), Some(true), Some(8), Some(1), Some(100)));
        // the lists, from library.json
        let doc = json!({ "images": [e], "gifFiles": [g], "drawnGifs": [{ "id": "g1", "name": "Ball", "size": 2, "fps": 4, "frames": [["#ff0000", null, null, "#00ff00"]] }] });
        set_from_doc(&doc, &media, &dir.join("plugins"));
        assert!(picture(e["id"].as_str().unwrap()).unwrap().file.is_some());
        let ball = gif("g1").unwrap();
        match ball.src {
            Some(GifSource::Drawn { size, fps, frames }) => {
                assert_eq!((size, fps, frames.len()), (2, 4.0, 1));
                assert_eq!(frames[0][0], Some([255, 0, 0, 255]));
                assert_eq!(frames[0][1], None);
            }
            _ => panic!("a drawn GIF"),
        }
        assert!(matches!(gif(g["id"].as_str().unwrap()).unwrap().src, Some(GifSource::File(_))));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn colours_read_like_css() {
        assert_eq!(hex("#ff8800"), Some([255, 136, 0, 255]));
        assert_eq!(hex("#f80"), Some([255, 136, 0, 255]));
        assert_eq!(hex("red"), None);
    }
}
