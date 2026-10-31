//! Minniondeck licensing: the public stub.
//!
//! The official build runs its buttons only with a licence signed by the Minniondeck licensing key
//! and bound to one PC. That code isn't in the public source: this stub stands in for it, with the
//! same functions, and always reports "unlicensed". A build from the public repo opens, but its
//! buttons stay locked. (Publishing: this file is copied over src-tauri/src/license.rs.)

use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};

/// Whether this run is licensed: never, in a build from the public source
pub fn is_licensed() -> bool {
    false
}

/// What the app makes of this PC's licence. Sent to the window for the activation screen. The stub
/// only ever says Unlicensed or Bad; the rest are kept so the app's code matches the official build.
#[allow(dead_code)]
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum Status {
    /// A valid licence for this PC
    Licensed { name: String },
    /// No licence stored yet
    Unlicensed,
    /// A licence is stored, but it was made for another PC
    WrongMachine,
    /// A licence is stored, but it's damaged or not signed by the licensing key
    Bad { why: String },
}
#[allow(dead_code)]
impl Status {
    pub fn ok(&self) -> bool {
        matches!(self, Status::Licensed { .. })
    }
}

/// This PC's machine code: 32 hex characters, shown so a licence can be asked for
pub fn machine_code() -> String {
    let mut h = Sha256::new();
    h.update(b"minniondeck-machine-v1\n");
    h.update(std::env::var("COMPUTERNAME").unwrap_or_default().as_bytes());
    h.finalize().iter().take(16).map(|b| format!("{b:02x}")).collect()
}

/// The machine code grouped for display: XXXXXXXX-XXXXXXXX-XXXXXXXX-XXXXXXXX
pub fn machine_code_display() -> String {
    let c = machine_code();
    c.as_bytes().chunks(8).map(|g| String::from_utf8_lossy(g).to_uppercase()).collect::<Vec<_>>().join("-")
}

pub fn status(_root: &Path) -> Status {
    Status::Unlicensed
}

pub fn init(root: &Path) -> Status {
    status(root)
}

pub fn activate(_root: &Path, _text: &str) -> Status {
    Status::Bad { why: "this build can't be activated: install the official release".into() }
}

const OWNER_ONLY: &str = "Licensing keys are only in the official build.";

pub fn owner_init(_root: &Path) -> Result<String, String> {
    Err(OWNER_ONLY.into())
}

pub fn owner_issue(_root: &Path, _code: &str, _name: &str, _out_dir: &Path) -> Result<String, String> {
    Err(OWNER_ONLY.into())
}

// ----- Window commands -----

#[tauri::command]
pub fn license_status(store: tauri::State<crate::storage::Store>) -> Status {
    status(&store.paths.root)
}

#[tauri::command]
pub fn license_machine_code() -> String {
    machine_code_display()
}

#[tauri::command]
pub fn license_activate(store: tauri::State<crate::storage::Store>, key: String) -> Status {
    activate(&store.paths.root, &key)
}
