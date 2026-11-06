// .mdprofile and .mdbackup files: a zip with a manifest, encrypted so only Minniondeck opens it.
//
// File layout:  "MINNIONDECK" | version (1 byte) | kind (1 byte) | nonce (12 bytes) | AES-256-GCM(zip)
// The 13-byte header is authenticated too, so a file can't be relabelled as another kind.
//
// Two keys:
// - Your own data (saves, pictures, backups: version 2) is sealed with a key made for this install
//   the first time it runs, kept in data.key beside the app. It is never in the source or the exe,
//   so nobody else's copy of Minniondeck can open your files.
// - Files made to be shared (.mdprofile: version 1) must open on every PC, so they use one key
//   that ships with the app and is public. That only keeps them from being edited by hand.
//   Version-1 saves from older builds are read with it and sealed again with this install's key.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::backend::util::{now_ms, sha256_hex};

const MAGIC: &[u8; 11] = b"MINNIONDECK";
/// Shared files, sealed with the public key
const SHARED_VERSION: u8 = 1;
/// This install's own data, sealed with data.key
const LOCAL_VERSION: u8 = 2;
/// The file beside the app that holds this install's key
pub const DATA_KEY_FILE: &str = "data.key";
const HEADER_LEN: usize = 13;
const NONCE_LEN: usize = 12;
pub const FORMAT: &str = "minniondeck";
pub const FORMAT_VERSION: u32 = 1;
/// Limits that stop a damaged or hostile file from filling memory
const MAX_ENTRY_BYTES: u64 = 100 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Profile = 1,
    Backup = 2,
    /// Save data: the library, settings and the like (vault.rs)
    Save = 3,
    /// A picture or GIF
    Media = 4,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Profile => "profile",
            Kind::Backup => "backup",
            Kind::Save => "save",
            Kind::Media => "media",
        }
    }
}

// The shared key (public: it has been in the source since the first release), in two halves.
const KEY_A: [u8; 32] = [
    0x5e, 0x91, 0x0c, 0xd7, 0x3a, 0x66, 0xb2, 0x18, 0xe4, 0x7f, 0x29, 0xc3, 0x8d, 0x40, 0xfa, 0x15,
    0x72, 0xae, 0x09, 0x63, 0xdb, 0x34, 0x88, 0xc1, 0x1f, 0x5a, 0xe6, 0x97, 0x2c, 0x7b, 0xb0, 0x4e,
];
const KEY_B: [u8; 32] = [
    0xa3, 0x2f, 0x76, 0x1b, 0xc8, 0x05, 0x9e, 0x64, 0x3d, 0xf1, 0x52, 0x8a, 0x17, 0xbc, 0x6e, 0xd9,
    0x40, 0x0b, 0xf5, 0x2e, 0x86, 0x79, 0x13, 0xca, 0x64, 0x9f, 0x31, 0x58, 0xe2, 0x07, 0x4d, 0xb6,
];

fn shared_cipher() -> Aes256Gcm {
    let mut key = [0u8; 32];
    for (i, k) in key.iter_mut().enumerate() {
        *k = KEY_A[i] ^ KEY_B[i];
    }
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key))
}

impl Kind {
    /// Kept on this PC only, so sealed with this install's key
    fn is_local(self) -> bool {
        matches!(self, Kind::Save | Kind::Media | Kind::Backup)
    }
}

static DATA_KEY: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();

/// Reads this install's key, or makes it the first time. Refuses to make a new one when files
/// sealed with the old one are there: that would lock them away for good.
pub fn init_data_key(root: &std::path::Path) -> Result<(), String> {
    if DATA_KEY.get().is_some() {
        return Ok(());
    }
    let key = load_or_make_key(root)?;
    let _ = DATA_KEY.set(key);
    reseal_old(root);
    Ok(())
}

