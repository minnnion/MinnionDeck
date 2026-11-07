//! The library: profiles, folders, pages and buttons, kept sealed in library\library.mdsave. Inside
//! it is the Minniondeck app's format ({ "schema_version": 1, "saved_at": <ms>, "data": {...} }). Whatever the window doesn't use yet (designs, GIFs, styles,
//! a button's later steps...) is kept exactly as it was read.
//!
//! The window works on a `Sample`: the profiles, and the pages and buttons of the profile in
//! use. `tick` runs every frame: it keeps the grid size with the page, gives new profiles,
//! pages and buttons their ids, loads another profile's pages when one is picked, and saves a
//! moment after anything changes. Before the first save of a session the file is copied to
//! backups\ (the 5 newest are kept).
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Map, Value, json};

use super::actions::Step;
use super::model::{Look, Media, PageRow, Profile, Sample, Tile, auto_index, auto_name};
use super::paths::Paths;

/// Saved this long after the last change.
const SAVE_AFTER: f64 = 0.6;
/// A design's background when it has none: the dark of the plugins' own designs.
const DESIGN_BG: &str = "#2d3142";

pub struct Library {
    pub paths: Paths,
    /// library.json's "data".
    doc: Value,
    /// The profile whose pages are in the Sample, and the page whose grid size is shown.
    loaded: String,
    page_shown: usize,
    /// Each button and each page or folder of that profile as read, by id, so what the window
    /// doesn't know is kept.
    buttons: HashMap<String, Value>,
    nodes: HashMap<String, Value>,
    profiles_sig: u64,
    /// The fingerprint last written, the last one seen and when it changed.
    saved: u64,
    seen: u64,
    changed_at: f64,
    /// Something only the file holds changed (a profile trusted), so save even if the window didn't.
    force: bool,
    backed_up: bool,
    /// "Back up when Minniondeck starts" (Settings › Backup)
    pub auto_backup: bool,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A new id, like the Minniondeck app's: a prefix, the time in base 36 and a counter.
fn uid(prefix: &str) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    let mut t = now_ms();
    let mut s = String::new();
    while t > 0 {
        let d = (t % 36) as u32;
        s.insert(0, char::from_digit(d, 36).unwrap_or('0'));
        t /= 36;
    }
    format!("{prefix}{s}{}", N.fetch_add(1, Ordering::Relaxed))
}

fn hash_of<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn fingerprint(s: &Sample) -> u64 {
    hash_of(&(&s.profiles, &s.pages, &s.tiles, s.active, s.page))
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn num_of(v: &Value, key: &str, def: usize) -> usize {
    v.get(key).and_then(|x| x.as_u64()).map(|n| n as usize).unwrap_or(def)
}

/// A new, empty library: "My profile" with "Page 1", as the Minniondeck app starts.
fn default_doc() -> Value {
    json!({
        "profiles": [{ "id": "mine", "name": "My profile", "body": "sky", "icon": "star", "auto": "Never",
            "tree": [{ "kind": "page", "id": "p-1", "name": "Page 1", "board": "Board-1", "cols": 5, "rows": 3 }] }],
        "boards": { "Board-1": [] },
        "designs": [],
        "active": { "prof": "mine", "page": "p-1" }
    })
}

fn default_button() -> Value {
    json!({ "id": "", "label": "", "x": 0, "y": 0, "w": 1, "h": 1, "mode": "single", "steps": [],
        "style": { "shape": "full", "textPos": "bottom" }, "states": [], "cur": 0,
        "anim": { "type": "press", "speed": "normal", "repeat": "once", "phone": true } })
}

fn default_state() -> Value {
    json!({ "name": "Default", "text": "", "glyph": "bolt", "gif": { "id": "none", "place": "bg", "speed": "normal" }, "bgType": "colour", "bg": "tile" })
}

/// The press animation's name in the file ↔ its index in ui::anim::NAMES.
fn anim_index(name: &str) -> usize {
    crate::ui::anim::NAMES.iter().position(|n| n.to_lowercase().replace(' ', "") == name).unwrap_or(if name.is_empty() { 1 } else { 0 })
}

fn anim_name(i: usize) -> String {
    crate::ui::anim::NAMES.get(i).map(|n| n.to_lowercase().replace(' ', "")).unwrap_or_else(|| "press".into())
}

/// A picture as the file keeps it → as the window draws it. Plugin pictures
/// ("plug:minnion_spotify:icons/sp-next.png") are the ones built in from assets\actions.
fn icon_from_real(g: &str) -> String {
    if g.is_empty() {
        return "bolt".into();
    }
    if let Some(rest) = g.strip_prefix("plug:") {
        if let Some((pid, file)) = rest.split_once(':') {
            let name = file.rsplit('/').next().unwrap_or(file).trim_end_matches(".png");
            let key = format!("{pid}/{name}");
            if crate::backend::catalog::has_picture(&key) {
                return format!("png:{key}");
            }
        }
        return "bolt".into();
    }
    g.to_string()
}

/// The other way; a picture that wasn't changed keeps its exact old name.
fn icon_to_real(icon: &str, old: &str) -> String {
    if !old.is_empty() && icon_from_real(old) == icon {
        return old.to_string();
    }
    match icon.strip_prefix("png:").and_then(|k| k.split_once('/')) {
        Some((pid, name)) => format!("plug:{pid}:icons/{name}.png"),
        None => icon.to_string(),
    }
}

/// The colour a look shows: its own colour, or its design's background (for the button's size).
fn colour_of(state: &Value, designs: &HashMap<String, Value>, size: &str) -> String {
    if str_of(state, "bgType") == "design" {
        let d = designs.get(str_of(state, "design"));
        let face = d.and_then(|d| d.get("faces")).and_then(|f| f.get(size)).or(d);
        let c = face.and_then(|f| f.get("bg")).map(|b| str_of(b, "colour")).unwrap_or("");
        return if c.starts_with('#') { c.to_string() } else { DESIGN_BG.into() };
    }
    match str_of(state, "bg") {
        "" => "tile".into(),
        c => c.to_string(),
    }
}

/// Fills a page list from a profile's tree (folders open in place, `depth` deep).
fn flatten(nodes: &[Value], depth: usize, out: &mut Vec<PageRow>, keep: &mut HashMap<String, Value>) {
    for n in nodes {
        let id = str_of(n, "id").to_string();
        keep.insert(id.clone(), n.clone());
        if str_of(n, "kind") == "folder" {
            out.push(PageRow {
                id,
                board: String::new(),
                name: str_of(n, "name").into(),
                folder: true,
                depth,
                open: n.get("open").and_then(|v| v.as_bool()).unwrap_or(true),
                count: 0,
                cols: 5,
                rows: 3,
            });
            if let Some(ch) = n.get("children").and_then(|c| c.as_array()) {
                flatten(ch, depth + 1, out, keep);
            }
        } else {
            out.push(PageRow {
                id,
                board: str_of(n, "board").into(),
                name: str_of(n, "name").into(),
                folder: false,
                depth,
                open: true,
                count: 0,
                cols: num_of(n, "cols", 5).clamp(1, 12),
                rows: num_of(n, "rows", 3).clamp(1, 12),
            });
        }
    }
}

/// The tree again from a page list. A row deeper than the folder before it goes inside it.
fn build_tree(rows: &[PageRow], i: &mut usize, depth: usize, keep: &HashMap<String, Value>) -> Vec<Value> {
    let mut out = vec![];
    while *i < rows.len() && rows[*i].depth >= depth {
        let r = &rows[*i];
        *i += 1;
        let mut n = keep.get(&r.id).cloned().filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
        let o = n.as_object_mut().expect("object");
        o.insert("kind".into(), json!(if r.folder { "folder" } else { "page" }));
        o.insert("id".into(), json!(r.id));
        o.insert("name".into(), json!(r.name));
        if r.folder {
            o.insert("open".into(), json!(r.open));
            let children = build_tree(rows, i, r.depth + 1, keep);
            o.insert("children".into(), Value::Array(children));
        } else {
            o.insert("board".into(), json!(r.board));
            o.insert("cols".into(), json!(r.cols));
            o.insert("rows".into(), json!(r.rows));
        }
        out.push(n);
    }
    out
}

/// What a profile holds: (pages, folders, buttons).
fn counts(tree: &[Value], boards: &Value) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for n in tree {
        if str_of(n, "kind") == "folder" {
            c.1 += 1;
            let k = counts(n.get("children").and_then(|v| v.as_array()).map(|v| v.as_slice()).unwrap_or(&[]), boards);
            c = (c.0 + k.0, c.1 + k.1, c.2 + k.2);
        } else {
            c.0 += 1;
            c.2 += boards.get(str_of(n, "board")).and_then(|b| b.as_array()).map(|b| b.len()).unwrap_or(0);
        }
    }
    c
}

/// Reads the library's "data" (from a sealed .mdsave file, or the plain library.json of the
/// Minniondeck app), or says why it can't be used.
fn read_doc(file: &Path) -> Result<Value, String> {
    let v: Value = if super::vault::is_sealed(file) {
        super::vault::read_json(file, super::package::Kind::Save)?
    } else {
        let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| format!("it isn't valid JSON ({e})"))?
    };
    let data = v.get("data").cloned().unwrap_or(v);
    let ok = data.get("profiles").and_then(|p| p.as_array()).map(|p| !p.is_empty()).unwrap_or(false) && data.get("boards").map(|b| b.is_object()).unwrap_or(false);
    if !ok {
        return Err("it has no profiles in it".into());
    }
    Ok(data)
}

