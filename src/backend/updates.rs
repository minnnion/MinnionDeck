//! Updates for Minniondeck itself, from a GitHub repository's Releases. Ported from the Minniondeck
//! app's app_update.rs.
//!
//! Settings › Updates (and a daily check at start-up) asks GitHub for the latest release. If it is
//! newer than this copy, the user can download its Windows installer: this app saves it in the
//! temp folder, checks its size and the SHA-256 GitHub lists for it, then starts it and closes so
//! the installer can replace the app. A download without GitHub's checksum is refused.
//!
//! The installer must also be signed: the release needs `<installer>.sig` beside it, made with
//! `Minniondeck.exe --sign-release <installer.exe>` on the PC that has the minnion author key. It
//! is checked against the built-in authors' keys (signing.rs); an unsigned release is refused.
//!
//! WHERE RELEASES COME FROM is not in the code: it is <data>\update.json, made on the first start
//! with the three addresses empty, which means "updates aren't set up yet" (this app is not the
//! Tauri app, so it must never offer that app's releases as its own update). Fill it in:
//!
//!   { "latest_url": "https://api.github.com/repos/<owner>/<repo>/releases/latest",
//!     "download_prefix": "https://github.com/<owner>/<repo>/releases/download/",
//!     "page_prefix": "https://github.com/<owner>/<repo>/",
//!     "installer_suffix": "-setup.exe",
//!     "installer_contains": "x64" }
//!
//! Put your owner and repository in the first three addresses (https only) and restart. The installer to start is always the file this module saved from
//! those downloads, never a path or address from anywhere else.
#![allow(dead_code)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::util::write_atomic;

/// The installer is a few MB; anything this big is refused whatever GitHub says
const MAX_INSTALLER_BYTES: u64 = 300 * 1024 * 1024;
const MAX_RELEASE_BYTES: u64 = 2 * 1024 * 1024;

/// Bumped for every download and by `cancel`; a download stops when it is no longer the latest
static DOWNLOAD: AtomicU64 = AtomicU64::new(0);
/// The checked installer, waiting for `install`
static READY: Mutex<Option<PathBuf>> = Mutex::new(None);

const OWNER: &str = "minnnion";
const REPO: &str = "MinnionDeck";

/// Where the releases are.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub latest_url: String,
    pub download_prefix: String,
    pub page_prefix: String,
    pub installer_suffix: String,
    pub installer_contains: String,
}

impl Source {
    /// MinnionDeck's own releases on GitHub.
    pub fn default_source() -> Source {
        Source {
            latest_url: format!("https://api.github.com/repos/{OWNER}/{REPO}/releases/latest"),
            download_prefix: format!("https://github.com/{OWNER}/{REPO}/releases/download/"),
            page_prefix: format!("https://github.com/{OWNER}/{REPO}/"),
            installer_suffix: "-setup.exe".into(),
            installer_contains: "x64".into(),
        }
    }

    /// Whether a repository is set in update.json.
    pub fn configured(&self) -> bool {
        !self.latest_url.is_empty()
    }

    /// Reads <root>\update.json. A missing one is made with the defaults, so it can be edited; an
    /// unusable one (not https) falls back to the defaults.
    pub fn load(root: &Path) -> Source {
        let file = root.join("update.json");
        let def = Source::default_source();
        match fs::read_to_string(&file) {
            // an update.json from before the addresses were built in (all empty) means the default
            Ok(text) => Source::parse(&text).filter(|s| s.configured()).unwrap_or(def),
            Err(_) => {
                let body = json!({
                    "latest_url": def.latest_url, "download_prefix": def.download_prefix, "page_prefix": def.page_prefix,
                    "installer_suffix": def.installer_suffix, "installer_contains": def.installer_contains,
                });
                let _ = write_atomic(&file, serde_json::to_string_pretty(&body).unwrap_or_default().as_bytes());
                def
            }
        }
    }

