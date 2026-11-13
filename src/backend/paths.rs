//! Where this app keeps its data: in the folder its exe is in, so the folder can be moved or
//! copied as a whole.
//!
//!   <exe folder>\
//!     Minniondeck.exe
//!     library\library.mdsave profiles, folders, pages, buttons, designs, GIFs (sealed: see vault.rs)
//!     library\media\         pictures and GIF files
//!     backups\               copies of the library
//!     settings.mdsave        the Settings page (sealed)
//!
//! Run from cargo (target\debug or target\release) it uses build\ in the project instead,
//! where build.ps1 puts the exe, so both find the same data.
//!
//! The Minniondeck app's own folder (%APPDATA%\com.minnion.deck) is only ever read, once, to
//! copy your profiles on the first start. Nothing is ever written there.
use std::path::PathBuf;

#[derive(Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub library: PathBuf,
    pub media: PathBuf,
    pub backups: PathBuf,
}

impl Paths {
    pub fn find() -> Paths {
        let dir = std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).unwrap_or_else(|| PathBuf::from("."));
        let root = dev_root(&dir).unwrap_or(dir);
        let library = root.join("library");
        Paths { media: library.join("media"), backups: root.join("backups"), library, root }
    }

    pub fn make_folders(&self) -> Result<(), String> {
        for d in [&self.root, &self.library, &self.media, &self.backups] {
            std::fs::create_dir_all(d).map_err(|e| format!("Could not make {}: {e}", d.display()))?;
        }
        Ok(())
    }

    pub fn library_file(&self) -> PathBuf {
        self.library.join("library.mdsave")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.mdsave")
    }

    /// Where an earlier version kept them, as plain files.
    pub fn legacy_library_file(&self) -> PathBuf {
        self.library.join("library.json")
    }

    pub fn legacy_settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    /// Seals the library, the settings and the pictures an earlier version left as plain files.
    /// Safe to call any time: it changes nothing that is already sealed. Returns a message when
    /// something couldn't be changed over (it is then left as it was).
    pub fn migrate_legacy(&self) -> Option<String> {
        use super::package::Kind;
        let mut problems = vec![];
        for (plain, sealed) in [(self.legacy_library_file(), self.library_file()), (self.legacy_settings_file(), self.settings_file())] {
            if let Err(e) = super::vault::migrate(&plain, &sealed, Kind::Save) {
                problems.push(e);
            }
        }
        problems.extend(super::media::seal_folder(&self.media));
        (!problems.is_empty()).then(|| problems.join(" "))
    }
}

/// Run from cargo (target\debug or target\release in a debug-capable build): the project's build\
/// folder. Only debug builds and tests know where the project is; a release build never has the
/// developer's folder written into it.
#[cfg(any(debug_assertions, test))]
fn dev_root(exe_dir: &std::path::Path) -> Option<PathBuf> {
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    exe_dir.starts_with(project.join("target")).then(|| project.join("build"))
}

#[cfg(not(any(debug_assertions, test)))]
fn dev_root(_: &std::path::Path) -> Option<PathBuf> {
    None
}

/// The Minniondeck app's data folder. Read only.
pub fn old_app_root() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("APPDATA")?).join("com.minnion.deck");
    p.is_dir().then_some(p)
}
