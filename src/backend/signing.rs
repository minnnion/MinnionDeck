//! Plugin signing. Authors sign plugins with their own Ed25519 key; Minniondeck runs a plugin
//! without Developer mode only when it is signed by a trusted author and none of its files changed.
//!
//! - `Minniondeck.exe plugin key --name "Your name"` makes the author's key once. The private key
//!   stays on their PC in <app data>\author\key.bin, locked to their Windows account (DPAPI). The
//!   public key goes in <name>.mdkey, the file they send to be trusted.
//! - `plugin pack` signs automatically when the PC has an author key: the pack gets signature.json,
//!   {"v":1,"author","key","files":{"<name>":"<sha256 hex>",…},"sig"}, where sig signs `message()`.
//! - Trusted keys are the built-in ones below plus the ones the user added in Settings › Plugins.
//!   Every file of an installed plugin must be listed in signature.json with a matching hash.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const SIG_FILE: &str = "signature.json";

/// Authors whose plugins run on every copy of Minniondeck without Developer mode. To add one,
/// they run `Minniondeck.exe plugin key` and send their .mdkey file; copy its name and key here.
pub const BUILT_IN: &[(&str, &str)] = &[
    ("Minniondeck", "2cfPLPsoukI1rpnbvbrmiU7y+vnC8Bc5bSNzFFswy1Y="),
    // Signs the General plugins (Timer, System Monitor, Soundboard, Weather); key 0cc7 5fff 43c1 1a23
    ("minnion", "MS4LTxUA6qVdJYTflEGMUIU3etDXurAlytmO/RUMy98="),
];

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// An author's public key, as in a .mdkey file
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AuthorKey {
    pub name: String,
    pub key: String,
}

/// What Minniondeck makes of a plugin's signature
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "trust", rename_all = "lowercase")]
pub enum Trust {
    /// Signed by a built-in author
    Trusted { author: String },
    /// Signed by an author the user added
    User { author: String },
    /// Signed, but by an author this PC doesn't trust
    Untrusted { author: String },
    Unsigned,
    /// Signed, but the signature or the files don't match: never runs
    Bad { why: String },
}

impl Trust {
    /// Runs without Developer mode
    pub fn trusted(&self) -> bool {
        matches!(self, Trust::Trusted { .. } | Trust::User { .. })
    }
}

/// A key shown short, for people to compare: "1a2b 3c4d 5e6f 7a8b"
pub fn fingerprint(key: &str) -> String {
    let hex = crate::backend::util::sha256_hex(key.as_bytes());
    hex[..16].as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).to_string()).collect::<Vec<_>>().join(" ")
}

fn valid_key(key: &str) -> bool {
    B64.decode(key).is_ok_and(|k| k.len() == 32)
}

/// The bytes the signature covers: the author, the key and every file's hash, in name order
fn message(author: &str, key: &str, files: &BTreeMap<String, String>) -> Vec<u8> {
    let mut m = format!("minniondeck-plugin-signature-v1\nauthor:{author}\nkey:{key}\n");
    for (name, hash) in files {
        m.push_str(&format!("{name} {hash}\n"));
    }
    m.into_bytes()
}

fn author_dir(root: &Path) -> PathBuf {
    root.join("author")
}

/// This PC's author key, if it has one: (name, public key)
pub fn author(root: &Path) -> Option<AuthorKey> {
    let text = fs::read_to_string(author_dir(root).join("author.json")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Make this PC's author key. Returns the .mdkey file to send to whoever trusts plugins.
pub fn make_key(root: &Path, name: &str, out_dir: &Path) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Give your author name, 1 to 40 characters: plugin key --name \"Your name\"".into());
    }
    let dir = author_dir(root);
    if dir.join("key.bin").exists() {
        let a = author(root).map(|a| a.name).unwrap_or_default();
        return Err(format!("This PC already has an author key ({a}). Its public key is in {}. Making a new one would stop your signed plugins from being trusted.", dir.display()));
    }
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| "Could not make a key.".to_string())?;
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|_| "Could not make a key.".to_string())?;
    let key = B64.encode(pair.public_key().as_ref());
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::backend::util::write_atomic(&dir.join("key.bin"), &protect(pkcs8.as_ref())?)?;
    let public = AuthorKey { name: name.to_string(), key };
    crate::backend::util::write_atomic(&dir.join("author.json"), serde_json::to_string_pretty(&public).unwrap_or_default().as_bytes())?;
    let file_name: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let out = out_dir.join(format!("{file_name}.mdkey"));
    let body = json!({ "v": 1, "name": public.name, "key": public.key });
    fs::write(&out, serde_json::to_string_pretty(&body).unwrap_or_default()).map_err(|e| format!("Could not write {}: {e}", out.display()))?;
    // A copy next to the private key, in case the shared one is lost
    let _ = fs::write(dir.join(format!("{file_name}.mdkey")), serde_json::to_string_pretty(&body).unwrap_or_default());
    Ok(out)
}

