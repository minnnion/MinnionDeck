//! Encrypted files. Everything this app saves about you is kept as a sealed `.md…` file, the same
//! lock as .mdprofile and .mdbackup (AES-256-GCM, the kind of file is part of what is checked, so a
//! save can't be swapped for a picture):
//!
//!   .mdsave    save data: the library, settings, plugin choices, paired phones...
//!   .mdmedia   a picture or GIF you imported (the name keeps its own ending: pic.png.mdmedia)
//!   .mdprofile, .mdbackup   made by Export and Back up (package.rs)
//!
//! The key ships inside the app, so these files are locked, not secret: nobody can open one by
//! hand or with another tool, but anything with this app can. They move between PCs as they are.
//!
//! Data kept in plain files by an earlier version is changed over by `migrate`: the sealed file is
//! written and read back first, and only then is the plain one removed.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::package::{self, Kind};
use super::util::write_atomic;

pub const SAVE_EXT: &str = "mdsave";
pub const MEDIA_EXT: &str = "mdmedia";

/// Seals `bytes` and writes them to `path` (a temporary file first, then moved into place).
pub fn write(path: &Path, kind: Kind, bytes: &[u8]) -> Result<(), String> {
    write_atomic(path, &package::seal(kind, bytes)?)
}

/// Reads and unseals a file. An error says whether it isn't one of ours or was changed.
pub fn read(path: &Path, kind: Kind) -> Result<Vec<u8>, String> {
    let sealed = fs::read(path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    package::open(&sealed, kind)
}

pub fn write_json(path: &Path, kind: Kind, v: &Value) -> Result<(), String> {
    write(path, kind, v.to_string().as_bytes())
}

pub fn read_json(path: &Path, kind: Kind) -> Result<Value, String> {
    serde_json::from_slice(&read(path, kind)?).map_err(|e| format!("{} isn't valid data ({e})", path.display()))
}

/// Seals a plain file that an earlier version left. Does nothing when the sealed file is there
/// already or there is no plain one. True when a file was changed over.
pub fn migrate(plain: &Path, sealed: &Path, kind: Kind) -> Result<bool, String> {
    if sealed.exists() || !plain.is_file() {
        return Ok(false);
    }
    let bytes = fs::read(plain).map_err(|e| format!("Could not read {}: {e}", plain.display()))?;
    write(sealed, kind, &bytes)?;
    // the plain file goes only once the sealed one reads back as the same
    if read(sealed, kind)? != bytes {
        let _ = fs::remove_file(sealed);
        return Err(format!("Could not seal {} safely, so it was left as it was.", plain.display()));
    }
    let _ = fs::remove_file(plain);
    Ok(true)
}

/// The sealed file for a plain one: `x.json` → `x.mdsave`.
pub fn sealed_name(plain: &Path) -> PathBuf {
    plain.with_extension(SAVE_EXT)
}

/// Whether a path is a sealed file of ours by its ending.
pub fn is_sealed(path: &Path) -> bool {
    matches!(path.extension().and_then(|e| e.to_str()), Some(SAVE_EXT | MEDIA_EXT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::util::now_ms;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("md-vault-{tag}-{}", now_ms()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn data_is_unreadable_on_disk_and_comes_back_whole() {
        let d = dir("roundtrip");
        let f = d.join("a.mdsave");
        write_json(&f, Kind::Save, &serde_json::json!({ "secret": "hunter2", "n": [1, 2, 3] })).unwrap();
        let raw = fs::read(&f).unwrap();
        assert!(raw.starts_with(b"MINNIONDECK"));
        assert!(!String::from_utf8_lossy(&raw).contains("hunter2"), "nothing readable on disk");
        assert_eq!(read_json(&f, Kind::Save).unwrap()["secret"], "hunter2");
        // the kind is checked: a save isn't a picture, and a changed byte is caught
        assert!(read(&f, Kind::Media).is_err());
        let mut bad = raw.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        fs::write(&f, bad).unwrap();
        assert!(read(&f, Kind::Save).unwrap_err().contains("damaged"));
        // two writes of the same data are different bytes (a fresh nonce each time)
        write(&f, Kind::Save, b"same").unwrap();
        let one = fs::read(&f).unwrap();
        write(&f, Kind::Save, b"same").unwrap();
        assert_ne!(one, fs::read(&f).unwrap());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_plain_file_is_sealed_then_removed_and_never_twice() {
        let d = dir("migrate");
        let (plain, sealed) = (d.join("library.json"), d.join("library.mdsave"));
        assert!(!migrate(&plain, &sealed, Kind::Save).unwrap(), "nothing to do");
        fs::write(&plain, br#"{"data":{"profiles":["mine"]}}"#).unwrap();
        assert!(migrate(&plain, &sealed, Kind::Save).unwrap());
        assert!(!plain.exists() && sealed.exists());
        assert_eq!(read_json(&sealed, Kind::Save).unwrap()["data"]["profiles"][0], "mine");
        // a plain file that turns up later doesn't replace the sealed one
        fs::write(&plain, b"old").unwrap();
        assert!(!migrate(&plain, &sealed, Kind::Save).unwrap());
        assert!(plain.exists());
        assert_eq!(read_json(&sealed, Kind::Save).unwrap()["data"]["profiles"][0], "mine");
        assert_eq!(sealed_name(Path::new("x/y/settings.json")), PathBuf::from("x/y/settings.mdsave"));
        assert!(is_sealed(Path::new("a.mdmedia")) && is_sealed(Path::new("a.mdsave")) && !is_sealed(Path::new("a.json")));
        let _ = fs::remove_dir_all(&d);
    }
}