/// Was: first start copied the old Tauri app's library. A new install now starts empty.
#[allow(dead_code)]
fn copy_from_old_app(p: &Paths) -> bool {
    let old_app_root = super::paths::old_app_root;
    let Some(old) = old_app_root() else { return false };
    let lib = old.join("library").join("library.json");
    // read as it is, then sealed: nothing of yours is left as a plain file here
    let Some(v) = std::fs::read_to_string(&lib).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { return false };
    if read_doc(&lib).is_err() || super::vault::write_json(&p.library_file(), super::package::Kind::Save, &v).is_err() {
        return false;
    }
    if let Some(s) = std::fs::read_to_string(old.join("settings.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
        let _ = super::vault::write_json(&p.settings_file(), super::package::Kind::Save, &s);
    }
    if let Ok(dir) = std::fs::read_dir(old.join("library").join("media")) {
        for e in dir.flatten() {
            if let (true, Some(name), Ok(bytes)) = (e.path().is_file(), e.file_name().to_str().map(String::from), std::fs::read(e.path())) {
                let _ = super::media::store(&p.media, &name, &bytes);
            }
        }
    }
    true
}

impl Library {
    /// Finds the data folder, reads the library (copying yours from the Minniondeck app the first
    /// time) and returns it with what the window shows, plus a message for the user, if any.
    pub fn open() -> (Library, Sample, Option<String>) {
        let paths = Paths::find();
        let mut note = paths.make_folders().err();
        // what an earlier version left as plain files is sealed first
        if let Some(problem) = paths.migrate_legacy() {
            note = Some(problem);
        }
        let file = paths.library_file();
        let doc = if file.exists() {
            match read_doc(&file) {
                Ok(d) => d,
                Err(e) => {
                    // keep the unreadable file aside, never overwrite it
                    let aside = paths.library.join(format!("library-unreadable-{}.mdsave", now_ms()));
                    let _ = std::fs::rename(&file, &aside);
                    note = Some(format!("Your library couldn't be read ({e}). It was kept as {}.", aside.file_name().and_then(|n| n.to_str()).unwrap_or("")));
                    default_doc()
                }
            }
        } else {
            default_doc()
        };
        let mut lib = Library {
            paths,
            doc,
            loaded: String::new(),
            page_shown: 0,
            buttons: HashMap::new(),
            nodes: HashMap::new(),
            profiles_sig: 0,
            saved: 0,
            seen: 0,
            changed_at: 0.0,
            force: false,
            backed_up: false,
            auto_backup: true,
        };
        let mut s = Sample::empty();
        s.profiles = lib.profiles();
        let want = str_of(&lib.doc["active"], "prof").to_string();
        s.active = s.profiles.iter().position(|p| p.id == want).unwrap_or(0);
        let page = str_of(&lib.doc["active"], "page").to_string();
        lib.load_pages(&mut s, &page);
        lib.profiles_sig = hash_of(&s.profiles);
        lib.saved = fingerprint(&s);
        lib.seen = lib.saved;
        lib.publish_media();
        (lib, s, note)
    }

    fn designs(&self) -> HashMap<String, Value> {
        self.doc.get("designs").and_then(|d| d.as_array()).map(|a| a.iter().map(|d| (str_of(d, "id").to_string(), d.clone())).collect()).unwrap_or_default()
    }

    fn profiles(&self) -> Vec<Profile> {
        let boards = &self.doc["boards"];
        self.doc["profiles"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|p| {
                        let tree = p.get("tree").and_then(|t| t.as_array()).map(|t| t.as_slice()).unwrap_or(&[]);
                        let (pages, folders, buttons) = counts(tree, boards);
                        Profile {
                            id: str_of(p, "id").into(),
                            name: str_of(p, "name").into(),
                            colour: match str_of(p, "body") { "" => "sky".into(), c => c.into() },
                            icon: match str_of(p, "icon") { "" => "star".into(), c => c.into() },
                            auto: auto_index(str_of(p, "auto")),
                            pages,
                            folders,
                            buttons,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn profile_index(&self, id: &str) -> Option<usize> {
        self.doc["profiles"].as_array()?.iter().position(|p| str_of(p, "id") == id)
    }

    /// Puts the pages and buttons of the profile in use into the Sample, showing `page` (an id)
    /// when it is one of them.
    fn load_pages(&mut self, s: &mut Sample, page: &str) {
        let Some(prof) = s.profiles.get(s.active).cloned() else { return };
        let tree: Vec<Value> = self.profile_index(&prof.id).and_then(|i| self.doc["profiles"][i]["tree"].as_array().cloned()).unwrap_or_default();
        self.nodes.clear();
        self.buttons.clear();
        s.pages.clear();
        s.tiles.clear();
        flatten(&tree, 0, &mut s.pages, &mut self.nodes);
        if !s.pages.iter().any(|p| !p.folder) {
            s.pages.push(PageRow::new_page("Page 1"));
        }
        let designs = self.designs();
        for (pi, pg) in s.pages.clone().iter().enumerate() {
            if pg.folder {
                continue;
            }
            let list = self.doc["boards"].get(&pg.board).and_then(|b| b.as_array()).cloned().unwrap_or_default();
            for b in &list {
                let t = tile_from(b, pi, &designs);
                self.buttons.insert(t.id.clone(), b.clone());
                s.tiles.push(t);
            }
            s.pages[pi].count = list.len();
        }
        s.page = s.pages.iter().position(|p| !p.folder && p.id == page).or_else(|| s.pages.iter().position(|p| !p.folder)).unwrap_or(0);
        s.cols = s.pages[s.page].cols;
        s.rows = s.pages[s.page].rows;
        self.page_shown = s.page;
        self.loaded = prof.id;
    }

    /// Writes the Sample's pages and buttons back into the profile they came from, giving new
    /// pages, folders and buttons their ids.
    fn store_pages(&mut self, s: &mut Sample) {
        let Some(pi) = self.profile_index(&self.loaded) else { return };
        for r in s.pages.iter_mut() {
            if r.id.is_empty() {
                r.id = uid(if r.folder { "f-" } else { "p-" });
            }
            if !r.folder && r.board.is_empty() {
                r.board = uid("Board");
            }
        }
        let tree = build_tree(&s.pages, &mut 0, 0, &self.nodes);
        self.doc["profiles"][pi]["tree"] = Value::Array(tree);
        let ids: HashMap<String, String> = s.pages.iter().filter(|p| !p.folder).map(|p| (p.name.clone(), p.id.clone())).collect();
        let designs = self.designs();
        for (k, pg) in s.pages.iter().enumerate() {
            if pg.folder {
                continue;
            }
            let mut list = vec![];
            for t in s.tiles.iter_mut().filter(|t| t.page == k) {
                if t.id.is_empty() {
                    t.id = uid("t-");
                }
                let b = self.button_to_real(t, &ids, &designs);
                self.buttons.insert(t.id.clone(), b.clone());
                list.push(b);
            }
            self.doc["boards"][&pg.board] = Value::Array(list);
        }
    }

    /// A button as the file keeps it: the one read (or the one it was copied from) with what
    /// the window changed put in.
    fn button_to_real(&self, t: &Tile, page_ids: &HashMap<String, String>, designs: &HashMap<String, Value>) -> Value {
        let mut b = self.buttons.get(&t.id).or_else(|| self.buttons.get(&t.src)).cloned().unwrap_or_else(default_button);
        let size = format!("{}x{}", t.w, t.h);
        let o = b.as_object_mut().expect("object");
        o.insert("id".into(), json!(t.id));
        o.insert("label".into(), json!(t.label));
        o.insert("x".into(), json!(t.col));
        o.insert("y".into(), json!(t.row));
        o.insert("w".into(), json!(t.w));
        o.insert("h".into(), json!(t.h));
        o.insert("cur".into(), json!(t.cur));
        // the first step is what the window edits; any later steps stay
        let mut steps: Vec<Value> = o.get("steps").and_then(|s| s.as_array()).cloned().unwrap_or_default();
        if !t.action.is_empty() {
            let mut first = steps.first().cloned().filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
            let old_f = first.get("f").cloned().unwrap_or_else(|| json!({}));
            let mut f = Map::new();
            for (k, v) in &t.values {
                // a page made this session is known by its name until it has an id
                let v = if t.action == "page" && k == "page" { page_ids.get(v).cloned().unwrap_or_else(|| v.clone()) } else { v.clone() };
                // a number or switch read from the file stays one when it wasn't changed
                let keep = old_f.get(k).filter(|ov| !ov.is_string() && ov.to_string() == v).cloned();
                f.insert(k.clone(), keep.unwrap_or(Value::String(v)));
            }
            if first.get("type").and_then(|x| x.as_str()) != Some(t.action.as_str()) {
                first = json!({});
            }
            first["type"] = json!(t.action);
            // a step that had no settings stays without
            if !f.is_empty() || first.get("f").is_some() {
                first["f"] = Value::Object(f);
            }
            if steps.is_empty() {
                steps.push(first);
            } else {
                steps[0] = first;
            }
        }
        o.insert("steps".into(), Value::Array(steps));
        // the looks
        let old_states: Vec<Value> = o.get("states").and_then(|s| s.as_array()).cloned().unwrap_or_default();
        let mut looks: Vec<(String, String, String, String, Media)> = vec![(t.first_name.clone(), String::new(), if t.image.is_empty() { t.icon.clone() } else { t.image.clone() }, t.colour.clone(), t.media.clone())];
        looks.extend(t.more.iter().map(|l| (l.name.clone(), l.label.clone(), l.icon.clone(), l.colour.clone(), l.media.clone())));
        let mut states = vec![];
        for (i, (name, text, icon, colour, media)) in looks.into_iter().enumerate() {
            let mut st = old_states.get(i).cloned().filter(|v| v.is_object()).unwrap_or_else(default_state);
            st["name"] = json!(if name.is_empty() { "Default".to_string() } else { name });
            if i > 0 {
                st["text"] = json!(text);
            }
            let old_glyph = str_of(&st, "glyph").to_string();
            st["glyph"] = json!(icon_to_real(&icon, &old_glyph));
            if colour_of(&st, designs, &size) != colour {
                // a design gives way to the colour; a picture stays on it
                if str_of(&st, "bgType") != "image" {
                    st["bgType"] = json!("colour");
                }
                st["bg"] = json!(colour);
            }
            media_into(&mut st, &media);
            states.push(st);
        }
        o.insert("states".into(), Value::Array(states));
        let old_anim = o.get("anim").map(|a| str_of(a, "type").to_string()).unwrap_or_default();
        if anim_index(&old_anim) != t.anim {
            let mut a = o.get("anim").cloned().filter(|v| v.is_object()).unwrap_or_else(|| json!({ "speed": "normal", "repeat": "once", "phone": true }));
            a["type"] = json!(anim_name(t.anim));
            o.insert("anim".into(), a);
        }
        b
    }

    /// Brings the file's profile list in line with the window's: new profiles get a page, a
    /// duplicated one gets its own copy of the pages and buttons, removed ones lose theirs.
    fn sync_profiles(&mut self, s: &mut Sample) {
        let old: HashMap<String, Value> = self.doc["profiles"].as_array().map(|a| a.iter().map(|p| (str_of(p, "id").to_string(), p.clone())).collect()).unwrap_or_default();
        let mut seen = HashSet::new();
        let mut out = vec![];
        for p in s.profiles.iter_mut() {
            let mut entry = if !p.id.is_empty() && !seen.contains(&p.id) { old.get(&p.id).cloned() } else { None };
            if entry.is_none() {
                entry = Some(match old.get(&p.id) {
                    Some(src) => self.copy_profile(src),
                    None => {
                        let board = uid("Board");
                        self.doc["boards"][&board] = json!([]);
                        json!({ "id": uid("pr-"), "name": "", "body": "sky", "icon": "star", "auto": "Never",
                            "tree": [{ "kind": "page", "id": uid("p-"), "name": "Page 1", "board": board, "cols": 5, "rows": 3 }] })
                    }
                });
            }
            let mut e = entry.expect("profile");
            p.id = str_of(&e, "id").to_string();
            e["name"] = json!(p.name);
            e["body"] = json!(p.colour);
            e["icon"] = json!(p.icon);
            e["auto"] = json!(auto_name(p.auto));
            seen.insert(p.id.clone());
            out.push(e);
        }
        // a removed profile's buttons go with it
        for (id, p) in &old {
            if !seen.contains(id) {
                let mut rows = vec![];
                flatten(p.get("tree").and_then(|t| t.as_array()).map(|t| t.as_slice()).unwrap_or(&[]), 0, &mut rows, &mut HashMap::new());
                if let Some(b) = self.doc["boards"].as_object_mut() {
                    for r in rows.iter().filter(|r| !r.folder) {
                        b.remove(&r.board);
                    }
                }
            }
        }
        self.doc["profiles"] = Value::Array(out);
    }

    /// A copy of a profile with new ids for it, its pages, folders, boards and buttons; its Go
    /// to page buttons point at the copy's own pages.
    fn copy_profile(&mut self, src: &Value) -> Value {
        let mut p = src.clone();
        p["id"] = json!(uid("pr-"));
        // a copy is the user's own: it doesn't follow the plugin's profile
        if let Some(o) = p.as_object_mut() {
            o.remove("from");
        }
        let mut map: HashMap<String, String> = HashMap::new();
        let mut boards: Vec<(String, String)> = vec![];
        fn walk(nodes: &mut [Value], map: &mut HashMap<String, String>, boards: &mut Vec<(String, String)>) {
            for n in nodes.iter_mut() {
                let folder = str_of(n, "kind") == "folder";
                let new = uid(if folder { "f-" } else { "p-" });
                map.insert(str_of(n, "id").to_string(), new.clone());
                n["id"] = json!(new);
                if folder {
                    if let Some(ch) = n.get_mut("children").and_then(|c| c.as_array_mut()) {
                        walk(ch, map, boards);
                    }
                } else {
                    let nb = uid("Board");
                    boards.push((str_of(n, "board").to_string(), nb.clone()));
                    n["board"] = json!(nb);
                }
            }
        }
        if let Some(t) = p.get_mut("tree").and_then(|t| t.as_array_mut()) {
            walk(t, &mut map, &mut boards);
        }
        for (old, new) in boards {
            let mut list = self.doc["boards"].get(&old).cloned().unwrap_or_else(|| json!([]));
            if let Some(a) = list.as_array_mut() {
                for b in a.iter_mut() {
                    b["id"] = json!(uid("t-"));
                    if let Some(steps) = b.get_mut("steps").and_then(|s| s.as_array_mut()) {
                        for st in steps.iter_mut().filter(|st| str_of(st, "type") == "page") {
                            if let Some(np) = st.get("f").map(|f| str_of(f, "page").to_string()).and_then(|id| map.get(&id).cloned()) {
                                st["f"]["page"] = json!(np);
                            }
                        }
                    }
                }
            }
            self.doc["boards"][&new] = list;
        }
        p
    }

    /// Every frame: keeps everything in step and saves a moment after a change. Returns a
    /// message when saving failed.
    pub fn tick(&mut self, s: &mut Sample, now: f64) -> Option<String> {
        // the grid size belongs to the page
        if s.page != self.page_shown {
            if let Some(p) = s.pages.get(s.page) {
                s.cols = p.cols;
                s.rows = p.rows;
            }
            self.page_shown = s.page;
        } else if let Some(p) = s.pages.get_mut(s.page) {
            p.cols = s.cols;
            p.rows = s.rows;
        }
        for (k, p) in s.pages.iter_mut().enumerate() {
            p.count = s.tiles.iter().filter(|t| t.page == k).count();
        }
        if let Some(p) = s.profiles.get_mut(s.active).filter(|p| p.id == self.loaded) {
            p.pages = s.pages.iter().filter(|r| !r.folder).count();
            p.folders = s.pages.iter().filter(|r| r.folder).count();
            p.buttons = s.tiles.len();
        }
        // every button has an id from the moment it exists: plugins address buttons by it
        for t in s.tiles.iter_mut().filter(|t| t.id.is_empty()) {
            t.id = uid("t-");
        }
        // a profile added, duplicated, removed or changed
        if hash_of(&s.profiles) != self.profiles_sig {
            self.store_pages(s);
            self.sync_profiles(s);
            self.profiles_sig = hash_of(&s.profiles);
        }
        // another profile picked: its pages
        let want = s.profiles.get(s.active).map(|p| p.id.clone()).unwrap_or_default();
        if want != self.loaded {
            self.store_pages(s);
            self.load_pages(s, "");
        }
        // save a moment after the last change
        let fp = fingerprint(s);
        if fp != self.seen {
            self.seen = fp;
            self.changed_at = now;
        }
        if (fp != self.saved || self.force) && now - self.changed_at >= SAVE_AFTER {
            return self.save(s).err();
        }
        None
    }

    /// A change is waiting to be saved (the window should look again soon).
    pub fn pending(&self) -> bool {
        self.seen != self.saved || self.force
    }

    /// Writes everything now (when the window closes).
    pub fn save(&mut self, s: &mut Sample) -> Result<(), String> {
        self.store_pages(s);
        let page = s.pages.get(s.page).map(|p| p.id.clone()).unwrap_or_default();
        self.doc["active"] = json!({ "prof": self.loaded, "page": page });
        let fp = fingerprint(s);
        self.saved = fp;
        self.seen = fp;
        self.force = false;
        self.write()
    }

    fn write(&mut self) -> Result<(), String> {
        // screenshot mode (--shot) moves and resizes buttons for the picture: never save that
        if std::env::args().any(|a| a == "--shot") {
            return Ok(());
        }
        let file = self.paths.library_file();
        if !self.backed_up && file.exists() {
            self.backed_up = true;
            if self.auto_backup {
                let _ = super::backups::create(&self.paths, super::backups::Reason::Startup);
            }
        }
        let text = serde_json::to_string(&json!({ "schema_version": 1, "saved_at": now_ms(), "data": self.doc })).map_err(|e| e.to_string())?;
        super::vault::write(&file, super::package::Kind::Save, text.as_bytes()).map_err(|e| format!("Couldn't save your buttons: {e}"))
    }

    /// Tells backend/media.rs which pictures and GIFs the library has.
    pub fn publish_media(&self) {
        super::media::set_from_doc(&self.doc, &self.paths.media, &self.paths.root.join("plugins"));
    }

    /// Adds an imported picture or GIF (backend/media.rs import) to the library. Returns its id.
    pub fn add_media(&mut self, kind: super::media::Kind, entry: Value) -> String {
        let key = match kind {
            super::media::Kind::Picture => "images",
            super::media::Kind::Gif => "gifFiles",
        };
        if !self.doc.get(key).is_some_and(|v| v.is_array()) {
            self.doc[key] = json!([]);
        }
        let id = str_of(&entry, "id").to_string();
        if let Some(a) = self.doc[key].as_array_mut() {
            a.push(entry);
        }
        self.force = true;
        self.publish_media();
        id
    }

    /// Everything a button does: its first step as the window has it, then any later steps.
    pub fn steps_of(&self, t: &Tile) -> Vec<Step> {
        let orig = self.buttons.get(&t.id).or_else(|| self.buttons.get(&t.src));
        // a number or switch read from the file stays one (plugins get 4455, not "4455")
        let old_f = orig.and_then(|b| b.get("steps")).and_then(|s| s.as_array()).and_then(|s| s.first()).and_then(|s| s.get("f")).cloned().unwrap_or_else(|| json!({}));
        let fields: Map<String, Value> = t
            .values
            .iter()
            .map(|(k, v)| (k.clone(), old_f.get(k).filter(|ov| !ov.is_string() && ov.to_string() == *v).cloned().unwrap_or_else(|| Value::String(v.clone()))))
            .collect();
        let mut out = vec![Step { kind: t.action.clone(), fields: Value::Object(fields) }];
        if let Some(rest) = orig.and_then(|b| b.get("steps")).and_then(|s| s.as_array()) {
            for st in rest.iter().skip(1) {
                out.push(Step { kind: str_of(st, "type").into(), fields: st.get("f").cloned().unwrap_or_else(|| json!({})) });
            }
        }
        out
    }

    /// The profile in use came from an imported file and its buttons haven't been allowed yet.
    pub fn needs_trust(&self) -> bool {
        self.profile_index(&self.loaded)
            .map(|i| &self.doc["profiles"][i])
            .map(|p| p.get("imported").and_then(|v| v.as_bool()).unwrap_or(false) && !p.get("trusted").and_then(|v| v.as_bool()).unwrap_or(false))
            .unwrap_or(false)
    }

    pub fn trust(&mut self) {
        if let Some(i) = self.profile_index(&self.loaded) {
            self.doc["profiles"][i]["trusted"] = json!(true);
            self.force = true;
        }
    }
}

// ===== Export and import of a profile (.mdprofile, see profile_io.rs) =====

/// A plugin's name for the "plugins this profile needs" list: the catalogue's name, or its id.
fn plugin_name_of(id: &str) -> String {
    crate::backend::catalog::plugins().iter().find(|p| p.id == id).map(|p| p.name.to_string()).unwrap_or_else(|| id.to_string())
}

/// Every face of a design: its own, or one per button size.
fn faces_of(d: &Value) -> Vec<&Value> {
    match d.get("faces").and_then(|f| f.as_object()) {
        Some(m) if !m.is_empty() => m.values().collect(),
        _ => vec![d],
    }
}

fn faces_of_mut(d: &mut Value) -> Vec<&mut Value> {
    if d.get("faces").and_then(|f| f.as_object()).is_some_and(|m| !m.is_empty()) {
        d.get_mut("faces").and_then(|f| f.as_object_mut()).map(|m| m.values_mut().collect()).unwrap_or_default()
    } else {
        vec![d]
    }
}

impl Library {
    /// Everything one profile needs to be exported: its pages' buttons and the designs, GIFs and
    /// pictures they use, as profile_io::build wants it.
    pub fn bundle_for(&mut self, s: &mut Sample, profile_id: &str) -> Result<Value, String> {
        self.store_pages(s);
        let idx = self.profile_index(profile_id).ok_or("That profile isn't in the library.")?;
        let mut profile = self.doc["profiles"][idx].clone();
        if let Some(o) = profile.as_object_mut() {
            for k in ["imported", "trusted", "needs", "from"] {
                o.remove(k);
            }
        }
        let mut rows = vec![];
        flatten(profile.get("tree").and_then(|t| t.as_array()).map(|t| t.as_slice()).unwrap_or(&[]), 0, &mut rows, &mut HashMap::new());
        let mut boards = Map::new();
        for r in rows.iter().filter(|r| !r.folder) {
            boards.insert(r.board.clone(), self.doc["boards"].get(&r.board).cloned().unwrap_or_else(|| json!([])));
        }
        let (mut design_ids, mut gif_ids, mut image_ids) = (HashSet::new(), HashSet::new(), HashSet::new());
        let mut needed: Vec<String> = vec![];
        let mut need = |n: String| {
            if !needed.contains(&n) {
                needed.push(n);
            }
        };
        let use_gif = |id: &str, ids: &mut HashSet<String>| {
            if !id.is_empty() && id != "none" {
                ids.insert(id.to_string());
            }
        };
        for list in boards.values() {
            for b in list.as_array().into_iter().flatten() {
                for st in b.get("steps").and_then(|s| s.as_array()).into_iter().flatten() {
                    if let Some((pid, _)) = str_of(st, "type").strip_prefix("plug:").and_then(|r| r.split_once(':')) {
                        need(plugin_name_of(pid));
                    }
                }
                for st in b.get("states").and_then(|s| s.as_array()).into_iter().flatten() {
                    for id in [str_of(st, "glyph"), st.get("gif").map(|g| str_of(g, "id")).unwrap_or(""), st.get("bgGif").map(|g| str_of(g, "id")).unwrap_or("")] {
                        if let Some(pid) = id.strip_prefix("plug:").and_then(|r| r.split(':').next()) {
                            need(plugin_name_of(pid));
                        }
                    }
                    if let Some(src) = st.get("bgImage").map(|i| str_of(i, "src")).filter(|s| !s.is_empty()) {
                        image_ids.insert(src.to_string());
                    }
                    for g in [st.get("bgGif"), st.get("gif")].into_iter().flatten() {
                        use_gif(str_of(g, "id"), &mut gif_ids);
                    }
                    if !str_of(st, "design").is_empty() {
                        design_ids.insert(str_of(st, "design").to_string());
                    }
                }
            }
        }
        let designs: Vec<Value> = self.doc.get("designs").and_then(|d| d.as_array()).map(|a| a.iter().filter(|d| design_ids.contains(str_of(d, "id"))).cloned().collect()).unwrap_or_default();
        for d in &designs {
            for f in faces_of(d) {
                if let Some(bg) = f.get("bg") {
                    if !str_of(bg, "image").is_empty() {
                        image_ids.insert(str_of(bg, "image").to_string());
                    }
                    use_gif(str_of(bg, "gif"), &mut gif_ids);
                }
                for l in f.get("layers").and_then(|l| l.as_array()).into_iter().flatten() {
                    if !str_of(l, "image").is_empty() {
                        image_ids.insert(str_of(l, "image").to_string());
                    }
                    use_gif(str_of(l, "gif"), &mut gif_ids);
                }
            }
        }
        let pick = |key: &str, ids: &HashSet<String>, need_media: bool| -> Vec<Value> {
            self.doc.get(key).and_then(|a| a.as_array()).map(|a| a.iter().filter(|e| ids.contains(str_of(e, "id")) && (!need_media || e.get("media").is_some())).cloned().collect()).unwrap_or_default()
        };
        Ok(json!({
            "profile": profile,
            "boards": boards,
            "designs": designs,
            "drawnGifs": pick("drawnGifs", &gif_ids, false),
            "gifFiles": pick("gifFiles", &gif_ids, true),
            "images": pick("images", &image_ids, true),
            "plugins": needed,
        }))
    }

    /// Adds a profile file (.mdprofile) without opening the window: Setup's starter profile for a
    /// plugin it installed. On a library nobody has touched yet (only the empty "My profile"), the
    /// new profile takes its place, so a new install shows just the profiles of the plugins chosen.
    /// Returns the profile's name.
    pub fn add_profile_file(path: &Path) -> Result<String, String> {
        let (mut lib, mut s, _) = Library::open();
        let bundle = super::profile_io::import_from(&lib.paths.media, path)?;
        let name = Library::add_profile_file_to(&mut lib, &mut s, &bundle)?;
        lib.write()?;
        Ok(name)
    }

    /// add_profile_file on a library that is already open (it isn't written here).
    fn add_profile_file_to(lib: &mut Library, s: &mut Sample, bundle: &Value) -> Result<String, String> {
        let untouched = lib.is_untouched();
        let (name, _, _) = lib.import_bundle(s, bundle)?;
        let new = lib.doc["profiles"].as_array().and_then(|a| a.last()).cloned().unwrap_or(Value::Null);
        if untouched {
            if let Some(a) = lib.doc["profiles"].as_array_mut() {
                a.retain(|p| str_of(p, "id") != "mine");
            }
            if let Some(b) = lib.doc["boards"].as_object_mut() {
                b.remove("Board-1");
            }
        }
        // the first one added is the one shown
        if untouched {
            let first_page = fn_first_page(new.get("tree"));
            lib.doc["active"] = json!({ "prof": str_of(&new, "id"), "page": first_page });
        }
        Ok(name)
    }

    /// Only the empty "My profile" with its empty page: what a new install starts with.
    fn is_untouched(&self) -> bool {
        let profiles = self.doc["profiles"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
        profiles.len() == 1 && str_of(&profiles[0], "id") == "mine" && self.doc["boards"].as_object().is_some_and(|b| b.values().all(|v| v.as_array().is_some_and(|a| a.is_empty())))
    }

    /// Adds an imported profile (as profile_io::import_from hands it over) with new ids for
    /// everything, so nothing already here is replaced. A copy of library.json is kept first.
    /// Returns (the new profile's name, pages, buttons).
    pub fn import_bundle(&mut self, s: &mut Sample, b: &Value) -> Result<(String, usize, usize), String> {
        self.store_pages(s);
        let _ = super::backups::create(&self.paths, super::backups::Reason::BeforeImport);
        let mut refs: HashMap<String, String> = HashMap::new();
        for key in ["drawnGifs", "gifFiles", "images", "designs"] {
            if !self.doc.get(key).is_some_and(|v| v.is_array()) {
                self.doc[key] = json!([]);
            }
        }
        let re = |refs: &HashMap<String, String>, id: &str| refs.get(id).cloned().unwrap_or_else(|| id.to_string());
        for (key, prefix) in [("drawnGifs", "g"), ("gifFiles", "f"), ("images", "u:")] {
            for e in b.get(key).and_then(|a| a.as_array()).into_iter().flatten() {
                let mut e = e.clone();
                let new = uid(prefix);
                refs.insert(str_of(&e, "id").to_string(), new.clone());
                e["id"] = json!(new);
                if let Some(a) = self.doc[key].as_array_mut() {
                    a.push(e);
                }
            }
        }
        for d in b.get("designs").and_then(|a| a.as_array()).into_iter().flatten() {
            let mut d = d.clone();
            let new = uid("d");
            refs.insert(str_of(&d, "id").to_string(), new.clone());
            d["id"] = json!(new);
            for f in faces_of_mut(&mut d) {
                if let Some(bg) = f.get_mut("bg").filter(|v| v.is_object()) {
                    for k in ["image", "gif"] {
                        if let Some(old) = bg.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(String::from) {
                            bg[k] = json!(re(&refs, &old));
                        }
                    }
                }
                for l in f.get_mut("layers").and_then(|l| l.as_array_mut()).into_iter().flatten() {
                    l["id"] = json!(uid("l"));
                    for k in ["image", "gif"] {
                        if let Some(old) = l.get(k).and_then(|v| v.as_str()).filter(|v| !v.is_empty()).map(String::from) {
                            l[k] = json!(re(&refs, &old));
                        }
                    }
                }
            }
            if let Some(a) = self.doc["designs"].as_array_mut() {
                a.push(d);
            }
        }
        // the tree, with new ids; each page's buttons go into a board of their own
        let (mut pages, mut buttons) = (0usize, 0usize);
        let mut made: Vec<String> = vec![];
        fn tree(nodes: &[Value], me: &mut Library, refs: &mut HashMap<String, String>, made: &mut Vec<String>, pages: &mut usize, buttons: &mut usize) -> Vec<Value> {
            let mut out = vec![];
            for n in nodes {
                if str_of(n, "kind") == "folder" {
                    let id = uid("f-");
                    refs.insert(str_of(n, "id").to_string(), id.clone());
                    let kids = tree(n.get("children").and_then(|c| c.as_array()).map(|c| c.as_slice()).unwrap_or(&[]), me, refs, made, pages, buttons);
                    let name = match str_of(n, "name") { "" => "Folder", x => x };
                    out.push(json!({ "kind": "folder", "id": id, "name": name, "open": n.get("open").and_then(|v| v.as_bool()).unwrap_or(true), "children": kids }));
                } else {
                    *pages += 1;
                    let board = uid("Board");
                    made.push(board.clone());
                    let list: Vec<Value> = n
                        .get("buttons")
                        .and_then(|b| b.as_array())
                        .into_iter()
                        .flatten()
                        .map(|t| {
                            *buttons += 1;
                            let mut c = t.clone();
                            c["id"] = json!(uid("t-"));
                            for st in c.get_mut("states").and_then(|s| s.as_array_mut()).into_iter().flatten() {
                                if let Some(i) = st.get_mut("bgImage").filter(|v| v.is_object()) {
                                    if let Some(old) = i.get("src").and_then(|v| v.as_str()).map(String::from) {
                                        i["src"] = json!(refs.get(&old).cloned().unwrap_or(old));
                                    }
                                }
                                for k in ["bgGif", "gif"] {
                                    if let Some(g) = st.get_mut(k).filter(|v| v.is_object()) {
                                        if let Some(old) = g.get("id").and_then(|v| v.as_str()).map(String::from) {
                                            g["id"] = json!(refs.get(&old).cloned().unwrap_or(old));
                                        }
                                    }
                                }
                                if let Some(old) = st.get("design").and_then(|v| v.as_str()).map(String::from) {
                                    st["design"] = json!(refs.get(&old).cloned().unwrap_or(old));
                                }
                            }
                            c
                        })
                        .collect();
                    me.doc["boards"][&board] = Value::Array(list);
                    let id = uid("p-");
                    refs.insert(str_of(n, "id").to_string(), id.clone());
                    let name = match str_of(n, "name") { "" => "Page", x => x };
                    out.push(json!({ "kind": "page", "id": id, "name": name, "board": board, "cols": num_of(n, "cols", 4).clamp(1, 12), "rows": num_of(n, "rows", 3).clamp(1, 12) }));
                }
            }
            out
        }
        let src = b.get("profile").cloned().unwrap_or_else(|| json!({}));
        let mut new_tree = tree(src.get("tree").and_then(|t| t.as_array()).map(|t| t.as_slice()).unwrap_or(&[]), self, &mut refs, &mut made, &mut pages, &mut buttons);
        // Go to page buttons follow their page or folder to its new id
        for board in &made {
            if let Some(list) = self.doc["boards"][board].as_array_mut() {
                for t in list.iter_mut() {
                    for st in t.get_mut("steps").and_then(|s| s.as_array_mut()).into_iter().flatten().filter(|st| str_of(st, "type") == "page") {
                        if let Some(old) = st.get("f").map(|f| str_of(f, "page").to_string()) {
                            st["f"]["page"] = json!(refs.get(&old).cloned().unwrap_or(old));
                        }
                    }
                }
            }
        }
        if !new_tree.iter().any(|n| str_of(n, "kind") == "page") && pages == 0 {
            let board = uid("Board");
            self.doc["boards"][&board] = json!([]);
            new_tree.push(json!({ "kind": "page", "id": uid("p-"), "name": "Main", "board": board, "cols": 5, "rows": 3 }));
        }
        let base: String = match str_of(&src, "name") { "" => "Imported profile", x => x }.chars().take(24).collect();
        let taken: HashSet<String> = self.doc["profiles"].as_array().map(|a| a.iter().map(|p| str_of(p, "name").to_string()).collect()).unwrap_or_default();
        let (mut name, mut k) = (base.clone(), 2);
        while taken.contains(&name) {
            name = format!("{} {k}", base.chars().take(20).collect::<String>());
            k += 1;
        }
        let profile = json!({
            "id": uid("pr-"), "name": name,
            "body": match str_of(&src, "body") { "" => "peach", x => x },
            "icon": match str_of(&src, "icon") { "" => "star", x => x },
            "auto": match str_of(&src, "auto") { "" => "Never", x => x },
            "tree": new_tree,
            // marked so buttons that open programs or press keys ask once before their first run
            "imported": true, "trusted": false,
            "needs": b.get("plugins").cloned().unwrap_or_else(|| json!([])),
        });
        if let Some(a) = self.doc["profiles"].as_array_mut() {
            a.push(profile);
        }
        s.profiles = self.profiles();
        self.profiles_sig = hash_of(&s.profiles);
        self.force = true;
        self.publish_media();
        Ok((name, pages, buttons))
    }
}

/// A look's picture and GIF as the file keeps them. An old look's bgGif counts as a background GIF.
fn media_from(st: &Value) -> Media {
    let mut m = Media::default();
    if str_of(st, "bgType") == "image" {
        if let Some(i) = st.get("bgImage") {
            m.pic = str_of(i, "src").to_string();
            m.contain = str_of(i, "fit") == "contain";
        }
    }
    let speed = |g: &Value| match str_of(g, "speed") { "slow" => 0, "fast" => 2, _ => 1 };
    match st.get("gif").filter(|g| !str_of(g, "id").is_empty() && str_of(g, "id") != "none") {
        Some(g) => {
            m.gif = str_of(g, "id").to_string();
            m.gif_icon = str_of(g, "place") == "icon";
            m.speed = speed(g);
        }
        None => {
            if let Some(g) = st.get("bgGif").filter(|g| !str_of(g, "id").is_empty() && str_of(g, "id") != "none") {
                m.gif = str_of(g, "id").to_string();
                m.speed = speed(g);
            }
        }
    }
    m
}

/// Puts a look's picture and GIF into the state, changing only what differs from what was read.
fn media_into(st: &mut Value, m: &Media) {
    let old = media_from(st);
    if old == *m {
        return;
    }
    if old.pic != m.pic || old.contain != m.contain {
        if m.pic.is_empty() {
            if str_of(st, "bgType") == "image" {
                st["bgType"] = json!("colour");
            }
        } else {
            st["bgType"] = json!("image");
            st["bgImage"] = json!({ "src": m.pic, "fit": if m.contain { "contain" } else { "cover" } });
        }
    }
    if old.gif != m.gif || old.gif_icon != m.gif_icon || old.speed != m.speed {
        let speed = ["slow", "normal", "fast"][m.speed.min(2) as usize];
        if m.gif.is_empty() {
            st["gif"] = json!({ "id": "none", "place": "bg", "speed": "normal" });
        } else {
            st["gif"] = json!({ "id": m.gif, "place": if m.gif_icon { "icon" } else { "bg" }, "speed": speed });
        }
        // the old background GIF field gives way to gif
        if let Some(o) = st.as_object_mut() {
            o.remove("bgGif");
        }
    }
}

/// A button from the file as the window shows it.
fn tile_from(b: &Value, page: usize, designs: &HashMap<String, Value>) -> Tile {
    let mut t = Tile::new(page, num_of(b, "x", 0), num_of(b, "y", 0));
    t.id = str_of(b, "id").into();
    t.w = num_of(b, "w", 1).clamp(1, 12);
    t.h = num_of(b, "h", 1).clamp(1, 12);
    t.label = str_of(b, "label").into();
    let size = format!("{}x{}", t.w, t.h);
    let states: Vec<Value> = b.get("states").and_then(|s| s.as_array()).cloned().unwrap_or_default();
    let first = states.first().cloned().unwrap_or_else(default_state);
    let icon = icon_from_real(str_of(&first, "glyph"));
    if icon.contains(':') {
        t.icon = "bolt".into();
        t.image = icon;
    } else {
        t.icon = icon;
    }
    t.colour = colour_of(&first, designs, &size);
    t.media = media_from(&first);
    t.first_name = str_of(&first, "name").into();
    t.more = states
        .iter()
        .skip(1)
        .map(|st| Look { name: str_of(st, "name").into(), label: str_of(st, "text").into(), icon: icon_from_real(str_of(st, "glyph")), colour: colour_of(st, designs, &size), media: media_from(st) })
        .collect();
    t.cur = num_of(b, "cur", 0).min(t.more.len());
    t.anim = anim_index(b.get("anim").map(|a| str_of(a, "type")).unwrap_or(""));
    if let Some(st) = b.get("steps").and_then(|s| s.as_array()).and_then(|s| s.first()) {
        t.action = str_of(st, "type").into();
        if let Some(f) = st.get("f").and_then(|f| f.as_object()) {
            for (k, v) in f {
                let v = match v {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                t.values.push((k.clone(), v));
            }
        }
    }
    t
}


// ===== Profiles that come with a plugin =====
//
// A ready-made profile from the marketplace remembers where it came from in its "from" entry:
// { "plugin": id, "sha": the .mdprofile's SHA-256, "base": what it looked like then }. When the
// plugin's profile changes, an untouched profile is updated in place (same id, name, colour,
// icon, rule and trust; a backup first) and an edited one is left for the user to decide.

/// What became of one profile when its plugin's profile changed.
pub enum PluginProfile {
    /// Already the same as the new one (now linked to it)
    Same,
    /// Untouched since it was added, so it was updated: its name
    Updated(String),
    /// Changed by the user (or added before profiles were linked): (its id, its name)
    Edited(String, String),
}

/// "Spotify", "Spotify 2", "Spotify 3"... for a plugin profile called "Spotify".
fn same_profile_name(name: &str, file_name: &str) -> bool {
    let base: String = file_name.chars().take(24).collect();
    let short: String = file_name.chars().take(20).collect();
    !file_name.is_empty() && (name == base || name.strip_prefix(&format!("{short} ")).is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())))
}

fn node_name(n: &Value) -> String {
    match str_of(n, "name") {
        "" if str_of(n, "kind") == "folder" => "Folder".into(),
        "" => "Page".into(),
        x => x.to_string(),
    }
}

/// A profile as the user sees it, as one hash: page and folder names and order, page sizes, and
/// every button as the window shows it, with what doesn't count left out (ids, the look showing
/// now, which copy of a picture it uses) and Go to page known by the page's name. The same for a
/// profile in the library and one in a file, so the two can be compared.
fn content_hash(tree: &[Value], buttons_of: &dyn Fn(&Value) -> Vec<Value>, designs: &HashMap<String, Value>) -> String {
    fn names_of(nodes: &[Value], m: &mut HashMap<String, String>) {
        for n in nodes {
            m.insert(str_of(n, "id").to_string(), node_name(n));
            if let Some(c) = n.get("children").and_then(|c| c.as_array()) {
                names_of(c, m);
            }
        }
    }
    fn walk(nodes: &[Value], depth: usize, h: &mut DefaultHasher, page: &mut dyn FnMut(&Value, &mut DefaultHasher)) {
        for n in nodes {
            let folder = str_of(n, "kind") == "folder";
            (depth, folder, node_name(n)).hash(h);
            if folder {
                walk(n.get("children").and_then(|c| c.as_array()).map(|c| c.as_slice()).unwrap_or(&[]), depth + 1, h, page);
            } else {
                (num_of(n, "cols", 4).clamp(1, 12), num_of(n, "rows", 3).clamp(1, 12)).hash(h);
                page(n, h);
            }
        }
    }
    let mut names = HashMap::new();
    names_of(tree, &mut names);
    let by_name = |v: &str| names.get(v).cloned().unwrap_or_else(|| v.to_string());
    let media = |s: &mut String| {
        if !s.is_empty() {
            *s = "media".into();
        }
    };
    let pic = |s: &mut String| {
        if s.starts_with("u:") {
            *s = "u:".into();
        }
    };
    let mut page = |n: &Value, h: &mut DefaultHasher| {
        let mut tiles: Vec<(Tile, Vec<(String, String)>)> = buttons_of(n)
            .iter()
            .map(|b| {
                let mut t = tile_from(b, 0, designs);
                t.id.clear();
                t.src.clear();
                t.cur = 0;
                media(&mut t.media.pic);
                media(&mut t.media.gif);
                pic(&mut t.image);
                pic(&mut t.icon);
                for l in t.more.iter_mut() {
                    media(&mut l.media.pic);
                    media(&mut l.media.gif);
                    pic(&mut l.icon);
                }
                if t.action == "page" {
                    for (k, v) in t.values.iter_mut() {
                        if k == "page" {
                            *v = by_name(v);
                        }
                    }
                }
                // the steps after the first (the window shows only the first)
                let rest: Vec<(String, String)> = b
                    .get("steps")
                    .and_then(|s| s.as_array())
                    .into_iter()
                    .flatten()
                    .skip(1)
                    .map(|st| {
                        let mut f = st.get("f").cloned().unwrap_or_else(|| json!({}));
                        if str_of(st, "type") == "page" {
                            let p = by_name(str_of(&f, "page"));
                            f["page"] = json!(p);
                        }
                        (str_of(st, "type").to_string(), f.to_string())
                    })
                    .collect();
                (t, rest)
            })
            .collect();
        tiles.sort_by_key(|(t, _)| (t.row, t.col));
        for (t, rest) in &tiles {
            t.hash(h);
            rest.hash(h);
        }
    };
    let mut h = DefaultHasher::new();
    walk(tree, 0, &mut h, &mut page);
    format!("{:016x}", h.finish())
}

/// The content hash of a profile file (as profile_io::import_from hands it over).
pub fn bundle_hash(b: &Value) -> String {
    let designs: HashMap<String, Value> = b.get("designs").and_then(|d| d.as_array()).map(|a| a.iter().map(|d| (str_of(d, "id").to_string(), d.clone())).collect()).unwrap_or_default();
    let tree = b.get("profile").and_then(|p| p.get("tree")).and_then(|t| t.as_array()).cloned().unwrap_or_default();
    content_hash(&tree, &|n| n.get("buttons").and_then(|b| b.as_array()).cloned().unwrap_or_default(), &designs)
}

impl Library {
    fn profile_hash(&self, idx: usize) -> String {
        let boards = &self.doc["boards"];
        let tree = self.doc["profiles"][idx].get("tree").and_then(|t| t.as_array()).cloned().unwrap_or_default();
        content_hash(&tree, &|n| boards.get(str_of(n, "board")).and_then(|b| b.as_array()).cloned().unwrap_or_default(), &self.designs())
    }

    /// Marks the last profile added as plugin `plugin`'s ready-made one (from the file with `sha`).
    pub fn link_last_profile(&mut self, plugin: &str, sha: &str) {
        let Some(idx) = self.doc["profiles"].as_array().map(|a| a.len()).filter(|n| *n > 0).map(|n| n - 1) else { return };
        let base = self.profile_hash(idx);
        self.doc["profiles"][idx]["from"] = json!({ "plugin": plugin, "sha": sha, "base": base });
        self.force = true;
    }

    /// The profiles that follow plugin `plugin`'s ready-made profile: the ones linked to it, and
    /// (when `adopt`) ones imported before profiles were linked that need only this plugin
    /// (named `plugin_name`). Their ids.
    pub fn plugin_profiles(&self, plugin: &str, plugin_name: &str, adopt: bool) -> Vec<String> {
        self.doc["profiles"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|p| match p.get("from") {
                        Some(f) => str_of(f, "plugin") == plugin,
                        None => {
                            adopt
                                && p.get("imported").and_then(|v| v.as_bool()).unwrap_or(false)
                                && p.get("needs").and_then(|n| n.as_array()).is_some_and(|n| n.len() == 1 && n[0].as_str() == Some(plugin_name))
                        }
                    })
                    .map(|p| str_of(p, "id").to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The SHA-256 of the plugin profile file profile `id` was last matched with ("" if none).
    pub fn profile_sha(&self, id: &str) -> String {
        self.profile_index(id).and_then(|i| self.doc["profiles"][i].get("from").map(|f| str_of(f, "sha").to_string())).unwrap_or_default()
    }

    /// Plugin `plugin`'s ready-made profile is now `bundle` (its file's `sha`). Each profile that
    /// follows it is linked if it already matches, updated in place if the user hasn't changed it,
    /// and reported as edited otherwise (nothing changes in it then).
    #[allow(clippy::too_many_arguments)]
    pub fn plugin_profile_changed(&mut self, s: &mut Sample, plugin: &str, plugin_name: &str, adopt: bool, bundle: &Value, sha: &str) -> Vec<PluginProfile> {
        self.store_pages(s);
        let new = bundle_hash(bundle);
        let file_name = bundle.get("profile").map(|p| str_of(p, "name").to_string()).unwrap_or_default();
        let mut out = vec![];
        for id in self.plugin_profiles(plugin, plugin_name, adopt) {
            let Some(idx) = self.profile_index(&id) else { continue };
            let cur = self.profile_hash(idx);
            let name = str_of(&self.doc["profiles"][idx], "name").to_string();
            let base = self.doc["profiles"][idx].get("from").map(|f| str_of(f, "base").to_string());
            // one added before profiles were linked counts only under the plugin profile's own
            // name ("Spotify", or "Spotify 2" for a second copy): someone's own layout that uses
            // the plugin is never taken for it
            if base.is_none() && !same_profile_name(&name, &file_name) {
                continue;
            }
            if cur == new {
                self.doc["profiles"][idx]["from"] = json!({ "plugin": plugin, "sha": sha, "base": new });
                self.force = true;
                out.push(PluginProfile::Same);
            } else if base.as_deref() == Some(cur.as_str()) {
                match self.replace_profile(s, &id, bundle, plugin, sha) {
                    Ok(n) => out.push(PluginProfile::Updated(n)),
                    Err(_) => out.push(PluginProfile::Edited(id, name)),
                }
            } else {
                out.push(PluginProfile::Edited(id, name));
            }
        }
        out
    }

    /// Puts plugin profile `bundle` in place of profile `id`'s pages and buttons. The profile keeps
    /// its id, name, colour, icon, switching rule and trust, and the page shown if it still has
    /// one of that name. A copy of library.json is kept first. Returns the profile's name.
    pub fn replace_profile(&mut self, s: &mut Sample, id: &str, bundle: &Value, plugin: &str, sha: &str) -> Result<String, String> {
        self.store_pages(s);
        let shown = s.pages.get(s.page).map(|p| p.name.clone()).unwrap_or_default();
        let before = self.doc["profiles"].as_array().map_or(0, |a| a.len());
        // added as a new profile first (new ids and its pictures, after a backup) ...
        self.import_bundle(s, bundle)?;
        let mut list = self.doc["profiles"].as_array().cloned().unwrap_or_default();
        if list.len() != before + 1 {
            return Err("The new profile couldn't be read.".into());
        }
        let temp = list.pop().expect("the new profile");
        let idx = list.iter().position(|p| str_of(p, "id") == id).ok_or("That profile isn't in the library any more.")?;
        // ... then its pages take the old ones' place, and the old boards go
        let mut rows = vec![];
        flatten(list[idx].get("tree").and_then(|t| t.as_array()).map(|t| t.as_slice()).unwrap_or(&[]), 0, &mut rows, &mut HashMap::new());
        if let Some(b) = self.doc["boards"].as_object_mut() {
            for r in rows.iter().filter(|r| !r.folder) {
                b.remove(&r.board);
            }
        }
        list[idx]["tree"] = temp["tree"].clone();
        list[idx]["needs"] = temp["needs"].clone();
        list[idx]["imported"] = json!(true);
        let name = str_of(&list[idx], "name").to_string();
        self.doc["profiles"] = Value::Array(list);
        let base = self.profile_hash(idx);
        self.doc["profiles"][idx]["from"] = json!({ "plugin": plugin, "sha": sha, "base": base });
        s.profiles = self.profiles();
        self.profiles_sig = hash_of(&s.profiles);
        if self.loaded == id {
            // the window shows this profile: its new pages, or the old ones would be saved back
            self.load_pages(s, "");
            if let Some(k) = s.pages.iter().position(|p| !p.folder && p.name == shown) {
                s.page = k;
                s.cols = s.pages[k].cols;
                s.rows = s.pages[k].rows;
                self.page_shown = k;
            }
        }
        self.force = true;
        Ok(name)
    }

    /// Keep mine: profile `id` stays as the user has it and isn't offered this version again.
    pub fn keep_profile(&mut self, id: &str, plugin: &str, sha: &str) {
        if let Some(idx) = self.profile_index(id) {
            let base = self.doc["profiles"][idx].get("from").map(|f| str_of(f, "base").to_string()).unwrap_or_default();
            self.doc["profiles"][idx]["from"] = json!({ "plugin": plugin, "sha": sha, "base": base });
            self.force = true;
        }
    }

    /// Setup's starter profile after it installed or updated plugin `plugin`: the profiles that
    /// follow it are updated (see plugin_profile_changed); it is added only when none does.
    /// Returns what happened, in words.
    pub fn add_plugin_profile(path: &Path, plugin: &str, plugin_name: &str, sha: &str) -> Result<String, String> {
        let (mut lib, mut s, _) = Library::open();
        let bundle = super::profile_io::import_from(&lib.paths.media, path)?;
        if lib.plugin_profiles(plugin, plugin_name, true).is_empty() {
            let name = Library::add_profile_file_to(&mut lib, &mut s, &bundle)?;
            lib.link_last_profile(plugin, sha);
            lib.write()?;
            return Ok(format!("{name} added"));
        }
        let done = lib.plugin_profile_changed(&mut s, plugin, plugin_name, true, &bundle, sha);
        lib.write()?;
        let said: Vec<String> = done
            .iter()
            .map(|d| match d {
                PluginProfile::Same => "already up to date".to_string(),
                PluginProfile::Updated(n) => format!("{n} updated"),
                PluginProfile::Edited(_, n) => format!("{n} kept as you changed it (the app asks)"),
            })
            .collect();
        Ok(said.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_pictures_go_both_ways() {
        let g = "plug:minnion_spotify:icons/sp-next.png";
        assert_eq!(icon_from_real(g), "png:minnion_spotify/sp-next");
        assert_eq!(icon_to_real("png:minnion_spotify/sp-next", g), g);
        assert_eq!(icon_to_real("png:minnion_timer/countdown", ""), "plug:minnion_timer:icons/countdown.png");
        assert_eq!(icon_from_real("plug:nobody:icons/x.png"), "bolt");
        assert_eq!(icon_from_real("px:home"), "px:home");
    }

    #[test]
    fn a_tree_comes_back_the_same() {
        let tree = json!([
            { "kind": "page", "id": "p1", "name": "One", "board": "b1", "cols": 5, "rows": 3 },
            { "kind": "folder", "id": "f1", "name": "F", "open": false, "children": [
                { "kind": "page", "id": "p2", "name": "Two", "board": "b2", "cols": 4, "rows": 3 }
            ] },
            { "kind": "page", "id": "p3", "name": "Three", "board": "b3", "cols": 3, "rows": 3 }
        ]);
        let mut rows = vec![];
        let mut keep = HashMap::new();
        flatten(tree.as_array().unwrap(), 0, &mut rows, &mut keep);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[2].depth, 1);
        assert_eq!(Value::Array(build_tree(&rows, &mut 0, 0, &keep)), tree);
    }

    #[test]
    fn a_button_keeps_what_the_window_does_not_know() {
        let b = json!({ "id": "t1", "label": "Next", "x": 1, "y": 0, "w": 1, "h": 1, "mode": "single", "knob": true,
            "steps": [{ "type": "key", "f": { "keys": "Ctrl+N" } }, { "type": "delay", "f": { "ms": "200" } }],
            "style": { "shape": "full", "textPos": "bottom" },
            "states": [{ "name": "Default", "text": "", "glyph": "bolt", "gif": { "id": "none" }, "bgType": "design", "bg": "#2d3142", "design": "d1" }],
            "cur": 0, "anim": { "type": "press", "speed": "fast" } });
        let designs: HashMap<String, Value> = [("d1".to_string(), json!({ "id": "d1", "bg": { "colour": "#112233" } }))].into();
        let t = tile_from(&b, 0, &designs);
        assert_eq!(t.colour, "#112233");
        assert_eq!(t.action, "key");
        let lib = Library {
            paths: Paths::find(),
            doc: json!({ "profiles": [], "boards": {}, "designs": [designs["d1"].clone()] }),
            loaded: String::new(),
            page_shown: 0,
            buttons: [("t1".to_string(), b.clone())].into(),
            nodes: HashMap::new(),
            profiles_sig: 0,
            saved: 0,
            seen: 0,
            changed_at: 0.0,
            force: false,
            backed_up: true,
            auto_backup: true,
        };
        // unchanged: the same button comes back
        assert_eq!(lib.button_to_real(&t, &HashMap::new(), &designs), b);
        // a new colour: the look stops using the design, and the rest stays
        let mut t2 = t.clone();
        t2.colour = "mint".into();
        let b2 = lib.button_to_real(&t2, &HashMap::new(), &designs);
        assert_eq!(b2["states"][0]["bgType"], "colour");
        assert_eq!(b2["states"][0]["bg"], "mint");
        assert_eq!(b2["knob"], true);
        assert_eq!(b2["steps"][1]["type"], "delay");
        assert_eq!(b2["anim"]["speed"], "fast");
        assert_eq!(lib.steps_of(&t).len(), 2);
    }
}

#[cfg(test)]
mod media_looks {
    use super::*;

    fn lib_with(b: &Value) -> Library {
        Library {
            paths: Paths::find(),
            doc: json!({ "profiles": [], "boards": {}, "designs": [] }),
            loaded: String::new(),
            page_shown: 0,
            buttons: [(str_of(b, "id").to_string(), b.clone())].into(),
            nodes: HashMap::new(),
            profiles_sig: 0,
            saved: 0,
            seen: 0,
            changed_at: 0.0,
            force: false,
            backed_up: true,
            auto_backup: true,
        }
    }

    #[test]
    fn pictures_and_gifs_on_looks_come_back_unchanged_and_change_when_edited() {
        let b = json!({ "id": "t1", "label": "Sun", "x": 0, "y": 0, "w": 1, "h": 1, "steps": [], "cur": 0,
            "states": [
                { "name": "Default", "text": "", "glyph": "sun", "gif": { "id": "g1", "place": "icon", "speed": "fast" }, "bgType": "image", "bg": "sky", "bgImage": { "src": "u:9", "fit": "contain" } },
                { "name": "Night", "text": "Zz", "glyph": "moon", "bgType": "colour", "bg": "lilac", "bgGif": { "id": "f2", "speed": "slow" } },
                { "name": "Plain", "text": "", "glyph": "bolt", "gif": { "id": "none", "place": "bg", "speed": "normal" }, "bgType": "colour", "bg": "tile" }
            ] });
        let designs = HashMap::new();
        let t = tile_from(&b, 0, &designs);
        assert_eq!(t.media, Media { pic: "u:9".into(), contain: true, gif: "g1".into(), gif_icon: true, speed: 2 });
        assert_eq!(t.more[0].media, Media { pic: String::new(), contain: false, gif: "f2".into(), gif_icon: false, speed: 0 }, "an old bgGif is a background GIF");
        assert!(t.more[1].media.is_empty());
        let lib = lib_with(&b);
        // nothing changed: the same button comes back
        assert_eq!(lib.button_to_real(&t, &HashMap::new(), &designs), b);
        // a new colour keeps the picture
        let mut t2 = t.clone();
        t2.colour = "mint".into();
        let b2 = lib.button_to_real(&t2, &HashMap::new(), &designs);
        assert_eq!((b2["states"][0]["bgType"].as_str(), b2["states"][0]["bg"].as_str(), b2["states"][0]["bgImage"]["src"].as_str()), (Some("image"), Some("mint"), Some("u:9")));
        // clearing the picture and the GIF; a new GIF on the second look replaces its bgGif
        let mut t3 = t.clone();
        t3.media.pic.clear();
        t3.media.gif.clear();
        t3.more[0].media.gif = "g7".into();
        t3.more[0].media.gif_icon = true;
        t3.more[1].media.pic = "u:3".into();
        let b3 = lib.button_to_real(&t3, &HashMap::new(), &designs);
        assert_eq!(b3["states"][0]["bgType"], "colour");
        assert_eq!(b3["states"][0]["bgImage"]["src"], "u:9", "the old picture setting is kept, just not used");
        assert_eq!(b3["states"][0]["gif"]["id"], "none");
        assert_eq!(b3["states"][1]["gif"], json!({ "id": "g7", "place": "icon", "speed": "slow" }));
        assert!(b3["states"][1].get("bgGif").is_none());
        assert_eq!(b3["states"][2]["bgType"], "image");
        assert_eq!(b3["states"][2]["bgImage"], json!({ "src": "u:3", "fit": "cover" }));
        // and read back, it is what was set
        let back = tile_from(&b3, 0, &designs);
        assert!(back.media.is_empty());
        assert_eq!(back.more[0].media.gif, "g7");
        assert_eq!(back.more[1].media.pic, "u:3");
    }
}

#[cfg(test)]
mod profile_files {
    use super::*;

    #[test]
    fn a_profile_exports_and_imports_with_new_ids() {
        let dir = std::env::temp_dir().join(format!("md-lib-test-{}", now_ms()));
        let paths = Paths { root: dir.clone(), library: dir.join("library"), media: dir.join("library").join("media"), backups: dir.join("backups") };
        paths.make_folders().unwrap();
        let doc = json!({
            "profiles": [{ "id": "pr1", "name": "Stream", "body": "mint", "icon": "star", "auto": "OBS Studio", "tree": [
                { "kind": "page", "id": "p1", "name": "Main", "board": "B1", "cols": 4, "rows": 3 },
                { "kind": "folder", "id": "f1", "name": "F", "open": true, "children": [
                    { "kind": "page", "id": "p2", "name": "Two", "board": "B2", "cols": 5, "rows": 3 } ] } ] }],
            "boards": { "B1": [{ "id": "t1", "label": "Go", "steps": [
                    { "type": "page", "f": { "page": "p2" } }, { "type": "plug:minnion_spotify:next", "f": {} }],
                "states": [{ "name": "Default", "design": "d1", "gif": { "id": "g1" } }] }], "B2": [] },
            "designs": [{ "id": "d1", "faces": { "1x1": { "bg": { "image": "u:1" }, "layers": [{ "id": "l1", "image": "u:1" }] } } }, { "id": "d2" }],
            "drawnGifs": [{ "id": "g1", "name": "Blink" }], "gifFiles": [], "images": [{ "id": "u:1", "media": "x.png" }],
            "active": {}
        });
        let mut lib = Library { paths, doc, loaded: String::new(), page_shown: 0, buttons: HashMap::new(), nodes: HashMap::new(), profiles_sig: 0, saved: 0, seen: 0, changed_at: 0.0, force: false, backed_up: true, auto_backup: true };
        let mut s = Sample::empty();
        s.profiles = lib.profiles();
        let b = lib.bundle_for(&mut s, "pr1").unwrap();
        // only what the profile uses goes along
        assert_eq!(b["designs"].as_array().unwrap().len(), 1);
        assert_eq!(b["drawnGifs"].as_array().unwrap().len(), 1);
        assert_eq!(b["images"].as_array().unwrap().len(), 1);
        assert_eq!(b["plugins"], json!(["Spotify"]));
        assert_eq!(b["boards"].as_object().unwrap().len(), 2);

        // through the file format and back in
        let bytes = super::super::profile_io::build(&lib.paths.media, &b);
        assert!(bytes.is_err(), "the picture file x.png isn't in the media folder");
        std::fs::write(lib.paths.media.join("x.png"), [1u8, 2, 3]).unwrap();
        let file = lib.paths.root.join("Stream.mdprofile");
        super::super::profile_io::export_to(&lib.paths.media, &b, &file).unwrap();
        let back = super::super::profile_io::import_from(&lib.paths.media, &file).unwrap();
        let (name, pages, buttons) = lib.import_bundle(&mut s, &back).unwrap();
        assert_eq!((name.as_str(), pages, buttons), ("Stream 2", 2, 1));
        assert_eq!(s.profiles.len(), 2);
        let p = &lib.doc["profiles"][1];
        assert_eq!(p["imported"], true);
        assert_eq!(p["trusted"], false);
        assert_eq!(p["needs"], json!(["Spotify"]));
        assert_eq!(p["auto"], "OBS Studio");
        // every id is new, and the Go to page button follows its page
        let (page1, page2) = (&p["tree"][0], &p["tree"][1]["children"][0]);
        assert_ne!(page1["id"], "p1");
        let b1 = &lib.doc["boards"][page1["board"].as_str().unwrap()][0];
        assert_ne!(b1["id"], "t1");
        assert_eq!(b1["steps"][0]["f"]["page"], page2["id"]);
        assert_eq!(b1["steps"][1]["type"], "plug:minnion_spotify:next");
        // the design, its picture and its GIF were added with new ids and the button points at them
        let (d, g) = (b1["states"][0]["design"].as_str().unwrap(), b1["states"][0]["gif"]["id"].as_str().unwrap());
        assert_ne!(d, "d1");
        assert_ne!(g, "g1");
        let design = lib.doc["designs"].as_array().unwrap().iter().find(|x| x["id"] == d).unwrap();
        let img = design["faces"]["1x1"]["bg"]["image"].as_str().unwrap();
        assert_ne!(img, "u:1");
        assert_eq!(design["faces"]["1x1"]["layers"][0]["image"], img);
        assert!(lib.doc["images"].as_array().unwrap().iter().any(|x| x["id"] == img));
        // the first profile is untouched, and a copy of the library was kept first
        assert_eq!(lib.doc["profiles"][0]["id"], "pr1");
        assert!(lib.force);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A plugin's profile file with these buttons on its one page.
    fn plugin_bundle(labels: &[&str]) -> Value {
        let buttons: Vec<Value> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| json!({ "id": format!("t{i}"), "label": l, "x": i, "y": 0, "w": 1, "h": 1, "steps": [{ "type": "plug:minnion_spotify:play", "f": {} }],
                "states": [{ "name": "On" }, { "name": "Off" }], "cur": 0 }))
            .collect();
        json!({ "profile": { "name": "Spotify", "body": "mint", "icon": "note", "tree": [
                    { "kind": "page", "id": "p1", "name": "Main", "cols": 5, "rows": 3, "buttons": buttons }] },
                "designs": [], "drawnGifs": [], "gifFiles": [], "images": [], "plugins": ["Spotify"] })
    }

    /// The real Spotify profile from the marketplace (with its pictures and GIFs) looks the same
    /// in the library as in its file, also after the window saved it, so it updates by itself.
    ///     cargo test --release real_plugin_profile -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_plugin_profile_counts_as_untouched() {
        let dir = std::env::temp_dir().join(format!("md-lib-real-{}", now_ms()));
        let paths = Paths { root: dir.clone(), library: dir.join("library"), media: dir.join("library").join("media"), backups: dir.join("backups") };
        paths.make_folders().unwrap();
        let mut lib = Library { paths, doc: default_doc(), loaded: String::new(), page_shown: 0, buttons: HashMap::new(), nodes: HashMap::new(), profiles_sig: 0, saved: 0, seen: 0, changed_at: 0.0, force: false, backed_up: true, auto_backup: true };
        let mut s = Sample::empty();
        s.profiles = lib.profiles();
        lib.load_pages(&mut s, "");
        let d = super::super::market::download_plugin_profile(&dir, "minnion_spotify").unwrap();
        let bundle = super::super::profile_io::import_from(&lib.paths.media, &d.path).unwrap();
        println!("{} buttons on {} pages", bundle["profile"]["tree"].to_string().matches("\"label\"").count(), bundle["profile"]["tree"].as_array().map_or(0, |t| t.len()));
        lib.import_bundle(&mut s, &bundle).unwrap();
        lib.link_last_profile("minnion_spotify", &d.sha);
        let base = lib.doc["profiles"][1]["from"]["base"].as_str().unwrap().to_string();
        assert_eq!(base, bundle_hash(&bundle), "the library and the file look the same");
        s.active = 1;
        lib.load_pages(&mut s, "");
        lib.store_pages(&mut s);
        assert_eq!(lib.profile_hash(1), base, "still untouched after the window saved it");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_plugin_profile_follows_its_plugin() {
        let dir = std::env::temp_dir().join(format!("md-lib-plug-{}", now_ms()));
        let paths = Paths { root: dir.clone(), library: dir.join("library"), media: dir.join("library").join("media"), backups: dir.join("backups") };
        paths.make_folders().unwrap();
        let mut lib = Library { paths, doc: default_doc(), loaded: String::new(), page_shown: 0, buttons: HashMap::new(), nodes: HashMap::new(), profiles_sig: 0, saved: 0, seen: 0, changed_at: 0.0, force: false, backed_up: true, auto_backup: true };
        let mut s = Sample::empty();
        s.profiles = lib.profiles();
        lib.load_pages(&mut s, "");

        // added from the marketplace and linked
        lib.import_bundle(&mut s, &plugin_bundle(&["Play"])).unwrap();
        lib.link_last_profile("minnion_spotify", "sha1");
        let id = lib.doc["profiles"][1]["id"].as_str().unwrap().to_string();
        assert_eq!(lib.plugin_profiles("minnion_spotify", "Spotify", false), vec![id.clone()]);
        let base = lib.doc["profiles"][1]["from"]["base"].as_str().unwrap().to_string();
        assert_eq!(base, bundle_hash(&plugin_bundle(&["Play"])), "the library and the file look the same");

        // shown in the window, saved back and a look pressed: still untouched
        s.active = 1;
        lib.load_pages(&mut s, "");
        s.tiles[0].cur = 1;
        lib.store_pages(&mut s);
        assert_eq!(lib.profile_hash(1), base);

        // the plugin's profile gets a button: the untouched profile is updated in place
        s.profiles[1].name = "My Spotify".into();
        lib.sync_profiles(&mut s);
        let r = lib.plugin_profile_changed(&mut s, "minnion_spotify", "Spotify", false, &plugin_bundle(&["Play", "Next"]), "sha2");
        assert!(matches!(r.as_slice(), [PluginProfile::Updated(n)] if n == "My Spotify"));
        assert_eq!(lib.doc["profiles"].as_array().unwrap().len(), 2, "no copy was added");
        assert_eq!(lib.doc["profiles"][1]["id"], json!(id));
        assert_eq!(lib.doc["profiles"][1]["name"], "My Spotify");
        assert_eq!(lib.profile_sha(&id), "sha2");
        assert_eq!(s.tiles.len(), 2, "the window shows the new buttons");
        assert_eq!(lib.doc["boards"].as_object().unwrap().len(), 2, "the old board went");

        // the user changes a button: the next version isn't put in by itself
        s.tiles[0].label = "Mine".into();
        let r = lib.plugin_profile_changed(&mut s, "minnion_spotify", "Spotify", false, &plugin_bundle(&["Play", "Next", "Back"]), "sha3");
        assert!(matches!(r.as_slice(), [PluginProfile::Edited(i, _)] if *i == id));
        assert_eq!(s.tiles[0].label, "Mine");
        lib.keep_profile(&id, "minnion_spotify", "sha3");
        assert_eq!(lib.profile_sha(&id), "sha3");

        // a profile added before profiles were linked is found and linked when it matches
        lib.import_bundle(&mut s, &plugin_bundle(&["Play"])).unwrap();
        let old = lib.doc["profiles"][2]["id"].as_str().unwrap().to_string();
        assert!(lib.plugin_profiles("minnion_spotify", "Spotify", false).len() == 1);
        assert!(lib.plugin_profiles("minnion_spotify", "Spotify", true).contains(&old));
        let r = lib.plugin_profile_changed(&mut s, "minnion_spotify", "Spotify", true, &plugin_bundle(&["Play"]), "sha1");
        assert!(matches!(r.as_slice(), [PluginProfile::Edited(..), PluginProfile::Same]));
        assert_eq!(lib.profile_sha(&old), "sha1");

        // someone's own layout that only uses the plugin is never taken for the plugin's profile
        lib.import_bundle(&mut s, &plugin_bundle(&["Mine"])).unwrap();
        lib.doc["profiles"][3]["name"] = json!("Friend's layout");
        let r = lib.plugin_profile_changed(&mut s, "minnion_spotify", "Spotify", true, &plugin_bundle(&["Play", "Next"]), "sha4");
        assert_eq!(r.len(), 2, "the linked one and the adopted one, not the friend's");
        assert_eq!(lib.profile_sha(lib.doc["profiles"][3]["id"].as_str().unwrap()), "");
        assert!(same_profile_name("Spotify 2", "Spotify") && same_profile_name("Spotify", "Spotify"));
        assert!(!same_profile_name("Spotify mine", "Spotify") && !same_profile_name("Spotify", ""));

        // exports and copies don't follow the plugin
        let b = lib.bundle_for(&mut s, &id).unwrap();
        assert!(b["profile"].get("from").is_none());
        let src = lib.doc["profiles"][1].clone();
        assert!(lib.copy_profile(&src).get("from").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Reads the library in build\library (your copy) and writes every profile back without
/// changing anything: the file must come back the same.
///     cargo test --release real_library -- --ignored --nocapture
#[cfg(test)]
mod real_file {
    use super::*;

    fn first_difference(a: &Value, b: &Value, path: &str) -> Option<String> {
        match (a, b) {
            (Value::Object(x), Value::Object(y)) => {
                for k in x.keys().chain(y.keys()) {
                    let p = format!("{path}.{k}");
                    match (x.get(k), y.get(k)) {
                        (Some(u), Some(v)) => {
                            if let Some(d) = first_difference(u, v, &p) {
                                return Some(d);
                            }
                        }
                        (u, v) => return Some(format!("{p}: {u:?} / {v:?}")),
                    }
                }
                None
            }
            (Value::Array(x), Value::Array(y)) => {
                if x.len() != y.len() {
                    return Some(format!("{path}: {} items / {}", x.len(), y.len()));
                }
                x.iter().zip(y).enumerate().find_map(|(i, (u, v))| first_difference(u, v, &format!("{path}[{i}]")))
            }
            _ => (a != b).then(|| format!("{path}: {a} / {b}")),
        }
    }

    #[test]
    #[ignore]
    fn real_library_round_trips() {
        let paths = Paths::find();
        let _ = paths.migrate_legacy();
        let doc = read_doc(&paths.library_file()).expect("build/library/library.mdsave");
        let mut lib = Library { paths, doc: doc.clone(), loaded: String::new(), page_shown: 0, buttons: HashMap::new(), nodes: HashMap::new(), profiles_sig: 0, saved: 0, seen: 0, changed_at: 0.0, force: false, backed_up: true, auto_backup: true };
        let mut s = Sample::empty();
        s.profiles = lib.profiles();
        for i in 0..s.profiles.len() {
            s.active = i;
            lib.load_pages(&mut s, "");
            lib.store_pages(&mut s);
        }
        lib.sync_profiles(&mut s);
        let d = first_difference(&doc, &lib.doc, "data");
        println!("profiles {}, buttons {}", s.profiles.len(), lib.doc["boards"].as_object().map(|b| b.values().map(|v| v.as_array().map(|a| a.len()).unwrap_or(0)).sum::<usize>()).unwrap_or(0));
        assert!(d.is_none(), "changed: {}", d.unwrap_or_default());
    }
}

/// Converts a COPY of the library, settings and pictures in build\ (your real ones are not touched)
/// from plain files to sealed ones, and checks nothing was lost.
///     cargo test --release real_data_seals -- --ignored --nocapture
#[cfg(test)]
mod real_migration {
    use super::*;
    use crate::backend::package::Kind;

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let (p, d) = (e.path(), to.join(e.file_name()));
            if p.is_dir() { copy_dir(&p, &d) } else { std::fs::copy(&p, &d).unwrap(); }
        }
    }

    #[test]
    #[ignore]
    fn real_data_seals_without_losing_anything() {
        let real = Paths::find();
        let tmp = std::env::temp_dir().join(format!("md-real-seal-{}", now_ms()));
        let p = Paths { root: tmp.clone(), library: tmp.join("library"), media: tmp.join("library").join("media"), backups: tmp.join("backups") };
        std::fs::create_dir_all(&p.library).unwrap();
        // the plain files, as an earlier version left them (from the real folder if it still has them)
        let lib_plain = if real.legacy_library_file().is_file() { std::fs::read(real.legacy_library_file()).unwrap() } else { vault_bytes(&real.library_file()) };
        std::fs::write(p.legacy_library_file(), &lib_plain).unwrap();
        let set_plain = if real.legacy_settings_file().is_file() { Some(std::fs::read(real.legacy_settings_file()).unwrap()) } else { None };
        if let Some(s) = &set_plain {
            std::fs::write(p.legacy_settings_file(), s).unwrap();
        }
        let mut n_media = 0;
        std::fs::create_dir_all(&p.media).unwrap();
        for e in std::fs::read_dir(&real.media).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let bytes = crate::backend::media::read_bytes(&e.path()).unwrap();
            std::fs::write(p.media.join(name.trim_end_matches(".mdmedia")), bytes).unwrap();
            n_media += 1;
        }
        let before: Value = serde_json::from_slice(&lib_plain).unwrap();
        assert!(p.migrate_legacy().is_none(), "nothing could be left behind");
        // sealed, the plain ones gone, and the same inside
        assert!(!p.legacy_library_file().exists() && p.library_file().is_file());
        let after = read_doc(&p.library_file()).unwrap();
        assert_eq!(after, before.get("data").cloned().unwrap_or(before.clone()), "the library is the same inside");
        if set_plain.is_some() {
            assert!(!p.legacy_settings_file().exists() && p.settings_file().is_file());
        }
        let sealed_media = std::fs::read_dir(&p.media).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".mdmedia")).count();
        assert_eq!(sealed_media, n_media, "every picture is sealed");
        assert!(std::fs::read_dir(&p.media).unwrap().flatten().all(|e| e.file_name().to_string_lossy().ends_with(".mdmedia")), "no plain picture is left");
        println!("sealed a copy: {} profiles, {} pictures, library {} bytes plain -> {} sealed", after["profiles"].as_array().map_or(0, |a| a.len()), n_media, lib_plain.len(), std::fs::metadata(p.library_file()).unwrap().len());
        // running it again changes nothing
        assert!(p.migrate_legacy().is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn vault_bytes(f: &Path) -> Vec<u8> {
        crate::backend::vault::read(f, Kind::Save).unwrap()
    }
}

/// The id of the first page in a profile's tree (folders searched in order).
fn fn_first_page(tree: Option<&Value>) -> String {
    for n in tree.and_then(|t| t.as_array()).into_iter().flatten() {
        if str_of(n, "kind") == "folder" {
            let id = fn_first_page(n.get("children"));
            if !id.is_empty() {
                return id;
            }
        } else {
            return str_of(n, "id").to_string();
        }
    }
    String::new()
}
