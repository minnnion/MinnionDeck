//! Small helpers the plugin code shares.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Writes a file by writing a temporary one next to it and renaming it over the real one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let fail = |e: std::io::Error| format!("Could not save {}: {e}", path.display());
    {
        let mut f = std::fs::File::create(&tmp).map_err(fail)?;
        f.write_all(bytes).map_err(fail)?;
        f.sync_all().map_err(fail)?;
    }
    std::fs::rename(&tmp, path).map_err(fail)
}

/// Reads a file, trying again for about two seconds when Windows says it is in use (an antivirus
/// scan, a backup program, OneDrive): only "not found" or an error that stays is given up on.
pub fn read_retrying(path: &Path) -> std::io::Result<Vec<u8>> {
    read_retrying_with(path, |p: &Path| std::fs::read(p))
}

pub(crate) fn read_retrying_with(path: &Path, mut read: impl FnMut(&Path) -> std::io::Result<Vec<u8>>) -> std::io::Result<Vec<u8>> {
    let mut last = None;
    for i in 0..6u64 {
        match read(path) {
            Ok(b) => return Ok(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(e),
            Err(e) => {
                last = Some(e);
                if i < 5 {
                    std::thread::sleep(std::time::Duration::from_millis(80 * (i + 1)));
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::ErrorKind::Other.into()))
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// A plain file name with no folders in it, safe to join onto a directory
pub fn safe_name(name: &str) -> Option<&str> {
    let ok = !name.is_empty() && name.len() <= 200 && !name.starts_with('.') && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok.then_some(name)
}