    fn parse(text: &str) -> Option<Source> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.trim().to_string());
        // empty means not set up; anything else must be an https address
        let https = |u: &str| u.is_empty() || (u.len() <= 500 && u.starts_with("https://") && !u.chars().any(|c| c.is_whitespace() || c.is_control()));
        let (latest_url, download_prefix, page_prefix) = (get("latest_url")?, get("download_prefix")?, get("page_prefix")?);
        if !(https(&latest_url) && https(&download_prefix) && https(&page_prefix)) || (latest_url.is_empty() != download_prefix.is_empty()) {
            return None;
        }
        Some(Source {
            latest_url,
            download_prefix,
            page_prefix,
            installer_suffix: get("installer_suffix").filter(|s| !s.is_empty()).unwrap_or_else(|| "-setup.exe".into()),
            installer_contains: get("installer_contains").unwrap_or_else(|| "x64".into()),
        })
    }
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize, Clone)]
struct Asset {
    name: String,
    size: u64,
    browser_download_url: String,
    /// "sha256:<hex>", which GitHub works out for every release file
    #[serde(default)]
    digest: Option<String>,
}

/// What Settings › Updates shows
#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub newer: bool,
    pub name: String,
    pub notes: String,
    pub page: String,
    pub size: u64,
}

/// The version of this app.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .user_agent(&format!("Minniondeck/{} (app update)", env!("CARGO_PKG_VERSION")))
        .build()
}

/// "v0.2.0" and "0.2.0" are the same version; anything after a - is a pre-release
fn parts(v: &str) -> (Vec<u64>, bool) {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let (main, pre) = v.split_once('-').map_or((v, false), |(m, _)| (m, true));
    (main.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
}

/// Is `latest` newer than `current`?
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, pa) = parts(latest);
    let (mut b, pb) = parts(current);
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b || (a == b && pb && !pa)
}

fn fetch_latest(src: &Source) -> Result<Release, String> {
    let resp = agent().get(&src.latest_url).set("Accept", "application/vnd.github+json").call().map_err(|e| match e {
        ureq::Error::Status(404, _) => "There are no releases on GitHub yet.".to_string(),
        ureq::Error::Status(code, _) => format!("GitHub didn't answer the update check (it answered {code}). Try again later."),
        _ => "Couldn't reach GitHub. Check your internet connection and try again.".to_string(),
    })?;
    let mut text = String::new();
    resp.into_reader().take(MAX_RELEASE_BYTES).read_to_string(&mut text).map_err(|e| e.to_string())?;
    let r: Release = serde_json::from_str(&text).map_err(|_| "GitHub's release information couldn't be read.".to_string())?;
    if r.draft || r.prerelease {
        return Err("The latest release on GitHub isn't a finished one.".into());
    }
    Ok(r)
}

/// The Windows installer of a release: the setup .exe, from this repository's downloads
fn installer<'a>(r: &'a Release, src: &Source) -> Option<&'a Asset> {
    r.assets.iter().find(|a| {
        let n = a.name.to_ascii_lowercase();
        n.ends_with(&src.installer_suffix.to_ascii_lowercase()) && n.contains(&src.installer_contains.to_ascii_lowercase()) && a.browser_download_url.starts_with(&src.download_prefix) && super::plugins::safe_name(&a.name)
    })
}

/// The SHA-256 GitHub lists for a release file, if it lists one
fn digest(a: &Asset) -> Option<String> {
    let hex = a.digest.as_deref()?.strip_prefix("sha256:")?.to_ascii_lowercase();
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
}

/// Ask GitHub for the latest release and compare it with this copy. Blocks; call it from a thread.
pub fn check(root: &Path) -> Result<UpdateInfo, String> {
    let src = Source::load(root);
    if !src.configured() {
        return Err("Updates aren't set up yet. Put your GitHub repository in update.json.".into());
    }
    let r = fetch_latest(&src)?;
    let current = current_version().to_string();
    let latest = r.tag_name.trim_start_matches(['v', 'V']).to_string();
    let size = installer(&r, &src).map_or(0, |a| a.size);
    Ok(UpdateInfo {
        newer: is_newer(&latest, &current),
        name: if r.name.is_empty() { format!("Minniondeck {latest}") } else { r.name.chars().take(80).collect() },
        notes: r.body.chars().take(4000).collect(),
        page: if r.html_url.starts_with(&src.page_prefix) { r.html_url.clone() } else { String::new() },
        current,
        latest,
        size,
    })
}

fn update_dir() -> PathBuf {
    std::env::temp_dir().join("Minniondeck-update")
}

