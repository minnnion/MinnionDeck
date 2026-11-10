//! Installing plugins from the marketplace: finds a plugin in the marketplace's index.json,
//! downloads its .mdplugin, checks the size and SHA-256 the index gives, installs it and starts it.
//! Ported from the Minniondeck app's market.rs.
//!
//! WHERE THE MARKETPLACE IS is not in the code: it is <data>\market.json, made on the first start:
//!
//!   { "index_url": "https://raw.githubusercontent.com/<owner>/<repo>/main/index.json",
//!     "files_base": "https://raw.githubusercontent.com/<owner>/<repo>/main" }
//!
//! `index_url` is the list ({ "plugins": [ { id, name, version, size, sha256, versions: [...] } ] }).
//! `files_base` is where the files are: <files_base>/plugins/<id>/<id>-<version>.mdplugin and a
//! plugin's profile at <files_base>/plugins/<id>/<file>. To use a new GitHub repository, change
//! the two addresses in that file (https only) and restart; nothing else needs to change.
#![allow(dead_code)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::plugins::safe_name;
use super::util::write_atomic;

/// Where the plugins are, until <data>\market.json says otherwise: the Minnion-Plugins
/// repository (index.json, and plugins/<id>/ with each plugin's .mdplugin files and .mdprofile).
const DEFAULT_OWNER: &str = "minnnion";
const DEFAULT_REPO: &str = "Minnion-Plugins";
/// The repository plugins used to come from: a market.json that still names it means the default.
const OLD_REPO: &str = "/minnnion/Minnion-Marketplace/";

const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
/// Plugins bigger than this are refused, whatever the index says
const MAX_PLUGIN_BYTES: u64 = 512 * 1024 * 1024;

/// Bumped for every download and by `cancel`; a download stops when it is no longer the latest
static DOWNLOAD: AtomicU64 = AtomicU64::new(0);

/// Where the list and the files come from.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub index_url: String,
    pub files_base: String,
}

impl Source {
    pub fn default_source() -> Source {
        let base = format!("https://raw.githubusercontent.com/{DEFAULT_OWNER}/{DEFAULT_REPO}/main");
        Source { index_url: format!("{base}/index.json"), files_base: base }
    }

    /// Reads <root>\market.json. A missing one is made with the defaults, so it can be edited;
    /// an unusable one (not https) falls back to the defaults.
    pub fn load(root: &Path) -> Source {
        let file = root.join("market.json");
        let def = Source::default_source();
        match fs::read_to_string(&file) {
            Ok(text) => match Source::parse(&text) {
                // made by an older build with the old address: moved on to the new one
                Some(s) if s.index_url.contains(OLD_REPO) => {
                    let _ = write_atomic(&file, serde_json::to_string_pretty(&json!({ "index_url": def.index_url, "files_base": def.files_base })).unwrap_or_default().as_bytes());
                    def
                }
                Some(s) => s,
                None => def,
            },
            Err(_) => {
                let _ = write_atomic(&file, serde_json::to_string_pretty(&json!({ "index_url": def.index_url, "files_base": def.files_base })).unwrap_or_default().as_bytes());
                def
            }
        }
    }

    fn parse(text: &str) -> Option<Source> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.trim().trim_end_matches('/').to_string());
        let (index_url, files_base) = (get("index_url")?, get("files_base")?);
        let https = |u: &str| u.len() <= 500 && u.starts_with("https://") && !u.chars().any(|c| c.is_whitespace() || c.is_control());
        (https(&index_url) && https(&files_base)).then_some(Source { index_url, files_base })
    }

    /// plugins/<id>/<id>-<version>.mdplugin
    fn file_url(&self, id: &str, version: &str) -> String {
        format!("{}/plugins/{id}/{id}-{version}.mdplugin", self.files_base)
    }

    /// A plugin's profile, in its folder next to its versions
    fn profile_url(&self, id: &str, file: &str) -> String {
        format!("{}/plugins/{id}/{file}", self.files_base)
    }
}

