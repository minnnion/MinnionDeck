// .mdprofile and .mdbackup files: a zip with a manifest, encrypted so only Minniondeck opens it.
//
// File layout:  "MINNIONDECK" | version (1 byte) | kind (1 byte) | nonce (12 bytes) | AES-256-GCM(zip)
// The 13-byte header is authenticated too, so a file can't be relabelled as another kind.
//
// The key ships inside the app. That keeps profiles from being unzipped or edited by hand,
// but someone who takes the app apart could still find it: treat it as a lock, not a vault.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::storage::{now_ms, sha256_hex};

const MAGIC: &[u8; 11] = b"MINNIONDECK";
const CONTAINER_VERSION: u8 = 1;
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
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Profile => "profile",
            Kind::Backup => "backup",
        }
    }
}

// The key is kept as two halves that are combined at run time, so it never sits in the
// app as one plain run of bytes.
const KEY_A: [u8; 32] = [
    0x5e, 0x91, 0x0c, 0xd7, 0x3a, 0x66, 0xb2, 0x18, 0xe4, 0x7f, 0x29, 0xc3, 0x8d, 0x40, 0xfa, 0x15,
    0x72, 0xae, 0x09, 0x63, 0xdb, 0x34, 0x88, 0xc1, 0x1f, 0x5a, 0xe6, 0x97, 0x2c, 0x7b, 0xb0, 0x4e,
];
const KEY_B: [u8; 32] = [
    0xa3, 0x2f, 0x76, 0x1b, 0xc8, 0x05, 0x9e, 0x64, 0x3d, 0xf1, 0x52, 0x8a, 0x17, 0xbc, 0x6e, 0xd9,
    0x40, 0x0b, 0xf5, 0x2e, 0x86, 0x79, 0x13, 0xca, 0x64, 0x9f, 0x31, 0x58, 0xe2, 0x07, 0x4d, 0xb6,
];

fn cipher() -> Aes256Gcm {
    let mut key = [0u8; 32];
    for (i, k) in key.iter_mut().enumerate() {
        *k = KEY_A[i] ^ KEY_B[i];
    }
    Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key))
}

pub fn seal(kind: Kind, zip_bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.push(CONTAINER_VERSION);
    header.push(kind as u8);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let sealed = cipher()
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
    if bytes[11] > CONTAINER_VERSION {
        return Err("This file was made by a newer version of Minniondeck.".into());
    }
    if bytes[12] != want as u8 {
        return Err(match want {
            Kind::Profile => "This is a backup, not a profile. Restore it from Settings → Backup.".into(),
            Kind::Backup => "This is a profile, not a backup.".into(),
        });
    }
    let (header, rest) = bytes.split_at(HEADER_LEN);
    let (nonce, sealed) = rest.split_at(NONCE_LEN);
    cipher()
        .decrypt(Nonce::from_slice(nonce), Payload { msg: sealed, aad: header })
        .map_err(|_| "The file is damaged or was changed outside Minniondeck.".into())
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
