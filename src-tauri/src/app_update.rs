// Updates for Minniondeck itself, from the public repository's GitHub Releases.
//
// Settings › Updates (and a daily check at start-up) asks GitHub for the latest release. If it is
// newer than this copy, the user can download its Windows installer: Minniondeck saves it in the
// temp folder, checks its size and the SHA-256 GitHub lists for it, then starts it and closes so
// the installer can replace the app. A download without GitHub's checksum is refused.
//
// Where releases come from is fixed here, and the installer to start is the one this file saved,
// never a path or address from the UI.

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

/// The public repository that holds the releases
const REPO: &str = "MinnionDeck";
/// Only installers from this repository's release downloads are fetched
fn download_prefix() -> String {
    format!("https://github.com/{}/{REPO}/releases/download/", crate::market::GITHUB_OWNER)
}
fn latest_url() -> String {
    format!("https://api.github.com/repos/{}/{REPO}/releases/latest", crate::market::GITHUB_OWNER)
}
/// The installer is a few MB; anything this big is refused whatever GitHub says
const MAX_INSTALLER_BYTES: u64 = 300 * 1024 * 1024;
const MAX_RELEASE_BYTES: u64 = 2 * 1024 * 1024;

/// Bumped for every download and by Cancel; a download stops when it is no longer the latest
static DOWNLOAD: AtomicU64 = AtomicU64::new(0);
/// The checked installer, waiting for Install now
static READY: Mutex<Option<PathBuf>> = Mutex::new(None);

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
#[derive(Serialize)]
pub struct UpdateInfo {
    current: String,
    latest: String,
    newer: bool,
    name: String,
    notes: String,
    page: String,
    size: u64,
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
fn is_newer(latest: &str, current: &str) -> bool {
    let (mut a, pa) = parts(latest);
    let (mut b, pb) = parts(current);
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b || (a == b && pb && !pa)
}

fn fetch_latest() -> Result<Release, String> {
    let resp = agent().get(&latest_url()).set("Accept", "application/vnd.github+json").call().map_err(|e| match e {
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

/// The Windows installer of a release: the NSIS setup .exe, from this repository's downloads
fn installer(r: &Release) -> Option<&Asset> {
    r.assets.iter().find(|a| {
        let n = a.name.to_ascii_lowercase();
        n.ends_with("-setup.exe") && n.contains("x64") && a.browser_download_url.starts_with(&download_prefix()) && crate::plugins::safe_name(&a.name)
    })
}

/// The SHA-256 GitHub lists for a release file, if it lists one
fn digest(a: &Asset) -> Option<String> {
    let hex = a.digest.as_deref()?.strip_prefix("sha256:")?.to_ascii_lowercase();
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
}

/// Ask GitHub for the latest release and compare it with this copy
#[tauri::command]
pub async fn app_update_check() -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let r = fetch_latest()?;
        let current = env!("CARGO_PKG_VERSION").to_string();
        let latest = r.tag_name.trim_start_matches(['v', 'V']).to_string();
        let size = installer(&r).map_or(0, |a| a.size);
        Ok(UpdateInfo {
            newer: is_newer(&latest, &current),
            name: if r.name.is_empty() { format!("Minniondeck {latest}") } else { r.name.chars().take(80).collect() },
            notes: r.body.chars().take(4000).collect(),
            page: r.html_url.starts_with(&format!("https://github.com/{}/{REPO}/", crate::market::GITHUB_OWNER)).then(|| r.html_url.clone()).unwrap_or_default(),
            current,
            latest,
            size,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

fn update_dir() -> PathBuf {
    std::env::temp_dir().join("Minniondeck-update")
}

/// Download the latest release's installer and check it. Progress goes out as app_update_progress.
#[tauri::command]
pub async fn app_update_download(app: AppHandle) -> Result<String, String> {
    let mine = DOWNLOAD.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let r = fetch_latest()?;
        if !is_newer(&r.tag_name, env!("CARGO_PKG_VERSION")) {
            return Err("You already have the latest version.".into());
        }
        let a = installer(&r).ok_or("This release has no Windows installer on GitHub yet.")?.clone();
        let sha = digest(&a).ok_or("GitHub doesn't list a checksum for this installer, so it wasn't downloaded.")?;
        if a.size == 0 || a.size > MAX_INSTALLER_BYTES {
            return Err("GitHub's information about this installer doesn't look right, so it wasn't downloaded.".into());
        }
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
            let mut hasher = Sha256::new();
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
                let _ = app.emit("app_update_progress", json!({ "got": got, "total": a.size }));
            }
            if got != a.size {
                return Err("The download ended early. Try again.".into());
            }
            if format!("{:x}", hasher.finalize()) != sha {
                return Err("The download doesn't match GitHub's checksum, so it wasn't kept.".into());
            }
            out.flush().map_err(|e| e.to_string())
        })();
        if let Err(e) = result {
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
        *READY.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
        Ok(a.name)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Stop the download that is running, if any
#[tauri::command]
pub fn app_update_cancel() {
    DOWNLOAD.fetch_add(1, Ordering::SeqCst);
}

/// Start the checked installer and close Minniondeck so it can replace the app
#[tauri::command]
pub fn app_update_install(app: AppHandle) -> Result<(), String> {
    let path = READY.lock().unwrap_or_else(|e| e.into_inner()).take().ok_or("Download the update first.")?;
    if !path.is_file() {
        return Err("The downloaded installer is gone. Download it again.".into());
    }
    crate::actions::start_file(&path)?;
    crate::applog::line(&format!("Starting the update installer {}", path.display()));
    app.exit(0);
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
        let asset = |name: &str, url: &str| Asset { name: name.into(), size: 1, browser_download_url: url.into(), digest: None };
        let rel = |assets| Release { tag_name: "v9.0.0".into(), name: String::new(), body: String::new(), html_url: String::new(), draft: false, prerelease: false, assets };
        let good = "https://github.com/minnnion/MinnionDeck/releases/download/v9.0.0/Minniondeck_9.0.0_x64-setup.exe";
        assert!(installer(&rel(vec![asset("Minniondeck_9.0.0_x64-setup.exe", good)])).is_some());
        assert!(installer(&rel(vec![asset("Minniondeck_9.0.0_x64-setup.exe", "https://example.com/x64-setup.exe")])).is_none());
        assert!(installer(&rel(vec![asset("Minniondeck_9.0.0_x64_en-US.msi", good)])).is_none());
    }

    #[test]
    fn checksums_must_be_full_sha256() {
        let a = |d: Option<&str>| Asset { name: "x".into(), size: 1, browser_download_url: String::new(), digest: d.map(Into::into) };
        assert!(digest(&a(Some("sha256:9ef11639be6570a2bd28a3d1b728f4bf404b8594299f455c2a457fbc222d39ed"))).is_some());
        assert!(digest(&a(Some("sha256:abc"))).is_none());
        assert!(digest(&a(Some("md5:9ef11639be6570a2bd28a3d1b728f4bf"))).is_none());
        assert!(digest(&a(None)).is_none());
    }

    /// Talks to GitHub: cargo test --lib app_update -- --ignored
    #[test]
    #[ignore]
    fn the_live_release_has_a_checked_installer() {
        let r = fetch_latest().expect("latest release");
        let a = installer(&r).expect("setup .exe");
        assert!(digest(a).is_some(), "GitHub lists a SHA-256");
        assert!(a.size > 0);
    }
}