/// Sign a plugin's files (name in the pack → bytes). None when this PC has no author key.
pub fn sign(root: &Path, files: &[(String, Vec<u8>)]) -> Result<Option<(AuthorKey, Vec<u8>)>, String> {
    let Some(me) = author(root) else { return Ok(None) };
    let sealed = fs::read(author_dir(root).join("key.bin")).map_err(|_| "The author key file is missing. Run plugin key again.".to_string())?;
    let pkcs8 = unprotect(&sealed)?;
    let pair = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| "The author key can't be read.".to_string())?;
    if B64.encode(pair.public_key().as_ref()) != me.key {
        return Err("The author key doesn't match author.json.".into());
    }
    let hashes: BTreeMap<String, String> = files.iter().filter(|(n, _)| n != SIG_FILE).map(|(n, b)| (n.clone(), crate::backend::util::sha256_hex(b))).collect();
    let sig = pair.sign(&message(&me.name, &me.key, &hashes));
    let body = json!({ "v": 1, "author": me.name, "key": me.key, "files": hashes, "sig": B64.encode(sig.as_ref()) });
    Ok(Some((me, serde_json::to_string_pretty(&body).unwrap_or_default().into_bytes())))
}

/// Every file under `folder`, as names with "/" separators
fn files_in(folder: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = format!("{prefix}{}", e.file_name().to_string_lossy());
            let path = e.path();
            if path.is_dir() {
                stack.push((path, format!("{name}/")));
            } else {
                out.push((name, path));
            }
        }
    }
    out
}

#[derive(Deserialize)]
struct SigFile {
    author: String,
    key: String,
    files: BTreeMap<String, String>,
    sig: String,
}

/// Check an installed (or unpacked) plugin folder against its signature
pub fn verify_folder(folder: &Path, user_trusted: &[AuthorKey]) -> Trust {
    let Ok(text) = fs::read_to_string(folder.join(SIG_FILE)) else { return Trust::Unsigned };
    let bad = |why: &str| Trust::Bad { why: why.to_string() };
    let Ok(s) = serde_json::from_str::<SigFile>(&text) else { return bad("Its signature can't be read.") };
    let Ok(key) = B64.decode(&s.key) else { return bad("Its signature can't be read.") };
    let Ok(sig) = B64.decode(&s.sig) else { return bad("Its signature can't be read.") };
    if UnparsedPublicKey::new(&ED25519, &key).verify(&message(&s.author, &s.key, &s.files), &sig).is_err() {
        return bad("Its signature doesn't match. It may have been changed after it was signed.");
    }
    let on_disk: Vec<(String, PathBuf)> = files_in(folder).into_iter().filter(|(n, _)| n != SIG_FILE).collect();
    if on_disk.len() != s.files.len() {
        return bad("It has files that weren't signed, or signed files are missing.");
    }
    for (name, path) in &on_disk {
        let matches = s.files.get(name).is_some_and(|want| fs::read(path).is_ok_and(|b| &crate::backend::util::sha256_hex(&b) == want));
        if !matches {
            return bad("Its files were changed after it was signed.");
        }
    }
    let author = s.author.chars().take(40).collect::<String>();
    if BUILT_IN.iter().any(|(_, k)| *k == s.key) {
        Trust::Trusted { author }
    } else if user_trusted.iter().any(|a| a.key == s.key) {
        Trust::User { author }
    } else {
        Trust::Untrusted { author }
    }
}