/// Download the latest release's installer and check it. `progress(got, total)` is called as it
/// arrives. Returns the installer's file name. Blocks; call it from a thread.
pub fn download(root: &Path, progress: impl Fn(u64, u64)) -> Result<String, String> {
    let src = Source::load(root);
    if !src.configured() {
        return Err("Updates aren't set up yet. Put your GitHub repository in update.json.".into());
    }
    let mine = DOWNLOAD.fetch_add(1, Ordering::SeqCst) + 1;
    let r = fetch_latest(&src)?;
    if !is_newer(&r.tag_name, current_version()) {
        return Err("You already have the latest version.".into());
    }
    let a = installer(&r, &src).ok_or("This release has no Windows installer on GitHub yet.")?.clone();
    let sha = digest(&a).ok_or("GitHub doesn't list a checksum for this installer, so it wasn't downloaded.")?;
    if a.size == 0 || a.size > MAX_INSTALLER_BYTES {
        return Err("GitHub's information about this installer doesn't look right, so it wasn't downloaded.".into());
    }
    // The installer must be signed with MinnionDeck's own key, not only listed by GitHub: a
    // release someone else managed to upload is refused before anything runs.
    let sig = fetch_signature(&r, &src, &a.name)?;
    let dir = update_dir();
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make a folder for the download: {e}"))?;
    let path = dir.join(&a.name);
    let resp = agent().get(&a.browser_download_url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("GitHub didn't send the installer (it answered {code})."),
        _ => "Couldn't reach GitHub. Check your internet connection and try again.".to_string(),
    })?;
    let result = (|| {
        let mut from = resp.into_reader();
        let mut out = fs::File::create(&path).map_err(|e| format!("Couldn't save the download: {e}"))?;
        let mut hasher = ring::digest::Context::new(&ring::digest::SHA256);
        let mut buf = vec![0u8; 64 * 1024];
        let mut got = 0u64;
        loop {
            if DOWNLOAD.load(Ordering::SeqCst) != mine {
                return Err("Download cancelled.".to_string());
            }
            let n = from.read(&mut buf).map_err(|_| "The download stopped. Check your internet connection and try again.".to_string())?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > a.size {
                return Err("The download is bigger than GitHub says, so it wasn't kept.".into());
            }
            hasher.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| format!("Couldn't save the download: {e}"))?;
            progress(got, a.size);
        }
        if got != a.size {
            return Err("The download ended early. Try again.".into());
        }
        let hex: String = hasher.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect();
        if hex != sha {
            return Err("The download doesn't match GitHub's checksum, so it wasn't kept.".into());
        }
        super::signing::verify_release(&sig, &a.name, &hex)?;
        out.flush().map_err(|e| e.to_string())
    })();
    if let Err(e) = result {
        let _ = fs::remove_dir_all(&dir);
        return Err(e);
    }
    *READY.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
    Ok(a.name)
}

/// The signature uploaded beside the installer (`<installer>.sig`), from this repository's downloads
fn fetch_signature(r: &Release, src: &Source, installer: &str) -> Result<Vec<u8>, String> {
    const MAX_SIG_BYTES: u64 = 64 * 1024;
    let want = super::signing::release_sig_name(installer);
    let a = r
        .assets
        .iter()
        .find(|a| a.name == want && a.browser_download_url.starts_with(&src.download_prefix))
        .ok_or("This release's installer isn't signed by MinnionDeck, so it wasn't downloaded.")?;
    if a.size == 0 || a.size > MAX_SIG_BYTES {
        return Err("This release's signature doesn't look right, so it wasn't downloaded.".into());
    }
    let resp = agent().get(&a.browser_download_url).call().map_err(|_| "Couldn't download the update's signature. Try again.".to_string())?;
    let mut sig = Vec::new();
    resp.into_reader().take(MAX_SIG_BYTES).read_to_end(&mut sig).map_err(|_| "Couldn't download the update's signature. Try again.".to_string())?;
    Ok(sig)
}

/// Stop the download that is running, if any
pub fn cancel() {
    DOWNLOAD.fetch_add(1, Ordering::SeqCst);
}