#[derive(Deserialize)]
struct Index {
    plugins: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    name: String,
    version: String,
    size: u64,
    sha256: String,
    #[serde(default)]
    versions: Vec<Version>,
    /// A ready-made profile that comes with the plugin
    #[serde(default)]
    profile: Option<ProfileFile>,
}

#[derive(Deserialize)]
struct Version {
    v: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct ProfileFile {
    name: String,
    file: String,
    #[serde(default)]
    pages: u32,
    size: u64,
    sha256: String,
}

/// Profiles are small; anything bigger is refused whatever the list says
const MAX_PROFILE_BYTES: u64 = 50 * 1024 * 1024;

fn profile_ok(p: &ProfileFile) -> bool {
    safe_name(&p.file) && p.file.ends_with(".mdprofile") && p.size > 0 && p.size <= MAX_PROFILE_BYTES && p.sha256.len() == 64 && p.sha256.chars().all(|c| c.is_ascii_hexdigit())
}

impl Entry {
    /// Size and SHA-256 of one version, the latest unless asked for another
    fn file(&self, version: &str) -> Option<(u64, String)> {
        if version == self.version {
            return Some((self.size, self.sha256.to_ascii_lowercase()));
        }
        self.versions.iter().find(|x| x.v == version).map(|x| (x.size, x.sha256.to_ascii_lowercase()))
    }
}

fn agent() -> ureq::Agent {
    // Read timeouts rather than one for the whole request, so big plugins have time to arrive
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent(&format!("Minniondeck/{} (plugin marketplace)", env!("CARGO_PKG_VERSION")))
        .build()
}

fn get(url: &str, what: &str) -> Result<ureq::Response, String> {
    agent().get(url).call().map_err(|e| match e {
        ureq::Error::Status(404, _) => format!("{what} isn't in the marketplace yet."),
        ureq::Error::Status(code, _) => format!("The marketplace didn't send {what} (it answered {code})."),
        _ => "Couldn't reach the marketplace. Check your internet connection and try again.".to_string(),
    })
}

/// index.json from the marketplace, as text
pub fn fetch_index_text(src: &Source) -> Result<String, String> {
    let mut text = String::new();
    get(&src.index_url, "The list")?.into_reader().take(MAX_INDEX_BYTES).read_to_string(&mut text).map_err(|e| e.to_string())?;
    Ok(text)
}

fn fetch_index(src: &Source) -> Result<Index, String> {
    serde_json::from_str(&fetch_index_text(src)?).map_err(|_| "The marketplace list couldn't be read.".to_string())
}

/// The plugin and the version asked for, from the marketplace list: (entry, version, size, sha256)
fn find(index: Index, id: &str, version: Option<&str>) -> Result<(Entry, String, u64, String), String> {
    let entry = index.plugins.into_iter().find(|p| p.id == id).ok_or("That plugin isn't in the marketplace.")?;
    let version = version.map(String::from).unwrap_or_else(|| entry.version.clone());
    let (size, sha) = entry.file(&version).ok_or_else(|| format!("Version {version} of {} isn't in the marketplace.", entry.name))?;
    if size == 0 || size > MAX_PLUGIN_BYTES || sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("The marketplace's entry for {} is incomplete.", entry.name));
    }
    Ok((entry, version, size, sha))
}

/// Downloads wait in a dot-folder, which the plugin list skips and start-up clears
pub fn downloads_dir(root: &Path) -> PathBuf {
    root.join("plugins").join(".downloads")
}

