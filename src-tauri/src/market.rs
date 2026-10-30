// Installing plugins from the marketplace website.
//
// The website's Install button opens a minniondeck://install?id=<plugin id>&v=<version> link.
// Windows starts Minniondeck with it (or hands it to the running copy, see lib.rs), and the UI
// shows a popup: Minniondeck looks the plugin up in the marketplace's index.json on GitHub,
// downloads its .mdplugin from GitHub Releases, checks the SHA-256 the index gives, and then asks
// before installing it, as it does for a double-clicked file.
//
// A link only names a plugin and a version. Where the list and the files come from is fixed here,
// so a web page can never point Minniondeck at a file of its own.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

/// The GitHub account that holds the marketplace repository
pub const GITHUB_OWNER: &str = "minniondeck";
/// The repository with index.json on its main branch and every plugin file in its Releases
const REPO: &str = "minniondeck-content";
/// The link scheme the website's Install button uses
pub const SCHEME: &str = "minniondeck";

const MAX_INDEX_BYTES: u64 = 2 * 1024 * 1024;
/// Plugins bigger than this are refused, whatever the index says
const MAX_PLUGIN_BYTES: u64 = 512 * 1024 * 1024;

/// Bumped for every download and by Cancel; a download stops when it is no longer the latest
static DOWNLOAD: AtomicU64 = AtomicU64::new(0);

fn index_url() -> String {
    format!("https://raw.githubusercontent.com/{GITHUB_OWNER}/{REPO}/main/index.json")
}

/// Where a plugin version's file is: a release named <id>-v<version> holding <id>-<version>.mdplugin
fn file_url(id: &str, version: &str) -> String {
    format!("https://github.com/{GITHUB_OWNER}/{REPO}/releases/download/{id}-v{version}/{id}-{version}.mdplugin")
}

/// What an install link asks for
#[derive(Debug, PartialEq)]
pub struct InstallLink {
    pub id: String,
    pub version: Option<String>,
}

fn safe_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= 32 && v.starts_with(|c: char| c.is_ascii_digit()) && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = s.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Read minniondeck://install?id=…&v=… (browsers may add a slash after install, or encode the values).
/// Anything else, such as the phone's minniondeck://pair links, is not an install link.
pub fn parse_link(link: &str) -> Option<InstallLink> {
    let link = link.trim();
    if !link.get(..SCHEME.len())?.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let rest = link[SCHEME.len()..].strip_prefix(':')?.trim_start_matches('/');
    let (what, query) = rest.split_once('?')?;
    if !what.trim_end_matches('/').eq_ignore_ascii_case("install") {
        return None;
    }
    let (mut id, mut version) = (None, None);
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = percent_decode(v)?;
        match k {
            "id" => id = Some(v),
            "v" if !v.is_empty() => version = Some(v),
            _ => {}
        }
    }
    let id = id.filter(|i| crate::plugins::safe_name(i))?;
    if version.as_deref().is_some_and(|v| !safe_version(v)) {
        return None;
    }
    Some(InstallLink { id, version })
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
    #[serde(default)]
    summary: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    official: bool,
    #[serde(default)]
    category: String,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    needs: Vec<String>,
    size: u64,
    sha256: String,
    #[serde(default)]
    versions: Vec<Version>,
}

#[derive(Deserialize)]
struct Version {
    v: String,
    size: u64,
    sha256: String,
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

/// A plugin as the popup shows it before downloading
#[derive(Serialize)]
pub struct Found {
    id: String,
    name: String,
    version: String,
    latest: String,
    summary: String,
    author: String,
    official: bool,
    category: String,
    keys: usize,
    needs: Vec<String>,
    size: u64,
    /// The version already installed, if any
    installed: Option<String>,
}

fn agent() -> ureq::Agent {
    // Read timeouts rather than one for the whole request, so big plugins have time to arrive
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent(&format!("Minniondeck/{} (plugin marketplace)", env!("CARGO_PKG_VERSION")))
        .build()
}

fn fetch_index() -> Result<Index, String> {
    let resp = agent().get(&index_url()).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("The marketplace list isn't available right now (GitHub answered {code})."),
        _ => "Couldn't reach GitHub. Check your internet connection and try again.".to_string(),
    })?;
    let mut text = String::new();
    resp.into_reader().take(MAX_INDEX_BYTES).read_to_string(&mut text).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|_| "The marketplace list couldn't be read.".to_string())
}