/// The signature file that goes up beside a release's installer: `<installer name>.sig`
pub fn release_sig_name(installer: &str) -> String {
    format!("{installer}.sig")
}

/// Sign an app installer for a release with this PC's author key (the same signature.json
/// format as plugins, listing the one file). Writes `<installer>.sig` beside it.
pub fn sign_release(root: &Path, installer: &Path) -> Result<(AuthorKey, PathBuf), String> {
    let name = installer.file_name().and_then(|n| n.to_str()).ok_or("Name the installer file.")?.to_string();
    let bytes = fs::read(installer).map_err(|e| format!("Could not read {}: {e}", installer.display()))?;
    let (me, body) = sign(root, &[(name.clone(), bytes)])?.ok_or_else(|| format!("There is no author key in {}.", author_dir(root).display()))?;
    let out = installer.with_file_name(release_sig_name(&name));
    fs::write(&out, body).map_err(|e| format!("Could not write {}: {e}", out.display()))?;
    Ok((me, out))
}

/// Check a release installer's signature: it must be signed by one of `trusted` (public keys)
/// and list exactly this installer with this SHA-256. Returns the author's name.
pub fn verify_release_with(sig: &[u8], installer: &str, sha256_hex: &str, trusted: &[&str]) -> Result<String, String> {
    let bad = || "The update's signature doesn't match, so it wasn't kept.".to_string();
    let s: SigFile = serde_json::from_slice(sig).map_err(|_| "The update's signature can't be read, so it wasn't kept.".to_string())?;
    if !trusted.contains(&s.key.as_str()) {
        return Err("The update isn't signed by MinnionDeck, so it wasn't kept.".into());
    }
    let key = B64.decode(&s.key).map_err(|_| bad())?;
    let signature = B64.decode(&s.sig).map_err(|_| bad())?;
    UnparsedPublicKey::new(&ED25519, &key).verify(&message(&s.author, &s.key, &s.files), &signature).map_err(|_| bad())?;
    if s.files.len() != 1 || s.files.get(installer).is_none_or(|h| !h.eq_ignore_ascii_case(sha256_hex)) {
        return Err(bad());
    }
    Ok(s.author.chars().take(40).collect())
}

/// `verify_release_with` the built-in authors' keys: only they sign MinnionDeck's own updates.
pub fn verify_release(sig: &[u8], installer: &str, sha256_hex: &str) -> Result<String, String> {
    let keys: Vec<&str> = BUILT_IN.iter().map(|(_, k)| *k).collect();
    verify_release_with(sig, installer, sha256_hex, &keys)
}

/// Where this PC's author key is: the app's own folder, or the Minniondeck app data folder the
/// plugin tools have always used.
pub fn author_root(app_root: &Path) -> PathBuf {
    let appdata = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("com.minnion.deck"));
    [Some(app_root.to_path_buf()), appdata].into_iter().flatten().find(|r| author(r).is_some()).unwrap_or_else(|| app_root.to_path_buf())
}

/// Read a .mdkey file someone sent
pub fn read_key_file(path: &Path) -> Result<AuthorKey, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    let k: AuthorKey = serde_json::from_str(&text).map_err(|_| "That isn't a Minniondeck author key (.mdkey) file.".to_string())?;
    let name = k.name.trim().chars().take(40).collect::<String>();
    if name.is_empty() || !valid_key(&k.key) {
        return Err("That author key file is damaged.".into());
    }
    Ok(AuthorKey { name, key: k.key })
}

/// Lock bytes to this Windows account (DPAPI)
#[cfg(windows)]
pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(&input, windows::core::PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).map_err(|e| format!("Windows couldn't protect the key: {e}"))?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        free_blob(out.pbData);
        Ok(bytes)
    }
}

/// Unlock bytes locked by `protect` (only works for the same Windows account)
#[cfg(windows)]
pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData};
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .map_err(|_| "The author key belongs to another Windows account and can't be used here.".to_string())?;
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        free_blob(out.pbData);
        Ok(bytes)
    }
}

#[cfg(windows)]
unsafe fn free_blob(p: *mut u8) {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    if !p.is_null() {
        let _ = unsafe { LocalFree(Some(HLOCAL(p as *mut core::ffi::c_void))) };
    }
}

