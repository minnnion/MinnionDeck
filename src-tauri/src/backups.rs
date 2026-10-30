// Backups of the library: encrypted .mdbackup packages in the backups folder.
// One is made when the app starts (if anything changed since the last one), before every
// import and before every restore. Only the newest ones are kept, 5 at most.

use std::fs;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::package::{self, Kind, PackageWriter};
use crate::storage::{Store, now_ms, safe_name, setting_value, sha256_hex, write_atomic};

pub const MAX_BACKUPS: usize = 5;
const EXT: &str = "mdbackup";
const LAST_HASH: &str = "last-backup.sha256";

/// "Backups to keep" from the Settings page, never more than 5
fn keep(store: &Store) -> usize {
    setting_value(store, "keep").and_then(|v| v.as_str().and_then(|s| s.parse().ok())).unwrap_or(MAX_BACKUPS).clamp(1, MAX_BACKUPS)
}

/// "Back up profiles every day" from the Settings page (on unless turned off)
pub fn auto_backup_on(store: &Store) -> bool {
    setting_value(store, "autoBackup").and_then(|v| v.as_bool()).unwrap_or(true)
}

/// 2026-10-18-093012 in UTC, from unix milliseconds
pub(crate) fn stamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    // Days since 1970-01-01 to a calendar date (Howard Hinnant's civil_from_days)
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}-{h:02}{m:02}{s:02}")
}

/// Hash of what the library holds, leaving out when it was saved: the app rewrites the
/// file on every start, and that alone must not count as a change worth a new backup
fn content_hash(library: &[u8]) -> String {
    match serde_json::from_slice::<Value>(library) {
        Ok(v) => sha256_hex(v["data"].to_string().as_bytes()),
        Err(_) => sha256_hex(library),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Startup,
    BeforeImport,
    BeforeRestore,
    ByHand,
}

/// Make a backup of the library and media. Returns its file name, or None when there
/// was nothing to back up (no library yet, or nothing changed since the last start-up backup).
pub fn create(store: &Store, reason: Reason) -> Result<Option<String>, String> {
    let _guard = store.guard();
    let lib_path = store.paths.library_file();
    let Ok(library) = fs::read(&lib_path) else { return Ok(None) };
    let hash = content_hash(&library);
    let hash_path = store.paths.backups.join(LAST_HASH);
    if reason == Reason::Startup && fs::read_to_string(&hash_path).ok().as_deref() == Some(hash.as_str()) {
        return Ok(None);
    }
    let mut w = PackageWriter::new();
    w.add("library.json", &library)?;
    let mut media = 0;
    if let Ok(entries) = fs::read_dir(&store.paths.media) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_file() && safe_name(&name).is_some() && !name.ends_with(".tmp") {
                let bytes = fs::read(entry.path()).map_err(|e| format!("Could not read {name}: {e}"))?;
                w.add(&format!("media/{name}"), &bytes)?;
                media += 1;
            }
        }
    }
    let why = match reason {
        Reason::Startup => "start-up",
        Reason::BeforeImport => "before import",
        Reason::BeforeRestore => "before restore",
        Reason::ByHand => "by hand",
    };
    let now = now_ms();
    let zip = w.finish(Kind::Backup, &format!("Backup {why}"), json!({ "media": media, "reason": why }), Vec::new())?;
    let sealed = package::seal(Kind::Backup, &zip)?;
    let name = format!("backup-{}-{:03}.{EXT}", stamp(now), now % 1000);
    write_atomic(&store.paths.backups.join(&name), &sealed)?;
    let _ = fs::write(&hash_path, &hash);
    rotate(store);
    crate::applog::line(&format!("Backup made ({why}): {name}"));
    Ok(Some(name))
}

/// Delete the oldest backups so only the newest ones stay
fn rotate(store: &Store) {
    let mut names = backup_names(store);
    let keep = keep(store);
    while names.len() > keep {
        let oldest = names.remove(0);
        let _ = fs::remove_file(store.paths.backups.join(oldest));
    }
}

/// Backup file names, oldest first (the names start with the date, so they sort by time)
fn backup_names(store: &Store) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&store.paths.backups)
        .map(|it| {
            it.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("backup-") && n.ends_with(&format!(".{EXT}")))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub name: String,
    pub created_ms: u64,
    pub bytes: u64,
}

#[tauri::command]
pub async fn list_backups(app: AppHandle) -> Result<Vec<BackupInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        let mut out: Vec<BackupInfo> = backup_names(&store)
            .into_iter()
            .filter_map(|name| {
                let meta = fs::metadata(store.paths.backups.join(&name)).ok()?;
                let created_ms = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
                Some(BackupInfo { name, created_ms, bytes: meta.len() })
            })
            .collect();
        out.reverse();
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn backup_now(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || create(&app.state::<Store>(), Reason::ByHand))
        .await
        .map_err(|e| e.to_string())?
}

/// Put a backup back. The current library is backed up first, so a restore can be undone.
/// The app restarts afterwards so every page starts from the restored data.
#[tauri::command]
pub async fn restore_backup(app: AppHandle, name: String) -> Result<(), String> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<Store>();
        if safe_name(&name).is_none() || !name.ends_with(&format!(".{EXT}")) {
            return Err("That backup name is not valid.".to_string());
        }
        // Read it before making the safety backup: rotating could delete the oldest one
        let bytes = fs::read(store.paths.backups.join(&name)).map_err(|_| "That backup no longer exists.".to_string())?;
        let pkg = package::read(&package::open(&bytes, Kind::Backup)?, Kind::Backup)?;
        let library = pkg.get("library.json").ok_or("The backup has no library in it.")?.to_vec();
        create(&store, Reason::BeforeRestore)?;

        let _guard = store.guard();
        store.frozen.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::applog::line(&format!("Restoring backup {name}"));
        if let Ok(entries) = fs::read_dir(&store.paths.media) {
            for entry in entries.flatten() {
                if entry.path().is_file() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        for path in pkg.paths() {
            if let Some(file) = path.strip_prefix("media/").and_then(safe_name) {
                write_atomic(&store.paths.media.join(file), pkg.get(path).unwrap_or_default())?;
            }
        }
        write_atomic(&store.paths.library_file(), &library)?;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    handle.restart();
}

#[cfg(test)]
mod tests {
    use super::{content_hash, stamp};

    #[test]
    fn saving_again_without_changes_is_not_a_change() {
        let a = br#"{"schema_version":1,"saved_at":100,"data":{"profiles":[1]}}"#;
        let b = br#"{"schema_version":1,"saved_at":999,"data":{"profiles":[1]}}"#;
        let c = br#"{"schema_version":1,"saved_at":999,"data":{"profiles":[2]}}"#;
        assert_eq!(content_hash(a), content_hash(b));
        assert_ne!(content_hash(a), content_hash(c));
    }

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(stamp(0), "1970-01-01-000000");
        // 2026-10-18 09:30:12 UTC
        assert_eq!(stamp(1_792_315_812_000), "2026-10-18-093012");
    }
}