/// Start the checked installer. The caller then closes the app so it can replace it.
pub fn install() -> Result<(), String> {
    let path = READY.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or("Download the update first.")?;
    if !path.is_file() {
        return Err("The downloaded installer is gone. Download it again.".into());
    }
    super::actions::open_path(&path)?;
    super::plugins::log(&format!("Starting the update installer {}", path.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.2.0", "0.2.0-beta"));
        assert!(!is_newer("0.2.0-beta", "0.2.0"));
    }

    #[test]
    fn only_this_repositorys_installer_is_used() {
        let src = Source {
            latest_url: "https://api.github.com/repos/me/app/releases/latest".into(),
            download_prefix: "https://github.com/me/app/releases/download/".into(),
            page_prefix: "https://github.com/me/app/".into(),
            ..Source::default_source()
        };
        let asset = |name: &str, url: &str| Asset { name: name.into(), size: 1, browser_download_url: url.into(), digest: None };
        let rel = |assets| Release { tag_name: "v9.0.0".into(), name: String::new(), body: String::new(), html_url: String::new(), draft: false, prerelease: false, assets };
        let good = "https://github.com/me/app/releases/download/v9.0.0/Minniondeck_9.0.0_x64-setup.exe";
        assert!(installer(&rel(vec![asset("Minniondeck_9.0.0_x64-setup.exe", good)]), &src).is_some());
        assert!(installer(&rel(vec![asset("Minniondeck_9.0.0_x64-setup.exe", "https://example.com/x64-setup.exe")]), &src).is_none());
        assert!(installer(&rel(vec![asset("Minniondeck_9.0.0_x64_en-US.msi", good)]), &src).is_none());
    }

    #[test]
    fn checksums_must_be_full_sha256() {
        let a = |d: Option<&str>| Asset { name: "x".into(), size: 1, browser_download_url: String::new(), digest: d.map(Into::into) };
        assert!(digest(&a(Some("sha256:9ef11639be6570a2bd28a3d1b728f4bf404b8594299f455c2a457fbc222d39ed"))).is_some());
        assert!(digest(&a(Some("sha256:abc"))).is_none());
        assert!(digest(&a(Some("md5:9ef11639be6570a2bd28a3d1b728f4bf"))).is_none());
        assert!(digest(&a(None)).is_none());
    }

    #[test]
    fn the_release_address_comes_from_update_json() {
        let dir = std::env::temp_dir().join(format!("md-update-src-{}", super::super::util::now_ms()));
        fs::create_dir_all(&dir).unwrap();
        let d = Source::load(&dir);
        assert_eq!(d, Source::default_source());
        assert!(d.configured() && d.latest_url.contains("minnnion/MinnionDeck"), "MinnionDeck's own releases by default");
        // an update.json from an older build, with the addresses empty, means the default too
        fs::write(dir.join("update.json"), r#"{"latest_url":"","download_prefix":"","page_prefix":""}"#).unwrap();
        assert_eq!(Source::load(&dir), d);
        assert!(dir.join("update.json").is_file(), "a file is made to edit");
        assert_eq!(Source::load(&dir), d);
        // a new repository, and an installer named another way
        fs::write(dir.join("update.json"), r#"{"latest_url":"https://api.github.com/repos/me/new/releases/latest","download_prefix":"https://github.com/me/new/releases/download/","page_prefix":"https://github.com/me/new/","installer_suffix":".exe","installer_contains":""}"#).unwrap();
        let s = Source::load(&dir);
        assert!(s.configured());
        assert_eq!(s.latest_url, "https://api.github.com/repos/me/new/releases/latest");
        assert_eq!((s.installer_suffix.as_str(), s.installer_contains.as_str()), (".exe", ""));
        // only https
        fs::write(dir.join("update.json"), r#"{"latest_url":"http://x/latest","download_prefix":"https://x/","page_prefix":"https://x/"}"#).unwrap();
        assert_eq!(Source::load(&dir), Source::default_source());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Talks to GitHub: cargo test --release real_release -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_release_is_found() {
        // the original Minniondeck app's repository, only to check the code reads a real release
        let src = Source {
            latest_url: "https://api.github.com/repos/minnnion/MinnionDeck/releases/latest".into(),
            download_prefix: "https://github.com/minnnion/MinnionDeck/releases/download/".into(),
            page_prefix: "https://github.com/minnnion/MinnionDeck/".into(),
            ..Source::default_source()
        };
        match fetch_latest(&src) {
            Ok(r) => {
                println!("latest release {} with {} files; installer: {}", r.tag_name, r.assets.len(), installer(&r, &src).map(|a| a.name.clone()).unwrap_or_else(|| "none".into()));
            }
            Err(e) => println!("no release to read: {e}"),
        }
    }
}