/// Copy `from` into `to`, hashing as it goes. Stops on Cancel and when more arrives than expected.
fn save_checked(mut from: impl Read, to: &Path, size: u64, sha: &str, mine: u64, mut progress: impl FnMut(u64)) -> Result<(), String> {
    let mut out = fs::File::create(to).map_err(|e| format!("Couldn't save the download: {e}"))?;
    let mut hasher = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = vec![0u8; 64 * 1024];
    let mut got = 0u64;
    loop {
        if DOWNLOAD.load(Ordering::SeqCst) != mine {
            return Err("Download cancelled.".into());
        }
        let n = from.read(&mut buf).map_err(|_| "The download stopped. Check your internet connection and try again.".to_string())?;
        if n == 0 {
            break;
        }
        got += n as u64;
        if got > size {
            return Err("The download is bigger than the marketplace says, so it wasn't kept.".into());
        }
        hasher.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("Couldn't save the download: {e}"))?;
        progress(got);
    }
    if got != size {
        return Err("The download ended early. Try again.".into());
    }
    let hex: String = hasher.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if hex != sha {
        return Err("The download doesn't match the marketplace's checksum, so it wasn't kept.".into());
    }
    out.flush().map_err(|e| e.to_string())
}

/// What an install did.
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
}

/// Downloads plugin `id` (the newest version unless `version` is given), checks it, installs it
/// into <root>\\plugins\\ and loads it. `progress(got, total)` is called while it downloads.
/// Blocks; call it from a thread.
pub fn install(root: &Path, id: &str, version: Option<&str>, progress: impl Fn(u64, u64)) -> Result<Installed, String> {
    let (file, name) = download(root, id, version, progress)?;
    let result = super::plugins::install_file(root, &file).map_err(|err| format!("{err}"));
    let _ = fs::remove_file(&file);
    let (m, _, _) = result?;
    super::plugins::log(&format!("Plugin {} {} installed", m.id, m.version));
    super::plugins::reload(Some(m.id.clone()))?;
    Ok(Installed { id: m.id, name, version: m.version })
}

/// Downloads plugin `id`'s .mdplugin and checks its size and SHA-256 against the marketplace's
/// list. Returns the file's path (in <root>plugins.downloads) and the plugin's name.
pub fn download(root: &Path, id: &str, version: Option<&str>, progress: impl Fn(u64, u64)) -> Result<(PathBuf, String), String> {
    if !safe_name(id) {
        return Err("That isn't a plugin id.".into());
    }
    let src = Source::load(root);
    let mine = DOWNLOAD.fetch_add(1, Ordering::SeqCst) + 1;
    let (e, version, size, sha) = find(fetch_index(&src)?, id, version)?;
    let dir = downloads_dir(root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{}-{version}.mdplugin", e.id));
    let resp = get(&src.file_url(&e.id, &version), &format!("{} {version}", e.name))?;
    progress(0, size);
    let saved = save_checked(resp.into_reader(), &file, size, &sha, mine, |got| progress(got, size));
    if let Err(err) = saved {
        let _ = fs::remove_file(&file);
        return Err(err);
    }
    super::plugins::log(&format!("Downloaded plugin {} {version}", e.id));
    Ok((file, e.name))
}

/// Downloads the profile that comes with a plugin and returns its path (for profile_io::import_from).
pub fn download_profile(root: &Path, id: &str) -> Result<PathBuf, String> {
    download_plugin_profile(root, id).map(|d| d.path)
}

/// A plugin's profile as downloaded: the file, its SHA-256 (as the list gives it, checked) and
/// the plugin's name.
pub struct ProfileDownload {
    pub path: PathBuf,
    pub sha: String,
    pub plugin_name: String,
}

pub fn download_plugin_profile(root: &Path, id: &str) -> Result<ProfileDownload, String> {
    let src = Source::load(root);
    let mine = DOWNLOAD.fetch_add(1, Ordering::SeqCst) + 1;
    let (e, _, _, _) = find(fetch_index(&src)?, id, None)?;
    let p = e.profile.filter(profile_ok).ok_or("This plugin doesn't come with a profile.")?;
    let dir = downloads_dir(root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{}-profile.mdprofile", e.id));
    let resp = get(&src.profile_url(&e.id, &p.file), &format!("The {} profile", p.name))?;
    let sha = p.sha256.to_ascii_lowercase();
    if let Err(err) = save_checked(resp.into_reader(), &file, p.size, &sha, mine, |_| {}) {
        let _ = fs::remove_file(&file);
        return Err(err);
    }
    Ok(ProfileDownload { path: file, sha, plugin_name: e.name })
}

/// The SHA-256 of each plugin's ready-made profile, from the list last read (live or the copy kept).
static PROFILE_SHA: std::sync::RwLock<Option<std::collections::HashMap<String, String>>> = std::sync::RwLock::new(None);

/// The SHA-256 of plugin `id`'s ready-made profile as the marketplace lists it now.
pub fn profile_sha(id: &str) -> Option<String> {
    PROFILE_SHA.read().ok()?.as_ref()?.get(id).cloned()
}

// ===== The live list =====

use crate::generated::market_data::{MARKET, MarketPlugin, Release};

/// The list the Marketplace page shows: the marketplace's own (live, or the copy kept from the
/// last time), or the one built into this app while there is none.
static LIVE: std::sync::RwLock<Option<&'static [MarketPlugin]>> = std::sync::RwLock::new(None);

pub fn plugins() -> &'static [MarketPlugin] {
    static BUILT_IN: std::sync::OnceLock<&'static [MarketPlugin]> = std::sync::OnceLock::new();
    LIVE.read().ok().and_then(|l| *l).unwrap_or_else(|| {
        BUILT_IN.get_or_init(|| Box::leak(MARKET.iter().filter(|p| !crate::backend::catalog::is_hidden(p.id)).cloned().collect::<Vec<_>>().into_boxed_slice()))
    })
}

/// Whether the list on show is the marketplace's (and not the built-in one).
pub fn is_live() -> bool {
    LIVE.read().ok().is_some_and(|l| l.is_some())
}

#[derive(Deserialize)]
struct ListEntry {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    official: bool,
    #[serde(default)]
    author: String,
    #[serde(default)]
    color: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    about: String,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    needs: Vec<String>,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    versions: Vec<ListVersion>,
    #[serde(default)]
    profile: Option<ProfileFile>,
}

#[derive(Deserialize)]
struct ListVersion {
    v: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Deserialize)]