fn load_or_make_key(root: &std::path::Path) -> Result<[u8; 32], String> {
    let file = root.join(DATA_KEY_FILE);
    Ok(match std::fs::read(&file) {
        Ok(bytes) => <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| format!("{} is damaged, so your saved data can't be opened. Put back the data.key from a copy of this folder.", file.display()))?,
        Err(_) => {
            if let Some(f) = find_local_sealed(root) {
                return Err(format!("{} is missing, so your saved data (like {}) can't be opened. Put back the data.key from a copy of this folder; nothing has been changed.", file.display(), f.display()));
            }
            let mut k = [0u8; 32];
            ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut k).map_err(|_| "Couldn't make this install's data key.".to_string())?;
            std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
            crate::backend::util::write_atomic(&file, &k)?;
            k
        }
    })
}

const LOCAL_EXTS: [&str; 3] = ["mdsave", "mdmedia", "mdbackup"];

/// Every save, picture and backup file under the folder (not inside plugins).
fn local_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    fn walk(dir: &std::path::Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < 4 && p.file_name().is_some_and(|n| n != "plugins" && n != "plugin-run" && n != "target") {
                    walk(&p, depth + 1, out);
                }
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| LOCAL_EXTS.contains(&x)) {
                out.push(p);
            }
        }
    }
    let mut out = vec![];
    walk(root, 0, &mut out);
    out
}

fn header_of(p: &std::path::Path) -> Option<(u8, u8)> {
    let mut h = [0u8; HEADER_LEN];
    std::fs::File::open(p).ok()?.read_exact(&mut h).ok()?;
    (&h[..MAGIC.len()] == MAGIC).then_some((h[11], h[12]))
}

fn find_local_sealed(root: &std::path::Path) -> Option<std::path::PathBuf> {
    local_files(root).into_iter().find(|p| header_of(p).is_some_and(|(v, _)| v == LOCAL_VERSION))
}

/// Files from older builds, sealed with the public key, are sealed again with this install's.
fn reseal_old(root: &std::path::Path) {
    for p in local_files(root) {
        let Some((SHARED_VERSION, k)) = header_of(&p) else { continue };
        let Some(kind) = [Kind::Save, Kind::Media, Kind::Backup].into_iter().find(|x| *x as u8 == k) else { continue };
        let Ok(bytes) = std::fs::read(&p) else { continue };
        if let Ok(plain) = open(&bytes, kind) {
            if let Ok(again) = seal(kind, &plain) {
                let _ = crate::backend::util::write_atomic(&p, &again);
            }
        }
    }
}

fn local_cipher() -> Result<Aes256Gcm, String> {
    if DATA_KEY.get().is_none() {
        #[cfg(test)]
        let _ = DATA_KEY.set([7u8; 32]);
        #[cfg(not(test))]
        init_data_key(&crate::backend::paths::Paths::find().root)?;
    }
    let k = DATA_KEY.get().ok_or("This install's data key couldn't be read.")?;
    Ok(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(k)))
}

pub fn seal(kind: Kind, zip_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let (version, cipher) = if kind.is_local() { (LOCAL_VERSION, local_cipher()?) } else { (SHARED_VERSION, shared_cipher()) };
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.push(version);
    header.push(kind as u8);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let sealed = cipher
        .encrypt(&nonce, Payload { msg: zip_bytes, aad: &header })
        .map_err(|_| "The file could not be encrypted.".to_string())?;
    let mut out = header;
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub fn open(bytes: &[u8], want: Kind) -> Result<Vec<u8>, String> {
    if bytes.len() < HEADER_LEN + NONCE_LEN + 16 || &bytes[..MAGIC.len()] != MAGIC {
        return Err("This is not a Minniondeck file.".into());
    }
    if bytes[11] > LOCAL_VERSION {
        return Err("This file was made by a newer version of Minniondeck.".into());
    }
    if bytes[12] != want as u8 {
        return Err(match want {
            Kind::Profile => "This is a backup, not a profile. Restore it from Settings → Backup.".into(),
            Kind::Backup => "This is a profile, not a backup.".into(),
            Kind::Save | Kind::Media => "This file isn't the kind of Minniondeck data it should be.".into(),
        });
    }
    let cipher = match bytes[11] {
        LOCAL_VERSION if want.is_local() => local_cipher()?,
        LOCAL_VERSION => return Err("This file can only be opened by the Minniondeck that made it.".into()),
        _ => shared_cipher(),
    };
    let (header, rest) = bytes.split_at(HEADER_LEN);
    let (nonce, sealed) = rest.split_at(NONCE_LEN);
    cipher.decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: header }).map_err(|_| {
        if bytes[11] == LOCAL_VERSION {
            "This file is damaged, or it was made by another Minniondeck install (each install has its own key).".to_string()
        } else {
            "The file is damaged or was changed outside Minniondeck.".to_string()
        }
    })
}

