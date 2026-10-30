// Street map tiles for live map buttons (the MSFS 2024 plugin's map with its "Street map"
// background). The UI asks for one tile at a time; it comes from the disk cache when it's there
// and fresh, else from the tile server, fetched as Minniondeck (OpenStreetMap's tile policy asks
// apps to say who they are, and to keep tiles they already have).
//
// The tile address is the plugin's setting, e.g. https://tile.openstreetmap.org/{z}/{x}/{y}.png.
// Only https addresses with {z}, {x} and {y} in them are used.

use base64::Engine;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Manager};

/// Tiles younger than this are used from the cache without asking the server
const KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
const MAX_TILE_BYTES: u64 = 2 * 1024 * 1024;

fn cache_dir(app: &AppHandle, template: &str) -> Result<PathBuf, String> {
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?;
    // One folder per tile address, so switching servers never mixes their tiles
    let key = &crate::storage::sha256_hex(template.as_bytes())[..12];
    Ok(root.join("tiles").join(key))
}

fn data_url(bytes: &[u8]) -> String {
    let mime = if bytes.starts_with(&[0xFF, 0xD8]) { "image/jpeg" } else if bytes.starts_with(b"RIFF") { "image/webp" } else { "image/png" };
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let agent = format!("Minniondeck/{} (desktop deck app; live map button)", env!("CARGO_PKG_VERSION"));
    let resp = ureq::get(url)
        .set("User-Agent", &agent)
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| format!("The map tile couldn't be loaded: {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader().take(MAX_TILE_BYTES).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("The tile server sent an empty tile.".into());
    }
    Ok(bytes)
}

/// One map tile as a data: URL, from the cache or the tile server
#[tauri::command]
pub async fn map_tile(app: AppHandle, template: String, z: u32, x: u32, y: u32) -> Result<String, String> {
    let template = template.trim().to_string();
    if !template.starts_with("https://") || !["{z}", "{x}", "{y}"].iter().all(|k| template.contains(k)) {
        return Err("The street map tile address must start with https:// and contain {z}, {x} and {y}.".into());
    }
    if z > 19 || x >= (1 << z) || y >= (1 << z) {
        return Err("That map tile doesn't exist.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let file = cache_dir(&app, &template)?.join(z.to_string()).join(x.to_string()).join(format!("{y}.tile"));
        let fresh = fs::metadata(&file).ok().and_then(|m| m.modified().ok()).is_some_and(|t| SystemTime::now().duration_since(t).unwrap_or_default() < KEEP);
        if fresh {
            if let Ok(bytes) = fs::read(&file) {
                return Ok(data_url(&bytes));
            }
        }
        let url = template.replace("{z}", &z.to_string()).replace("{x}", &x.to_string()).replace("{y}", &y.to_string());
        match fetch(&url) {
            Ok(bytes) => {
                if let Some(dir) = file.parent() {
                    let _ = fs::create_dir_all(dir);
                }
                let _ = crate::storage::write_atomic(&file, &bytes);
                Ok(data_url(&bytes))
            }
            // Offline or refused: an old copy is better than none
            Err(e) => fs::read(&file).map(|b| data_url(&b)).map_err(|_| e),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