struct List {
    plugins: Vec<ListEntry>,
}

/// Kept for the life of the program: the list is replaced a few times at most
fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn leak_all(v: &[String]) -> &'static [&'static str] {
    Box::leak(v.iter().map(|s| leak(s)).collect::<Vec<_>>().into_boxed_slice())
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The marketplace's index.json as the Marketplace page's list. Entries without a usable id are left out.
fn parse_list(text: &str) -> Result<&'static [MarketPlugin], String> {
    let list: List = serde_json::from_str(text).map_err(|_| "The marketplace list couldn't be read.".to_string())?;
    let mut out: Vec<MarketPlugin> = vec![];
    for e in list.plugins.iter().filter(|e| safe_name(&e.id) && !e.name.is_empty() && !crate::backend::catalog::is_hidden(&e.id)) {
        let mut versions: Vec<&ListVersion> = e.versions.iter().collect();
        versions.sort_by(|a, b| version_key(&b.v).cmp(&version_key(&a.v)));
        let releases: Vec<Release> = versions.iter().map(|v| Release { v: leak(&v.v), date: leak(&v.date), title: leak(&v.title), notes: leak_all(&v.notes) }).collect();
        out.push(MarketPlugin {
            id: leak(&e.id),
            name: leak(&e.name),
            version: leak(&e.version),
            category: leak(if e.category.is_empty() { "general" } else { &e.category }),
            official: e.official,
            author: leak(&e.author),
            colour: leak(if e.color.is_empty() { "mint" } else { &e.color }),
            summary: leak(&e.summary),
            about: leak(&e.about),
            actions: leak_all(&e.actions),
            needs: leak_all(&e.needs),
            size: e.size,
            profile_pages: e.profile.as_ref().filter(|p| profile_ok(p)).map_or(0, |p| p.pages),
            published: true,
            releases: Box::leak(releases.into_boxed_slice()),
        });
    }
    if out.is_empty() {
        return Err("The marketplace has no plugins in it yet.".into());
    }
    let shas: std::collections::HashMap<String, String> =
        list.plugins.iter().filter_map(|e| e.profile.as_ref().filter(|p| profile_ok(p)).map(|p| (e.id.clone(), p.sha256.to_ascii_lowercase()))).collect();
    if let Ok(mut m) = PROFILE_SHA.write() {
        *m = Some(shas);
    }
    Ok(Box::leak(out.into_boxed_slice()))
}