#[derive(Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// Read first. It says what the package holds and lets every file be checked.
#[derive(Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub kind: String,
    pub app_version: String,
    pub created_ms: u64,
    pub name: String,
    #[serde(default)]
    pub contents: Value,
    pub files: Vec<FileEntry>,
    /// Plugins the buttons use, so missing ones can be offered for install
    #[serde(default)]
    pub plugins: Vec<String>,
}

pub struct PackageWriter {
    zip: ZipWriter<Cursor<Vec<u8>>>,
    files: Vec<FileEntry>,
}

impl PackageWriter {
    pub fn new() -> Self {
        PackageWriter { zip: ZipWriter::new(Cursor::new(Vec::new())), files: Vec::new() }
    }

    pub fn add(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
        // JSON squeezes well; pictures and GIFs are already compressed, so they are stored as they are
        let method = if path.ends_with(".json") { CompressionMethod::Deflated } else { CompressionMethod::Stored };
        self.zip
            .start_file(path, SimpleFileOptions::default().compression_method(method))
            .and_then(|_| self.zip.write_all(bytes).map_err(Into::into))
            .map_err(|e| format!("Could not pack {path}: {e}"))?;
        self.files.push(FileEntry { path: path.to_string(), bytes: bytes.len() as u64, sha256: sha256_hex(bytes) });
        Ok(())
    }

    pub fn add_json(&mut self, path: &str, value: &Value) -> Result<(), String> {
        self.add(path, value.to_string().as_bytes())
    }

    pub fn add_dir(&mut self, path: &str) -> Result<(), String> {
        self.zip.add_directory(path, SimpleFileOptions::default()).map_err(|e| format!("Could not pack {path}: {e}"))
    }

    /// Write the manifest last, once every file and its checksum is known
    pub fn finish(mut self, kind: Kind, name: &str, contents: Value, plugins: Vec<String>) -> Result<Vec<u8>, String> {
        let manifest = Manifest {
            format: FORMAT.into(),
            format_version: FORMAT_VERSION,
            kind: kind.word().into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            created_ms: now_ms(),
            name: name.into(),
            contents,
            files: self.files.clone(),
            plugins,
        };
        let text = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
        self.zip
            .start_file("manifest.json", SimpleFileOptions::default().compression_method(CompressionMethod::Deflated))
            .and_then(|_| self.zip.write_all(&text).map_err(Into::into))
            .map_err(|e| format!("Could not pack the manifest: {e}"))?;
        let cursor = self.zip.finish().map_err(|e| format!("Could not finish the package: {e}"))?;
        Ok(cursor.into_inner())
    }
}

pub struct Package {
    pub manifest: Manifest,
    files: HashMap<String, Vec<u8>>,
}

impl Package {
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(|v| v.as_slice())
    }

    pub fn json(&self, path: &str) -> Result<Value, String> {
        let bytes = self.get(path).ok_or_else(|| format!("{path} is missing from the file."))?;
        serde_json::from_slice(bytes).map_err(|_| format!("{path} in the file is damaged."))
    }

    pub fn json_or(&self, path: &str, fallback: Value) -> Result<Value, String> {
        if self.get(path).is_some() { self.json(path) } else { Ok(fallback) }
    }

    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.files.keys()
    }
}

/// Only plain relative paths inside the package are accepted
fn safe_path(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 300
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && p.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, path: &str, limit: u64) -> Result<Vec<u8>, String> {
    let entry = archive.by_name(path).map_err(|_| format!("{path} is missing from the file."))?;
    let mut out = Vec::new();
    entry.take(limit + 1).read_to_end(&mut out).map_err(|_| format!("{path} in the file is damaged."))?;
    if out.len() as u64 > limit {
        return Err(format!("{path} in the file is too large."));
    }
    Ok(out)
}