/// The plugin and the version a link asks for, from the marketplace list
fn find(index: Index, link: &InstallLink) -> Result<(Entry, String, u64, String), String> {
    let entry = index.plugins.into_iter().find(|p| p.id == link.id).ok_or("That plugin isn't in the marketplace.")?;
    let version = link.version.clone().unwrap_or_else(|| entry.version.clone());
    let (size, sha) = entry.file(&version).ok_or_else(|| format!("Version {version} of {} isn't in the marketplace.", entry.name))?;
    if size == 0 || size > MAX_PLUGIN_BYTES || sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("The marketplace's entry for {} is incomplete.", entry.name));
    }
    Ok((entry, version, size, sha))
}

fn installed_version(root: &Path, id: &str) -> Option<String> {
    let text = fs::read_to_string(root.join("plugins").join(id).join("manifest.json")).ok()?;
    let m: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(m.get("version").and_then(|v| v.as_str()).unwrap_or("").to_string())
}

/// Downloads wait in a dot-folder, which the plugin list skips and start-up clears
fn downloads_dir(root: &Path) -> PathBuf {
    root.join("plugins").join(".downloads")
}

/// What an install link names, looked up in the marketplace list
#[tauri::command]
pub async fn market_lookup(link: String) -> Result<Found, String> {
    let link = parse_link(&link).ok_or("That isn't a Minniondeck install link.")?;
    let root = crate::plugins::data_root()?;
    tauri::async_runtime::spawn_blocking(move || {
        let (e, version, size, _) = find(fetch_index()?, &link)?;
        Ok(Found {
            installed: installed_version(&root, &e.id),
            keys: e.actions.len(),
            id: e.id,
            name: e.name,
            latest: e.version,
            version,
            summary: e.summary,
            author: e.author,
            official: e.official,
            category: e.category,
            needs: e.needs,
            size,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Copy `from` into `to`, hashing as it goes. Stops on Cancel and when more arrives than expected.
fn save_checked(mut from: impl Read, to: &Path, size: u64, sha: &str, mine: u64, mut progress: impl FnMut(u64)) -> Result<(), String> {
    let mut out = fs::File::create(to).map_err(|e| format!("Couldn't save the download: {e}"))?;
    let mut hasher = Sha256::new();
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
    if format!("{:x}", hasher.finalize()) != sha {
        return Err("The download doesn't match the marketplace's checksum, so it wasn't kept.".into());
    }
    out.flush().map_err(|e| e.to_string())
}

/// Download a plugin from GitHub Releases and check it. Progress goes out as market_progress
/// events; the answer is the file's path, for inspect_plugin and install_plugin.
#[tauri::command]
pub async fn market_download(app: AppHandle, link: String) -> Result<String, String> {
    let link = parse_link(&link).ok_or("That isn't a Minniondeck install link.")?;
    let root = crate::plugins::data_root()?;
    let mine = DOWNLOAD.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let (e, version, size, sha) = find(fetch_index()?, &link)?;
        let dir = downloads_dir(&root);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let file = dir.join(format!("{}-{version}.mdplugin", e.id));
        // GitHub sends release files on from its own download host; that redirect is followed
        let resp = agent().get(&file_url(&e.id, &version)).call().map_err(|err| match err {
            ureq::Error::Status(404, _) => format!("{} {version} isn't on GitHub Releases yet.", e.name),
            ureq::Error::Status(code, _) => format!("GitHub didn't send the file (it answered {code})."),
            _ => "Couldn't reach GitHub. Check your internet connection and try again.".to_string(),
        })?;
        let mut last = Instant::now();
        let emit = |got: u64| {
            let _ = app.emit("market_progress", json!({ "id": e.id, "version": version, "got": got, "total": size }));
        };
        emit(0);
        let saved = save_checked(resp.into_reader(), &file, size, &sha, mine, |got| {
            if last.elapsed() >= Duration::from_millis(100) || got == size {
                last = Instant::now();
                emit(got);
            }
        });
        if let Err(err) = saved {
            let _ = fs::remove_file(&file);
            return Err(err);
        }
        crate::applog::line(&format!("Downloaded plugin {} {version} from GitHub", e.id));
        Ok(file.display().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The website's address for one of its pages. GIFs and button pictures are made there now and
/// come into the app with Import in the Edit button window.
fn site_url(page: &str) -> Option<String> {
    matches!(page, "gifs" | "images" | "marketplace" | "docs").then(|| format!("https://{GITHUB_OWNER}.github.io/#{page}"))
}

/// Open a page of the website in the browser. Only the pages named above, never an address from the UI.
#[tauri::command]
pub fn open_site(page: String) -> Result<(), String> {
    crate::actions::open_url(&site_url(&page).ok_or("That isn't a page of the Minniondeck website.")?)
}

/// Stop the download that is running, if any
#[tauri::command]
pub fn market_cancel() {
    DOWNLOAD.fetch_add(1, Ordering::SeqCst);
}

/// Delete a downloaded plugin file once it is installed or no longer wanted
#[tauri::command]
pub fn market_discard(path: String) -> Result<(), String> {
    let dir = downloads_dir(&crate::plugins::data_root()?);
    let path = PathBuf::from(path);
    // Only files this module saved: straight inside the downloads folder
    if path.parent() == Some(dir.as_path()) && path.file_name().and_then(|n| n.to_str()).is_some_and(crate::plugins::safe_name) {
        let _ = fs::remove_file(&path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: &str, v: Option<&str>) -> Option<InstallLink> {
        Some(InstallLink { id: id.into(), version: v.map(Into::into) })
    }

    #[test]
    fn install_links_are_read_in_the_forms_browsers_send() {
        assert_eq!(parse_link("minniondeck://install?id=minnion_spotify"), link("minnion_spotify", None));
        assert_eq!(parse_link("minniondeck://install/?id=minnion_spotify&v=0.2.0"), link("minnion_spotify", Some("0.2.0")));
        assert_eq!(parse_link("MinnionDeck://Install?v=0.26.6&id=minnioncore_deck"), link("minnioncore_deck", Some("0.26.6")));
        assert_eq!(parse_link("minniondeck:install?id=minnion%5Flights&v="), link("minnion_lights", None));
        assert_eq!(parse_link(" minniondeck://install?id=x&utm=1 "), link("x", None));
    }

    #[test]
    fn other_links_and_unsafe_values_are_refused() {
        assert_eq!(parse_link("minniondeck://pair?v=1&h=192.168.1.20&p=47800"), None);
        assert_eq!(parse_link("https://install?id=x"), None);
        assert_eq!(parse_link("minniondeck://install"), None);
        assert_eq!(parse_link("minniondeck://install?v=1.0"), None);
        assert_eq!(parse_link("minniondeck://install?id=..%2F..%2Fwindows"), None);
        assert_eq!(parse_link("minniondeck://install?id=.hidden"), None);
        assert_eq!(parse_link("minniondeck://install?id=ok&v=1.0/../../x"), None);
        assert_eq!(parse_link("minniondeck://install?id=ok&v=latest"), None);
        assert_eq!(parse_link("minniondeck://install?id=ok%ZZ"), None);
        assert_eq!(parse_link("minniondeckx://install?id=ok"), None);
    }

    #[test]
    fn files_come_from_the_marketplace_repository_only() {
        assert_eq!(
            file_url("minnion_spotify", "0.2.1"),
            format!("https://github.com/{GITHUB_OWNER}/{REPO}/releases/download/minnion_spotify-v0.2.1/minnion_spotify-0.2.1.mdplugin")
        );
        assert!(index_url().starts_with("https://raw.githubusercontent.com/"));
    }

    #[test]
    fn only_the_websites_own_pages_open() {
        assert_eq!(site_url("gifs"), Some(format!("https://{GITHUB_OWNER}.github.io/#gifs")));
        assert!(site_url("images").is_some());
        assert_eq!(site_url("https://evil.example"), None);
        assert_eq!(site_url(""), None);
    }

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
    fn the_asked_for_version_is_found_with_its_own_checksum() {
        let (_, v, size, sha) = find(index(), &InstallLink { id: "p".into(), version: None }).unwrap();
        assert_eq!((v.as_str(), size, sha), ("2.0.0", 10, "a".repeat(64)));
        let (_, v, size, sha) = find(index(), &InstallLink { id: "p".into(), version: Some("1.0.0".into()) }).unwrap();
        assert_eq!((v.as_str(), size, sha), ("1.0.0", 5, "b".repeat(64)));
        assert!(find(index(), &InstallLink { id: "p".into(), version: Some("3.0.0".into()) }).is_err());
        assert!(find(index(), &InstallLink { id: "q".into(), version: None }).is_err());
    }

    #[test]
    fn a_download_is_kept_only_when_size_and_checksum_match() {
        let dir = std::env::temp_dir().join(format!("md-market-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.mdplugin");
        let data = b"hello plugin";
        let sha = format!("{:x}", Sha256::digest(data));
        let mine = DOWNLOAD.load(Ordering::SeqCst);
        assert!(save_checked(&data[..], &file, data.len() as u64, &sha, mine, |_| {}).is_ok());
        assert!(save_checked(&data[..], &file, data.len() as u64, &"0".repeat(64), mine, |_| {}).unwrap_err().contains("checksum"));
        assert!(save_checked(&data[..], &file, 4, &sha, mine, |_| {}).unwrap_err().contains("bigger"));
        assert!(save_checked(&data[..], &file, 100, &sha, mine, |_| {}).unwrap_err().contains("early"));
        assert!(save_checked(&data[..], &file, data.len() as u64, &sha, mine + 1, |_| {}).unwrap_err().contains("cancelled"));
        let _ = fs::remove_dir_all(&dir);
    }
}