fn cache_file(root: &Path) -> PathBuf {
    root.join("market-cache.json")
}

/// The list kept from the last time, if any: the Marketplace page has something to show before
/// (and without) the network. Returns how many plugins.
pub fn load_cache(root: &Path) -> Option<usize> {
    let text = fs::read_to_string(cache_file(root)).ok()?;
    let list = parse_list(&text).ok()?;
    if let Ok(mut l) = LIVE.write() {
        *l = Some(list);
    }
    for p in list {
        if let Ok(bytes) = fs::read(icon_file(root, p.id)) {
            crate::ui::images::register_live(&format!("market/{}", p.id), bytes);
        }
    }
    Some(list.len())
}

fn icon_file(root: &Path, id: &str) -> PathBuf {
    root.join("market-icons").join(format!("{id}.png"))
}

/// Each plugin's picture from the marketplace (plugins/<id>/icon.png), for its card: kept in
/// <data>\market-icons\ and shown as "png:market/<id>". Blocks; call it from a thread.
fn fetch_icons(root: &Path, src: &Source, list: &[MarketPlugin]) {
    let _ = fs::create_dir_all(root.join("market-icons"));
    for p in list.iter().filter(|p| safe_name(p.id)) {
        let url = format!("{}/plugins/{}/icon.png", src.files_base, p.id);
        let Ok(resp) = agent().get(&url).call() else { continue };
        let mut bytes = Vec::new();
        if resp.into_reader().take(256 * 1024).read_to_end(&mut bytes).is_err() || !bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
            continue;
        }
        let _ = write_atomic(&icon_file(root, p.id), &bytes);
        crate::ui::images::register_live(&format!("market/{}", p.id), bytes);
    }
}

/// The picture for plugin `id`'s card: the marketplace's own, once it has come.
pub fn card_icon(id: &str) -> Option<String> {
    let key = format!("market/{id}");
    crate::ui::images::has_live(&key).then(|| format!("png:{key}"))
}

/// Reads the marketplace's list again (from the address in market.json), keeps a copy, and shows
/// it. Returns how many plugins. Blocks; call it from a thread.
pub fn refresh(root: &Path) -> Result<usize, String> {
    let src = Source::load(root);
    let text = fetch_index_text(&src)?;
    let list = parse_list(&text)?;
    let _ = write_atomic(&cache_file(root), text.as_bytes());
    if let Ok(mut l) = LIVE.write() {
        *l = Some(list);
    }
    fetch_icons(root, &src, list);
    Ok(list.len())
}

/// Stop the download that is running, if any
pub fn cancel() {
    DOWNLOAD.fetch_add(1, Ordering::SeqCst);
}

/// Downloads a plugin from the live marketplace into a temporary folder and installs it there
/// (not into your data folder), to check the whole path: list, download, checksum, unpack,
/// signature.
///     cargo test --release real_marketplace -- --ignored --nocapture
#[cfg(test)]
mod real_market {
    use super::*;