/// Open the zip, read the manifest first, then read and check every file it lists
pub fn read(zip_bytes: &[u8], kind: Kind) -> Result<Package, String> {
    let mut archive = ZipArchive::new(Cursor::new(zip_bytes)).map_err(|_| "The file is damaged.".to_string())?;
    let manifest: Manifest = serde_json::from_slice(&read_entry(&mut archive, "manifest.json", 4 * 1024 * 1024)?)
        .map_err(|_| "The file's manifest is damaged.".to_string())?;
    if manifest.format != FORMAT {
        return Err("This is not a Minniondeck file.".into());
    }
    if manifest.format_version > FORMAT_VERSION {
        return Err("This file was made by a newer version of Minniondeck.".into());
    }
    if manifest.kind != kind.word() {
        return Err(format!("This file holds a {}, not a {}.", manifest.kind, kind.word()));
    }
    let mut files = HashMap::new();
    let mut total = 0u64;
    for f in &manifest.files {
        if !safe_path(&f.path) {
            return Err("The file lists a path that is not allowed.".into());
        }
        total += f.bytes;
        if f.bytes > MAX_ENTRY_BYTES || total > MAX_TOTAL_BYTES {
            return Err("The file is too large.".into());
        }
        let bytes = read_entry(&mut archive, &f.path, f.bytes)?;
        if bytes.len() as u64 != f.bytes || sha256_hex(&bytes) != f.sha256 {
            return Err(format!("{} in the file is damaged.", f.path));
        }
        files.insert(f.path.clone(), bytes);
    }
    Ok(Package { manifest, files })
}

#[cfg(test)]
mod key_tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("md-key-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn each_install_makes_its_own_key_once() {
        let d = dir("new");
        let k1 = load_or_make_key(&d).unwrap();
        assert_eq!(std::fs::read(d.join(DATA_KEY_FILE)).unwrap(), k1);
        assert_eq!(load_or_make_key(&d).unwrap(), k1, "the same key is read back");
        let other = dir("other");
        assert_ne!(load_or_make_key(&other).unwrap(), k1, "another install gets another key");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn a_lost_key_is_never_replaced_while_data_sealed_with_it_is_there() {
        let d = dir("lost");
        std::fs::create_dir_all(d.join("library")).unwrap();
        std::fs::write(d.join("library").join("library.mdsave"), seal(Kind::Save, b"buttons").unwrap()).unwrap();
        let e = load_or_make_key(&d).unwrap_err();
        assert!(e.contains("missing") && e.contains("library.mdsave"), "{e}");
        assert!(!d.join(DATA_KEY_FILE).exists(), "no new key was made");
        std::fs::write(d.join(DATA_KEY_FILE), b"short").unwrap();
        assert!(load_or_make_key(&d).unwrap_err().contains("damaged"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn saves_use_this_installs_key_and_profiles_the_shared_one() {
        let save = seal(Kind::Save, b"mine").unwrap();
        assert_eq!(save[11], LOCAL_VERSION);
        assert_eq!(open(&save, Kind::Save).unwrap(), b"mine");
        let prof = seal(Kind::Profile, b"shared").unwrap();
        assert_eq!(prof[11], SHARED_VERSION);
        assert_eq!(open(&prof, Kind::Profile).unwrap(), b"shared");
        // a save from an older build (shared key) still opens, and is sealed again with this install's key
        let d = dir("old");
        let mut old = prof.clone();
        old[12] = Kind::Save as u8;
        let old = {
            let mut h = Vec::from(&MAGIC[..]);
            h.push(SHARED_VERSION);
            h.push(Kind::Save as u8);
            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
            let body = shared_cipher().encrypt(&nonce, Payload { msg: b"old save", aad: &h }).unwrap();
            h.extend_from_slice(&nonce);
            h.extend_from_slice(&body);
            h
        };
        assert_eq!(open(&old, Kind::Save).unwrap(), b"old save");
        std::fs::write(d.join("settings.mdsave"), &old).unwrap();
        reseal_old(&d);
        let again = std::fs::read(d.join("settings.mdsave")).unwrap();
        assert_eq!(again[11], LOCAL_VERSION);
        assert_eq!(open(&again, Kind::Save).unwrap(), b"old save");
        let _ = std::fs::remove_dir_all(&d);
    }
}