#[cfg(not(windows))]
pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(data.to_vec())
}
#[cfg(not(windows))]
pub fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    Ok(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_authors_have_good_keys() {
        for (name, key) in BUILT_IN {
            assert!(valid_key(key), "{name}'s key isn't a 32-byte Ed25519 key");
        }
        let minnion = BUILT_IN.iter().find(|(n, _)| *n == "minnion").expect("minnion is built in");
        assert_eq!(fingerprint(minnion.1), "0cc7 5fff 43c1 1a23");
    }

    #[test]
    fn sign_then_verify_and_catch_changes() {
        let root = std::env::temp_dir().join(format!("md-sign-test-{}", crate::backend::util::now_ms()));
        let plugin = root.join("plugin");
        fs::create_dir_all(plugin.join("icons")).unwrap();
        let files = vec![("manifest.json".to_string(), b"{}".to_vec()), ("x.dll".to_string(), b"dll".to_vec()), ("icons/a.png".to_string(), b"png".to_vec())];
        for (n, b) in &files {
            fs::write(plugin.join(n), b).unwrap();
        }
        // No key yet: not signed
        assert!(sign(&root, &files).unwrap().is_none());
        assert_eq!(verify_folder(&plugin, &[]), Trust::Unsigned);
        let mdkey = make_key(&root, "Test Author", &root).unwrap();
        assert!(make_key(&root, "Test Author", &root).unwrap_err().contains("already has"));
        let me = read_key_file(&mdkey).unwrap();
        let (_, sig) = sign(&root, &files).unwrap().unwrap();
        fs::write(plugin.join(SIG_FILE), &sig).unwrap();
        assert_eq!(verify_folder(&plugin, &[]), Trust::Untrusted { author: "Test Author".into() });
        assert_eq!(verify_folder(&plugin, std::slice::from_ref(&me)), Trust::User { author: "Test Author".into() });
        // A changed file, or an extra one, is caught
        fs::write(plugin.join("x.dll"), b"evil").unwrap();
        assert!(matches!(verify_folder(&plugin, &[me.clone()]), Trust::Bad { .. }));
        fs::write(plugin.join("x.dll"), b"dll").unwrap();
        fs::write(plugin.join("extra.dll"), b"x").unwrap();
        assert!(matches!(verify_folder(&plugin, &[me]), Trust::Bad { .. }));
        assert_eq!(fingerprint("abc").len(), 19);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_release_installer_is_signed_and_only_its_own_signature_passes() {
        let root = std::env::temp_dir().join(format!("md-release-sign-{}", crate::backend::util::now_ms()));
        fs::create_dir_all(&root).unwrap();
        let exe = root.join("MinnionDeck-0.2.0-x64-setup.exe");
        fs::write(&exe, b"installer bytes").unwrap();
        assert!(sign_release(&root, &exe).unwrap_err().contains("no author key"));
        let me = read_key_file(&make_key(&root, "Test Author", &root).unwrap()).unwrap();
        let (_, sig_file) = sign_release(&root, &exe).unwrap();
        assert_eq!(sig_file, root.join("MinnionDeck-0.2.0-x64-setup.exe.sig"));
        let sig = fs::read(&sig_file).unwrap();
        let sha = crate::backend::util::sha256_hex(b"installer bytes");
        let name = "MinnionDeck-0.2.0-x64-setup.exe";
        assert_eq!(verify_release_with(&sig, name, &sha, &[me.key.as_str()]).unwrap(), "Test Author");
        // not a built-in author: MinnionDeck's own updates refuse it
        assert!(verify_release(&sig, name, &sha).unwrap_err().contains("isn't signed by MinnionDeck"));
        // another file, other bytes, or a changed signature are refused
        assert!(verify_release_with(&sig, "Other-setup.exe", &sha, &[me.key.as_str()]).is_err());
        assert!(verify_release_with(&sig, name, &crate::backend::util::sha256_hex(b"evil"), &[me.key.as_str()]).is_err());
        let tampered = String::from_utf8(sig.clone()).unwrap().replace("Test Author", "Someone Else");
        assert!(verify_release_with(tampered.as_bytes(), name, &sha, &[me.key.as_str()]).is_err());
        assert!(verify_release_with(b"not json", name, &sha, &[me.key.as_str()]).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