    #[test]
    #[ignore]
    fn real_marketplace_download_and_install() {
        let root = std::env::temp_dir().join(format!("md-real-market-{}", super::super::util::now_ms()));
        fs::create_dir_all(&root).unwrap();
        let text = fetch_index_text(&Source::default_source()).expect("the marketplace list");
        let index: Index = serde_json::from_str(&text).unwrap();
        println!("marketplace has {} plugins: {:?}", index.plugins.len(), index.plugins.iter().map(|p| format!("{} {}", p.id, p.version)).collect::<Vec<_>>());
        let live = refresh(&root).expect("the live list");
        println!("the Marketplace page would list {live} plugins; cached: {}", cache_file(&root).is_file());
        assert!(is_live() && plugins().len() == live);
        assert_eq!(load_cache(&root), Some(live));
        let id = std::env::var("MD_PLUGIN").unwrap_or_else(|_| "minnion_timer".into());
        let seen = std::cell::Cell::new(0u64);
        let (file, name) = download(&root, &id, None, |got, _| seen.set(got)).unwrap();
        println!("downloaded {name}: {} bytes", seen.get());
        let (m, dest, trust) = super::super::plugins::install_file(&root, &file).unwrap();
        println!("installed {} {} -> {}, signature {:?}", m.id, m.version, dest.display(), trust);
        assert!(dest.join(&m.dll).is_file());
        assert!(trust.trusted(), "a marketplace plugin should be signed by a trusted author");
        // the starter profile of Spotify: downloaded, checked, opened
        let pf = download_profile(&root, "minnion_spotify").unwrap();
        let media = root.join("media");
        fs::create_dir_all(&media).unwrap();
        let b = super::super::profile_io::import_from(&media, &pf).unwrap();
        println!("starter profile {}: {} pages/folders", b["manifest"]["name"], b["profile"]["tree"].as_array().map_or(0, |t| t.len()));
        assert!(b["profile"]["tree"].as_array().is_some_and(|t| !t.is_empty()));
        let _ = fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Index {
        serde_json::from_str(&format!(
            r#"{{"plugins":[{{"id":"p","name":"P","version":"2.0.0","size":10,"sha256":"{a}",
                "versions":[{{"v":"2.0.0","size":10,"sha256":"{a}"}},{{"v":"1.0.0","size":5,"sha256":"{b}"}}]}}]}}"#,
            a = "a".repeat(64),
            b = "B".repeat(64)
        ))
        .unwrap()
    }

    #[test]
    fn the_marketplace_address_comes_from_market_json() {
        let dir = std::env::temp_dir().join(format!("md-market-src-{}", super::super::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        // none yet: the defaults, and a file is made to edit
        let d = Source::load(&dir);
        assert_eq!(d, Source::default_source());
        assert!(dir.join("market.json").is_file());
        assert_eq!(Source::load(&dir), d);
        assert!(d.index_url.contains("/minnnion/Minnion-Plugins/"), "the plugins repository by default");
        // a market.json from an older build, naming the old repository, moves on to the new one
        fs::write(dir.join("market.json"), r#"{"index_url":"https://raw.githubusercontent.com/minnnion/Minnion-Marketplace/main/index.json","files_base":"https://raw.githubusercontent.com/minnnion/Minnion-Marketplace/main"}"#).unwrap();
        assert_eq!(Source::load(&dir), d);
        assert!(fs::read_to_string(dir.join("market.json")).unwrap().contains("Minnion-Plugins"));
        // a new repository
        fs::write(dir.join("market.json"), r#"{"index_url":"https://raw.githubusercontent.com/me/new/main/index.json","files_base":"https://raw.githubusercontent.com/me/new/main/"}"#).unwrap();
        let s = Source::load(&dir);
        assert_eq!(s.index_url, "https://raw.githubusercontent.com/me/new/main/index.json");
        assert_eq!(s.file_url("a", "1.0.0"), "https://raw.githubusercontent.com/me/new/main/plugins/a/a-1.0.0.mdplugin");
        assert_eq!(s.profile_url("a", "A.mdprofile"), "https://raw.githubusercontent.com/me/new/main/plugins/a/A.mdprofile");
        // only https; anything else is ignored
        fs::write(dir.join("market.json"), r#"{"index_url":"http://x/index.json","files_base":"https://x"}"#).unwrap();
        assert_eq!(Source::load(&dir), Source::default_source());
        fs::write(dir.join("market.json"), "not json").unwrap();
        assert_eq!(Source::load(&dir), Source::default_source());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_asked_for_version_is_found_with_its_own_checksum() {
        let (_, v, size, sha) = find(index(), "p", None).unwrap();
        assert_eq!((v.as_str(), size, sha), ("2.0.0", 10, "a".repeat(64)));
        let (_, v, size, sha) = find(index(), "p", Some("1.0.0")).unwrap();
        assert_eq!((v.as_str(), size, sha), ("1.0.0", 5, "b".repeat(64)));
        assert!(find(index(), "p", Some("3.0.0")).is_err());
        assert!(find(index(), "q", None).is_err());
    }

    #[test]
    fn a_download_is_kept_only_when_size_and_checksum_match() {
        let dir = std::env::temp_dir().join(format!("md-market-dl-{}", super::super::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.mdplugin");
        let data = b"hello plugin";
        let sha = super::super::util::sha256_hex(data);
        let mine = DOWNLOAD.load(Ordering::SeqCst);
        assert!(save_checked(&data[..], &file, data.len() as u64, &sha, mine, |_| {}).is_ok());
        assert!(save_checked(&data[..], &file, data.len() as u64, &"0".repeat(64), mine, |_| {}).unwrap_err().contains("checksum"));
        assert!(save_checked(&data[..], &file, 4, &sha, mine, |_| {}).unwrap_err().contains("bigger"));
        assert!(save_checked(&data[..], &file, 100, &sha, mine, |_| {}).unwrap_err().contains("early"));
        assert!(save_checked(&data[..], &file, data.len() as u64, &sha, mine + 1, |_| {}).unwrap_err().contains("cancelled"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_marketplace_list_becomes_the_pages_list() {
        let text = format!(
            r#"{{"schema":1,"plugins":[
              {{"id":"a","name":"Alpha","version":"1.2.0","category":"games","official":true,"author":"Me","color":"sky","summary":"S","about":"A",
                "actions":["One","Two"],"needs":["Sim"],"size":10,"sha256":"{h}",
                "versions":[{{"v":"1.0.0","size":5,"sha256":"{h}"}},{{"v":"1.2.0","date":"2026-11-01","title":"New","notes":["Faster"],"size":10,"sha256":"{h}"}}],
                "profile":{{"name":"P","file":"P.mdprofile","pages":3,"size":10,"sha256":"{h}"}}}},
              {{"id":"../bad","name":"Bad","version":"1.0.0"}},
              {{"id":"b","name":"Beta","version":"0.1.0"}}]}}"#,
            h = "a".repeat(64)
        );
        let list = parse_list(&text).unwrap();
        assert_eq!(list.len(), 2, "an entry with an unsafe id is left out");
        let a = &list[0];
        assert_eq!((a.id, a.name, a.version, a.category, a.colour, a.author), ("a", "Alpha", "1.2.0", "games", "sky", "Me"));
        assert_eq!((a.actions, a.needs, a.profile_pages, a.official, a.published), (&["One", "Two"][..], &["Sim"][..], 3, true, true));
        assert_eq!((a.releases[0].v, a.releases[0].title, a.releases[0].notes), ("1.2.0", "New", &["Faster"][..]), "newest first, with its notes");
        assert_eq!(a.releases[1].v, "1.0.0");
        // defaults for what an entry leaves out
        let b = &list[1];
        assert_eq!((b.category, b.colour, b.profile_pages), ("general", "mint", 0));
        assert!(parse_list("not json").is_err());
        assert!(parse_list(r#"{"plugins":[]}"#).is_err());
    }

    #[test]
    fn profiles_are_checked() {
        let ok = |file: &str, size: u64, sha: &str| profile_ok(&ProfileFile { name: "P".into(), file: file.into(), pages: 1, size, sha256: sha.into() });
        let sha = "a".repeat(64);
        assert!(ok("Spotify.mdprofile", 10, &sha));
        assert!(!ok("../Spotify.mdprofile", 10, &sha));
        assert!(!ok("Spotify.mdplugin", 10, &sha));
        assert!(!ok("Spotify.mdprofile", 0, &sha));
        assert!(!ok("Spotify.mdprofile", 10, "abc"));
    }
}
