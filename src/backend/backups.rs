//! Backups of the library: encrypted .mdbackup packages in the backups folder, the same files the
//! Minniondeck app makes. One is made when the app starts (if anything changed since the last
//! one), before every import and before every restore. Only the newest 5 are kept.
use std::fs;

use serde_json::{Value, json};

use super::package::{self, Kind, PackageWriter};
use super::paths::Paths;
use super::util::{now_ms, safe_name, sha256_hex, write_atomic};
use super::{media, vault};

pub const MAX_BACKUPS: usize = 5;
const EXT: &str = "mdbackup";
const LAST_HASH: &str = "last-backup.sha256";

/// 2026-10-18-093012 in UTC, from unix milliseconds
pub fn stamp(ms: u64) -> String {
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

/// "5 Nov 14:22" in this PC's time, from unix milliseconds
pub fn local_text(ms: u64) -> String {
    let local = ms as i64 - local_offset_minutes() * 60_000;
    let s = stamp(local.max(0) as u64);
    let month: usize = s[5..7].parse().unwrap_or(1);
    let day: u32 = s[8..10].parse().unwrap_or(1);
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{day} {} {}:{}", M[month.clamp(1, 12) - 1], &s[11..13], &s[13..15])
}

/// Minutes this PC's clock is behind UTC (negative east of it), counting summer time.
#[cfg(windows)]
fn local_offset_minutes() -> i64 {
    use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    let mut tz = TIME_ZONE_INFORMATION::default();
    let id = unsafe { GetTimeZoneInformation(&mut tz) };
    let extra = match id {
        2 => tz.DaylightBias, // TIME_ZONE_ID_DAYLIGHT
        _ => tz.StandardBias,
    };
    (tz.Bias + extra) as i64
}

#[cfg(not(windows))]
fn local_offset_minutes() -> i64 {
    0
}

/// Hash of what the library holds, leaving out when it was saved: the app rewrites the
/// file on every start, and that alone must not count as a change worth a new backup
fn content_hash(library: &[u8]) -> String {
    match serde_json::from_slice::<Value>(library) {
        Ok(v) => sha256_hex(v["data"].to_string().as_bytes()),
        Err(_) => sha256_hex(library),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    Startup,
    BeforeImport,
    BeforeRestore,
    ByHand,
}

/// Make a backup of the library and media. Returns its file name, or None when there
/// was nothing to back up (no library yet, or nothing changed since the last start-up backup).
pub fn create(paths: &Paths, reason: Reason) -> Result<Option<String>, String> {
    let Ok(library) = vault::read(&paths.library_file(), Kind::Save) else { return Ok(None) };
    fs::create_dir_all(&paths.backups).map_err(|e| e.to_string())?;
    let hash = content_hash(&library);
    let hash_path = paths.backups.join(LAST_HASH);
    if reason == Reason::Startup && fs::read_to_string(&hash_path).ok().as_deref() == Some(hash.as_str()) {
        return Ok(None);
    }
    let mut w = PackageWriter::new();
    w.add("library.json", &library)?;
    let mut media = 0;
    // each picture under its own name (a sealed one is opened: the backup is sealed as a whole)
    for (name, bytes) in media::all_files(&paths.media)? {
        w.add(&format!("media/{name}"), &bytes)?;
        media += 1;
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
    write_atomic(&paths.backups.join(&name), &sealed)?;
    let _ = fs::write(&hash_path, &hash);
    rotate(paths);
    Ok(Some(name))
}

/// Delete the oldest backups so only the newest ones stay
fn rotate(paths: &Paths) {
    let mut names = names(paths);
    while names.len() > MAX_BACKUPS {
        let oldest = names.remove(0);
        let _ = fs::remove_file(paths.backups.join(oldest));
    }
}

/// Backup file names, oldest first (the names start with the date, so they sort by time)
fn names(paths: &Paths) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(&paths.backups)
        .map(|it| it.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("backup-") && n.ends_with(&format!(".{EXT}"))).collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[derive(Clone, Debug)]
pub struct BackupInfo {
    pub name: String,
    pub created_ms: u64,
    pub bytes: u64,
}

/// The backups there are, newest first.
pub fn list(paths: &Paths) -> Vec<BackupInfo> {
    let mut out: Vec<BackupInfo> = names(paths)
        .into_iter()
        .filter_map(|name| {
            let meta = fs::metadata(paths.backups.join(&name)).ok()?;
            let created_ms = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
            Some(BackupInfo { name, created_ms, bytes: meta.len() })
        })
        .collect();
    out.reverse();
    out
}

/// Put a backup back. The current library is backed up first, so a restore can be undone.
/// The caller then reads the library again.
pub fn restore(paths: &Paths, name: &str) -> Result<(), String> {
    if safe_name(name).is_none() || !name.ends_with(&format!(".{EXT}")) {
        return Err("That backup name is not valid.".to_string());
    }
    // Read it before making the safety backup: rotating could delete the oldest one
    let bytes = fs::read(paths.backups.join(name)).map_err(|_| "That backup no longer exists.".to_string())?;
    let pkg = package::read(&package::open(&bytes, Kind::Backup)?, Kind::Backup)?;
    let library = pkg.get("library.json").ok_or("The backup has no library in it.")?.to_vec();
    if serde_json::from_slice::<Value>(&library).is_err() {
        return Err("The library in that backup is damaged.".into());
    }
    create(paths, Reason::BeforeRestore)?;
    if let Ok(entries) = fs::read_dir(&paths.media) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    for path in pkg.paths() {
        if let Some(file) = path.strip_prefix("media/").and_then(safe_name) {
            media::store(&paths.media, file, pkg.get(path).unwrap_or_default())?;
        }
    }
    vault::write(&paths.library_file(), Kind::Save, &library)
}

/// A readable size: "1.2 MB".
pub fn size_text(b: u64) -> String {
    if b >= 1_000_000 { format!("{:.1} MB", b as f64 / 1_000_000.0) } else { format!("{} KB", (b / 1000).max(1)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("md-backups-{tag}-{}", now_ms()));
        let p = Paths { root: dir.clone(), library: dir.join("library"), media: dir.join("library").join("media"), backups: dir.join("backups") };
        p.make_folders().unwrap();
        p
    }

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
        assert_eq!(stamp(1_792_315_812_000), "2026-10-18-093012");
        assert!(local_text(1_792_315_812_000).contains("Oct"));
    }

    #[test]
    fn a_backup_is_made_listed_rotated_and_restored() {
        let p = temp_paths("cycle");
        // nothing to back up yet
        assert_eq!(create(&p, Reason::ByHand).unwrap(), None);
        vault::write(&p.library_file(), Kind::Save, br#"{"schema_version":1,"saved_at":1,"data":{"profiles":["one"]}}"#).unwrap();
        media::store(&p.media, "pic.png", &[1u8, 2, 3]).unwrap();
        let first = create(&p, Reason::Startup).unwrap().expect("a backup");
        // at start-up an unchanged library isn't backed up again; by hand it is
        assert_eq!(create(&p, Reason::Startup).unwrap(), None);
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(create(&p, Reason::ByHand).unwrap().is_some());
        assert_eq!(list(&p).len(), 2);
        assert!(list(&p)[0].name > list(&p)[1].name, "newest first");
        // the library changes, then the first backup is put back
        vault::write(&p.library_file(), Kind::Save, br#"{"schema_version":1,"saved_at":2,"data":{"profiles":["two"]}}"#).unwrap();
        media::store(&p.media, "other.png", &[9u8]).unwrap();
        fs::remove_file(media::file_for(&p.media, "pic.png").unwrap()).unwrap();
        restore(&p, &first).unwrap();
        assert!(String::from_utf8(vault::read(&p.library_file(), Kind::Save).unwrap()).unwrap().contains("one"));
        assert_eq!(media::read_named(&p.media, "pic.png").unwrap(), [1u8, 2, 3]);
        assert!(media::file_for(&p.media, "other.png").is_none(), "the media folder is the backup's");
        // a safety backup of what was there was made, so the restore can be undone
        assert!(list(&p).len() >= 3);
        // only the newest 5 stay
        for _ in 0..8 {
            std::thread::sleep(std::time::Duration::from_millis(3));
            create(&p, Reason::ByHand).unwrap();
        }
        assert_eq!(list(&p).len(), MAX_BACKUPS);
        // bad names are refused
        assert!(restore(&p, "../library.json").is_err());
        assert!(restore(&p, "backup-x.txt").is_err());
        let _ = fs::remove_dir_all(&p.root);
    }
}
