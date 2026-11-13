//! Backups of the library: encrypted .mdbackup packages in the backups folder, the same files the
//! Minniondeck app makes. One is made when the app starts (if anything changed since the last
//! one), before every import and before every restore. Only the newest 5 are kept.
//!
//! Those are sealed with this install's key (data.key, locked to the Windows account), so they
//! open only here. A password backup (.mdportable) holds the same, sealed with a password instead
//! (PBKDF2-HMAC-SHA256, then AES-256-GCM), so it opens on another PC or after Windows is installed
//! again.
//!
//! A restore never leaves half a library: the backup's pictures go into a new folder and its
//! library into a file beside the real one; only then are the folders swapped and the library put
//! in place (each a rename). If the PC goes off in the middle, the next start finishes or undoes it
//! (`finish_interrupted_restore`).
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

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

/// Backups being made on another thread now: unused pictures aren't cleared out meanwhile (the
/// backup is reading them).
pub static BUSY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Makes a backup on another thread, of the library as it is on disk now (read here, before the
/// caller changes it). The pictures are only ever added to while it runs.
pub fn create_in_background(paths: &Paths, reason: Reason) {
    use std::sync::atomic::Ordering;
    let Ok(library) = vault::read(&paths.library_file(), Kind::Save) else { return };
    let paths = paths.clone();
    BUSY.fetch_add(1, Ordering::SeqCst);
    let spawned = std::thread::Builder::new().name("backup".into()).spawn(move || {
        let _ = create_from(&paths, library, reason);
        BUSY.fetch_sub(1, Ordering::SeqCst);
    });
    if spawned.is_err() {
        BUSY.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Make a backup of the library and media. Returns its file name, or None when there
/// was nothing to back up (no library yet, or nothing changed since the last start-up backup).
pub fn create(paths: &Paths, reason: Reason) -> Result<Option<String>, String> {
    let Ok(library) = vault::read(&paths.library_file(), Kind::Save) else { return Ok(None) };
    create_from(paths, library, reason)
}

fn create_from(paths: &Paths, library: Vec<u8>, reason: Reason) -> Result<Option<String>, String> {
    fs::create_dir_all(&paths.backups).map_err(|e| e.to_string())?;
    let hash = content_hash(&library);
    let hash_path = paths.backups.join(LAST_HASH);
    if reason == Reason::Startup && fs::read_to_string(&hash_path).ok().as_deref() == Some(hash.as_str()) {
        return Ok(None);
    }
    let why = match reason {
        Reason::Startup => "start-up",
        Reason::BeforeImport => "before import",
        Reason::BeforeRestore => "before restore",
        Reason::ByHand => "by hand",
    };
    let now = now_ms();
    let zip = pack(paths, &library, why)?;
    let sealed = package::seal(Kind::Backup, &zip)?;
    let name = format!("backup-{}-{:03}.{EXT}", stamp(now), now % 1000);
    write_atomic(&paths.backups.join(&name), &sealed)?;
    let _ = fs::write(&hash_path, &hash);
    rotate(paths);
    Ok(Some(name))
}

/// The library and every picture as a backup package (not sealed yet).
fn pack(paths: &Paths, library: &[u8], why: &str) -> Result<Vec<u8>, String> {
    let mut w = PackageWriter::new();
    w.add("library.json", library)?;
    let mut media = 0;
    // each picture under its own name (a sealed one is opened: the backup is sealed as a whole)
    for (name, bytes) in media::all_files(&paths.media)? {
        w.add(&format!("media/{name}"), &bytes)?;
        media += 1;
    }
    w.finish(Kind::Backup, &format!("Backup {why}"), json!({ "media": media, "reason": why }), Vec::new())
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
    put_back(paths, &pkg)
}

/// The library file a restore writes before it is put in place
fn staged_library(paths: &Paths) -> PathBuf {
    paths.library.join("library.mdsave.restore")
}

/// Folders beside library\media a restore leaves while it runs: its new pictures, and the old ones
fn folders_named(paths: &Paths, prefix: &str) -> Vec<PathBuf> {
    fs::read_dir(&paths.library)
        .map(|it| it.flatten().filter(|e| e.file_name().to_string_lossy().starts_with(prefix) && e.path().is_dir()).map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Puts a backup package's library and pictures in place of the ones here. What is there now is
/// backed up first. Nothing is removed until everything new is written beside it.
fn put_back(paths: &Paths, pkg: &package::Package) -> Result<(), String> {
    let library = pkg.get("library.json").ok_or("The backup has no library in it.")?.to_vec();
    if serde_json::from_slice::<Value>(&library).is_err() {
        return Err("The library in that backup is damaged.".into());
    }
    create(paths, Reason::BeforeRestore)?;
    fs::create_dir_all(&paths.media).map_err(|e| e.to_string())?;
    let stamp = now_ms();
    let staged = paths.library.join(format!("media.restore-{stamp}"));
    let old = paths.library.join(format!("media.old-{stamp}"));
    let fail = |e: String| {
        let _ = fs::remove_dir_all(&staged);
        let _ = fs::remove_file(staged_library(paths));
        e
    };
    // 1. everything new, beside what is there
    fs::create_dir_all(&staged).map_err(|e| fail(e.to_string()))?;
    for path in pkg.paths() {
        if let Some(file) = path.strip_prefix("media/").and_then(safe_name) {
            media::store(&staged, file, pkg.get(path).unwrap_or_default()).map_err(fail)?;
        }
    }
    vault::write(&staged_library(paths), Kind::Save, &library).map_err(fail)?;
    // 2. the pictures swap folders (two renames), then the library is renamed into place
    fs::rename(&paths.media, &old).map_err(|e| fail(format!("Could not move the pictures aside: {e}. Close anything showing them and try again.")))?;
    if let Err(e) = fs::rename(&staged, &paths.media) {
        let _ = fs::rename(&old, &paths.media);
        return Err(fail(format!("Could not put the backup's pictures in place: {e}")));
    }
    if let Err(e) = fs::rename(staged_library(paths), paths.library_file()) {
        // the next start finishes it (the new pictures are in, the library is ready beside them)
        return Err(format!("Could not put the backup's library in place: {e}. Start Minniondeck again to finish."));
    }
    // 3. only now does the old go
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// At start, before the library is read: a restore that stopped half-way is finished when its new
/// pictures were already in place, else undone. Returns a message when something was put right.
pub fn finish_interrupted_restore(paths: &Paths) -> Option<String> {
    let staged_lib = staged_library(paths);
    let olds = folders_named(paths, "media.old-");
    let mut said = None;
    if staged_lib.exists() {
        if olds.is_empty() {
            // the pictures were never swapped: the restore didn't happen
            let _ = fs::remove_file(&staged_lib);
        } else if !paths.media.exists() {
            // stopped between the two folder renames: the old pictures go back
            let _ = fs::rename(&olds[0], &paths.media);
            let _ = fs::remove_file(&staged_lib);
            said = Some("A restore was stopped part way. Your library was left as it was.".to_string());
        } else if fs::rename(&staged_lib, paths.library_file()).is_ok() {
            // the new pictures are in: the library goes with them
            said = Some("A restore was stopped part way and has been finished.".to_string());
        }
    }
    // what is left from earlier tries
    if !staged_lib.exists() && paths.media.exists() {
        for d in folders_named(paths, "media.old-").into_iter().chain(folders_named(paths, "media.restore-")) {
            let _ = fs::remove_dir_all(d);
        }
    }
    said
}

// ===== Password backups (.mdportable): open on any PC with the password =====

pub const PORTABLE_EXT: &str = "mdportable";
const PORTABLE_MAGIC: &[u8; 20] = b"MINNIONDECK-PORTABLE";
const PORTABLE_VERSION: u8 = 1;
/// PBKDF2-HMAC-SHA256 rounds for a new file (OWASP 2023), and what a file may ask for when opened
const PORTABLE_ROUNDS: u32 = 600_000;
const MIN_ROUNDS: u32 = 100_000;
const MAX_ROUNDS: u32 = 10_000_000;
pub const MIN_PASSWORD: usize = 8;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const PORTABLE_HEADER: usize = 20 + 1 + 4 + SALT_LEN + NONCE_LEN;
/// The biggest password backup opened
const MAX_PORTABLE_BYTES: u64 = 600 * 1024 * 1024;

fn portable_key(password: &str, salt: &[u8], rounds: u32) -> Result<[u8; 32], String> {
    let rounds = NonZeroU32::new(rounds).ok_or("The file is damaged.")?;
    let mut key = [0u8; 32];
    ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA256, rounds, salt, password.as_bytes(), &mut key);
    Ok(key)
}

/// Seals a backup package with a password: magic | version | rounds | salt | nonce | AES-256-GCM,
/// the header bound to the sealed part, so nothing in it can be changed unnoticed.
fn seal_portable(zip: &[u8], password: &str, rounds: u32) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    use ring::rand::SecureRandom;
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    let rng = ring::rand::SystemRandom::new();
    rng.fill(&mut salt).map_err(|_| "Could not make a random salt.")?;
    rng.fill(&mut nonce).map_err(|_| "Could not make a random nonce.")?;
    let key = portable_key(password, &salt, rounds)?;
    let mut out = Vec::with_capacity(PORTABLE_HEADER + zip.len() + 16);
    out.extend_from_slice(PORTABLE_MAGIC);
    out.push(PORTABLE_VERSION);
    out.extend_from_slice(&rounds.to_be_bytes());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).map_err(|_| "Could not encrypt the backup.")?;
    let sealed = cipher.encrypt(aes_gcm::Nonce::from_slice(&nonce), Payload { msg: zip, aad: &out }).map_err(|_| "Could not encrypt the backup.")?;
    out.extend_from_slice(&sealed);
    Ok(out)
}

fn open_portable(bytes: &[u8], password: &str) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    if bytes.len() < PORTABLE_HEADER + 16 || &bytes[..20] != PORTABLE_MAGIC {
        return Err("That isn't a MinnionDeck password backup.".into());
    }
    if bytes[20] != PORTABLE_VERSION {
        return Err("That backup was made by a newer version of MinnionDeck.".into());
    }
    let rounds = u32::from_be_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
    if !(MIN_ROUNDS..=MAX_ROUNDS).contains(&rounds) {
        return Err("That backup is damaged.".into());
    }
    let (header, sealed) = bytes.split_at(PORTABLE_HEADER);
    let salt = &header[25..25 + SALT_LEN];
    let nonce = &header[25 + SALT_LEN..];
    let key = portable_key(password, salt, rounds)?;
    let cipher = aes_gcm::Aes256Gcm::new_from_slice(&key).map_err(|_| "That backup can't be opened.")?;
    cipher.decrypt(aes_gcm::Nonce::from_slice(nonce), Payload { msg: sealed, aad: header }).map_err(|_| "That password isn't right, or the file is damaged.".to_string())
}

/// Saves the library and every picture to `file`, sealed with `password`. Blocks (a second or two).
pub fn export_portable(paths: &Paths, file: &Path, password: &str) -> Result<(), String> {
    export_portable_rounds(paths, file, password, PORTABLE_ROUNDS)
}

fn export_portable_rounds(paths: &Paths, file: &Path, password: &str, rounds: u32) -> Result<(), String> {
    if password.chars().count() < MIN_PASSWORD {
        return Err(format!("Use a password of at least {MIN_PASSWORD} characters."));
    }
    let library = vault::read(&paths.library_file(), Kind::Save).map_err(|_| "There is no library to save yet.".to_string())?;
    let zip = pack(paths, &library, "password backup")?;
    write_atomic(file, &seal_portable(&zip, password, rounds)?)
}

/// Puts a password backup back (what is here now is backed up first, as for any restore). Blocks.
pub fn import_portable(paths: &Paths, file: &Path, password: &str) -> Result<(), String> {
    let len = fs::metadata(file).map_err(|e| format!("Could not open {}: {e}", file.display()))?.len();
    if len > MAX_PORTABLE_BYTES {
        return Err("That file is too big to be a MinnionDeck backup.".into());
    }
    let bytes = fs::read(file).map_err(|e| format!("Could not open {}: {e}", file.display()))?;
    let zip = open_portable(&bytes, password)?;
    drop(bytes);
    let pkg = package::read(&zip, Kind::Backup)?;
    drop(zip);
    put_back(paths, &pkg)
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
        // nothing is left beside the library by a restore
        let left: Vec<String> = fs::read_dir(&p.library).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != "media" && n != "library.mdsave").collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = fs::remove_dir_all(&p.root);
    }

    fn lib_says(p: &Paths) -> String {
        String::from_utf8(vault::read(&p.library_file(), Kind::Save).unwrap()).unwrap()
    }

    // the PC going off in the middle of a restore never leaves half of one
    #[test]
    fn a_restore_stopped_half_way_is_finished_or_undone_at_the_next_start() {
        let p = temp_paths("halfway");
        let old_lib = br#"{"data":{"profiles":["old"]}}"#;
        let set_up = |p: &Paths| {
            let _ = fs::remove_dir_all(&p.library);
            p.make_folders().unwrap();
            vault::write(&p.library_file(), Kind::Save, old_lib).unwrap();
            media::store(&p.media, "old.png", b"old").unwrap();
            let staged = p.library.join("media.restore-1");
            fs::create_dir_all(&staged).unwrap();
            media::store(&staged, "new.png", b"new").unwrap();
            vault::write(&staged_library(p), Kind::Save, br#"{"data":{"profiles":["new"]}}"#).unwrap();
            staged
        };
        // stopped before the pictures were swapped: as it was
        set_up(&p);
        assert_eq!(finish_interrupted_restore(&p), None);
        assert!(lib_says(&p).contains("old") && media::read_named(&p.media, "old.png").is_some());
        assert!(!staged_library(&p).exists() && folders_named(&p, "media.restore-").is_empty());
        // stopped between the folder renames: the old pictures come back
        let staged = set_up(&p);
        fs::rename(&p.media, p.library.join("media.old-1")).unwrap();
        assert!(finish_interrupted_restore(&p).unwrap().contains("left as it was"));
        assert!(lib_says(&p).contains("old") && media::read_named(&p.media, "old.png").is_some());
        assert!(!staged.exists() && folders_named(&p, "media.old-").is_empty());
        // stopped after the pictures were swapped: the library follows them
        let staged = set_up(&p);
        fs::rename(&p.media, p.library.join("media.old-1")).unwrap();
        fs::rename(&staged, &p.media).unwrap();
        assert!(finish_interrupted_restore(&p).unwrap().contains("finished"));
        assert!(lib_says(&p).contains("new") && media::read_named(&p.media, "new.png").is_some());
        assert!(folders_named(&p, "media.old-").is_empty());
        // nothing to do: nothing happens
        assert_eq!(finish_interrupted_restore(&p), None);
        let _ = fs::remove_dir_all(&p.root);
    }

    #[test]
    fn a_password_backup_opens_with_its_password_only_and_restores() {
        let p = temp_paths("portable");
        vault::write(&p.library_file(), Kind::Save, br#"{"schema_version":1,"saved_at":1,"data":{"profiles":["mine"]}}"#).unwrap();
        media::store(&p.media, "pic.png", &[7u8, 8, 9]).unwrap();
        let file = p.root.join("move.mdportable");
        assert!(export_portable_rounds(&p, &file, "short", MIN_ROUNDS).unwrap_err().contains("at least"));
        export_portable_rounds(&p, &file, "correct horse", MIN_ROUNDS).unwrap();
        let raw = fs::read(&file).unwrap();
        assert!(raw.starts_with(PORTABLE_MAGIC) && !String::from_utf8_lossy(&raw).contains("mine"), "nothing readable in it");
        // it doesn't use this install's key: only the password opens it
        assert!(open_portable(&raw, "wrong horse").unwrap_err().contains("password"));
        let mut changed = raw.clone();
        changed[22] ^= 1; // the rounds, part of what is checked
        assert!(open_portable(&changed, "correct horse").is_err());
        let last = raw.len() - 1;
        let mut changed = raw.clone();
        changed[last] ^= 1;
        assert!(open_portable(&changed, "correct horse").is_err());
        assert!(open_portable(b"MINNIONDECK\x02\x02", "x").unwrap_err().contains("isn't"));
        // the library changes, then the password backup is put back
        vault::write(&p.library_file(), Kind::Save, br#"{"data":{"profiles":["other"]}}"#).unwrap();
        fs::remove_file(media::file_for(&p.media, "pic.png").unwrap()).unwrap();
        assert!(import_portable(&p, &file, "wrong horse").is_err());
        assert!(lib_says(&p).contains("other"), "a wrong password changes nothing");
        import_portable(&p, &file, "correct horse").unwrap();
        assert!(lib_says(&p).contains("mine"));
        assert_eq!(media::read_named(&p.media, "pic.png").unwrap(), [7u8, 8, 9]);
        let _ = fs::remove_dir_all(&p.root);
    }
}
