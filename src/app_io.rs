//! Importing and exporting profiles (.mdprofile files). The file dialogs and the packing run on
//! their own threads; the result comes back here and is applied to the library on the next frame.
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::Value;

use crate::app::{App, View};
use crate::backend::{dialogs, profile_io};

pub enum IoMsg {
    /// A profile file was opened and unpacked (its pictures are already in the media folder)
    Imported(Result<Value, String>),
    Exported(Result<String, String>),
    ExportedAll(Result<(String, usize), String>),
    BackedUp(Result<Option<String>, String>),
    Restored(Result<(), String>),
    /// The marketplace's list was read again: how many plugins (`by hand`: say so)
    MarketRefreshed(Result<usize, String>, bool),
    /// A plugin file was looked into: the path and what it is
    PluginInspected(Result<(std::path::PathBuf, crate::backend::plugins::PackInfo), String>),
    /// A plugin file was installed: its name
    PluginInstalled(Result<String, String>),
    /// An author's key was trusted: their name
    AuthorAdded(Result<String, String>),
    /// A picture or GIF copied into library\media: its kind, its entry, and the editor and look it is for
    MediaImported(crate::backend::media::Kind, Result<serde_json::Value, String>, u64, usize),
}

pub struct Io {
    pub tx: Sender<IoMsg>,
    rx: Receiver<IoMsg>,
}

impl Io {
    pub fn new() -> Io {
        let (tx, rx) = channel();
        Io { tx, rx }
    }
}

impl App {
    /// Import…: choose a .mdprofile file and add it as a new profile.
    pub fn start_import(&mut self) {
        let (tx, media, ctx) = (self.io.tx.clone(), self.lib.paths.media.clone(), self.egui.clone());
        std::thread::spawn(move || {
            if let Some(path) = dialogs::open_file("Import a profile", "Minniondeck profile", profile_io::EXT) {
                let _ = tx.send(IoMsg::Imported(profile_io::import_from(&media, &path)));
                ctx.request_repaint();
            }
        });
    }

    /// Export: save profile `i` as a .mdprofile file.
    pub fn start_export(&mut self, i: usize, now: f64) {
        let Some(p) = self.sample.profiles.get(i).cloned() else { return };
        let bundle = match self.lib.bundle_for(&mut self.sample, &p.id) {
            Ok(b) => b,
            Err(e) => return self.say(&format!("Could not export {}: {e}", p.name), now),
        };
        let (tx, media, ctx) = (self.io.tx.clone(), self.lib.paths.media.clone(), self.egui.clone());
        std::thread::spawn(move || {
            if let Some(path) = dialogs::save_file("Export profile", "Minniondeck profile", profile_io::EXT, &profile_io::default_file_name(&p.name)) {
                let _ = tx.send(IoMsg::Exported(profile_io::export_to(&media, &bundle, &path)));
                ctx.request_repaint();
            }
        });
    }

    /// Export every profile into one folder, one file each. Existing files are never replaced.
    pub fn start_export_all(&mut self, now: f64) {
        let mut bundles = vec![];
        for p in self.sample.profiles.clone() {
            match self.lib.bundle_for(&mut self.sample, &p.id) {
                Ok(b) => bundles.push((p.name, b)),
                Err(e) => return self.say(&format!("Could not export {}: {e}", p.name), now),
            }
        }
        let (tx, media, ctx) = (self.io.tx.clone(), self.lib.paths.media.clone(), self.egui.clone());
        std::thread::spawn(move || {
            if let Some(folder) = dialogs::pick_folder("Choose a folder for the profiles") {
                let r = (|| {
                    for (name, bundle) in &bundles {
                        profile_io::export_to(&media, bundle, &profile_io::free_path(&folder, name))?;
                    }
                    Ok((folder.display().to_string(), bundles.len()))
                })();
                let _ = tx.send(IoMsg::ExportedAll(r));
                ctx.request_repaint();
            }
        });
    }

    /// Reads the marketplace's list again (not in screenshot mode).
    pub fn start_market_refresh(&mut self, by_hand: bool) {
        if !self.tray.on() || self.market_fetching {
            return;
        }
        self.market_fetching = true;
        let (tx, root, ctx) = (self.io.tx.clone(), self.lib.paths.root.clone(), self.egui.clone());
        std::thread::spawn(move || {
            let _ = tx.send(IoMsg::MarketRefreshed(crate::backend::market::refresh(&root), by_hand));
            ctx.request_repaint();
        });
    }

    /// Downloads the ready-made profile that comes with plugin `id` and adds it as a new profile.
    pub fn start_market_profile(&mut self, id: &str, now: f64) {
        if !self.tray.on() {
            return self.say("Profiles come from the marketplace when the app is running for real", now);
        }
        let (tx, root, media, ctx, id) = (self.io.tx.clone(), self.lib.paths.root.clone(), self.lib.paths.media.clone(), self.egui.clone(), id.to_string());
        self.say("Getting the profile…", now);
        std::thread::spawn(move || {
            let r = crate::backend::market::download_profile(&root, &id).and_then(|path| {
                let out = profile_io::import_from(&media, &path);
                let _ = std::fs::remove_file(&path);
                out
            });
            let _ = tx.send(IoMsg::Imported(r));
            ctx.request_repaint();
        });
    }

    /// Import… in the Edit button window: choose a picture or GIF and put it on look `look`.
    pub fn start_media_import(&mut self, kind: crate::backend::media::Kind, token: u64, look: usize) {
        use crate::backend::media::Kind;
        let (tx, media, ctx) = (self.io.tx.clone(), self.lib.paths.media.clone(), self.egui.clone());
        std::thread::spawn(move || {
            let picked = match kind {
                Kind::Picture => dialogs::open_file_any("Import a picture", "Pictures (PNG, GIF, JPEG, WebP)", &["png", "gif", "jpg", "jpeg", "webp"]),
                Kind::Gif => dialogs::open_file("Import a GIF", "GIF files", "gif"),
            };
            if let Some(path) = picked {
                let _ = tx.send(IoMsg::MediaImported(kind, crate::backend::media::import(&media, &path, kind), token, look));
                ctx.request_repaint();
            }
        });
    }

    /// Reads the list of backups again (Settings › Backup).
    pub fn refresh_backups(&mut self, now: f64) {
        self.backups = crate::backend::backups::list(&self.lib.paths);
        self.backups_at = now;
    }

    /// Back up now.
    pub fn start_backup(&mut self) {
        let (tx, paths, ctx) = (self.io.tx.clone(), self.lib.paths.clone(), self.egui.clone());
        std::thread::spawn(move || {
            let _ = tx.send(IoMsg::BackedUp(crate::backend::backups::create(&paths, crate::backend::backups::Reason::ByHand)));
            ctx.request_repaint();
        });
    }

    /// Put a backup back. What is there now is backed up first, so this can be undone.
    pub fn start_restore(&mut self, name: String) {
        let (tx, paths, ctx) = (self.io.tx.clone(), self.lib.paths.clone(), self.egui.clone());
        std::thread::spawn(move || {
            let _ = tx.send(IoMsg::Restored(crate::backend::backups::restore(&paths, &name)));
            ctx.request_repaint();
        });
    }

    /// Every frame: applies what the import and export threads finished.
    pub fn pump_io(&mut self, now: f64) {
        if self.view == View::Marketplace && !self.market_fetching && now - self.market_checked > 600.0 {
            self.market_checked = now;
            self.start_market_refresh(false);
        }
        while let Ok(m) = self.io.rx.try_recv() {
            match m {
                IoMsg::Imported(Ok(bundle)) => match self.lib.import_bundle(&mut self.sample, &bundle) {
                    Ok((name, pages, buttons)) => {
                        self.fresh_profile = Some((self.sample.profiles.len().saturating_sub(1), now));
                        self.pending_view = Some(View::Profiles);
                        self.say(&format!("Imported {name}: {pages} {}, {buttons} {}", if pages == 1 { "page" } else { "pages" }, if buttons == 1 { "button" } else { "buttons" }), now);
                    }
                    Err(e) => self.say(&format!("Could not import that file: {e}"), now),
                },
                IoMsg::Imported(Err(e)) => self.say(&format!("Could not import that file: {e}"), now),
                IoMsg::Exported(Ok(path)) => self.say(&format!("Exported to {path}"), now),
                IoMsg::Exported(Err(e)) => self.say(&format!("Could not export: {e}"), now),
                IoMsg::ExportedAll(Ok((folder, n))) => self.say(&format!("Exported {n} {} to {folder}", if n == 1 { "profile" } else { "profiles" }), now),
                IoMsg::ExportedAll(Err(e)) => self.say(&format!("Could not export the profiles: {e}"), now),
                IoMsg::BackedUp(Ok(Some(_))) => {
                    self.refresh_backups(now);
                    self.say("Backed up your profiles", now);
                }
                IoMsg::BackedUp(Ok(None)) => self.say("Nothing to back up yet", now),
                IoMsg::BackedUp(Err(e)) => self.say(&format!("Could not back up: {e}"), now),
                // the library was put back on disk: read it again, so every page starts from it
                IoMsg::Restored(Ok(())) => {
                    let (lib, sample, _) = crate::backend::library::Library::open();
                    self.lib = lib;
                    self.sample = sample;
                    self.selected_tile = None;
                    self.history.clear();
                    self.editor = None;
                    self.menu = None;
                    self.refresh_backups(now);
                    self.say("Backup restored. What was here before was backed up first", now);
                }
                IoMsg::Restored(Err(e)) => self.say(&format!("Could not restore: {e}"), now),
                IoMsg::MediaImported(kind, Ok(entry), token, look) => {
                    let name = entry.get("name").and_then(|n| n.as_str()).unwrap_or("It").to_string();
                    let id = self.lib.add_media(kind, entry);
                    // onto the look it was imported for, if that Edit window is still open
                    match self.editor.as_mut().filter(|e| e.token == token) {
                        Some(ed) => {
                            if let Some(l) = ed.looks.get_mut(look) {
                                match kind {
                                    crate::backend::media::Kind::Picture => l.media.pic = id,
                                    crate::backend::media::Kind::Gif => l.media.gif = id,
                                }
                            }
                            ed.bump = Some(now);
                            self.say(&format!("{name} added and put on the button"), now);
                        }
                        None => self.say(&format!("{name} added"), now),
                    }
                }
                IoMsg::MediaImported(_, Err(e), _, _) => self.say(&format!("Could not import it: {e}"), now),
                IoMsg::PluginInspected(Ok((path, info))) => {
                    self.install_ask = Some(crate::app_open::InstallAsk { path, info, opened: None, closing: None, busy: false });
                }
                IoMsg::PluginInspected(Err(e)) => self.say(&format!("Could not open that plugin: {e}"), now),
                IoMsg::PluginInstalled(r) => {
                    self.install_ask = None;
                    match r {
                        Ok(name) => self.say(&format!("{name} installed. Its buttons are in Edit button."), now),
                        Err(e) => self.say(&format!("Could not install it: {e}"), now),
                    }
                }
                IoMsg::AuthorAdded(Ok(name)) => self.say(&format!("{name} is trusted now. Their plugins can run."), now),
                IoMsg::AuthorAdded(Err(e)) => self.say(&format!("Could not add that author: {e}"), now),
                IoMsg::MarketRefreshed(r, by_hand) => {
                    self.market_fetching = false;
                    self.market_checked = now;
                    match r {
                        Ok(n) if by_hand => self.say(&format!("The Marketplace has {n} {}", if n == 1 { "plugin" } else { "plugins" }), now),
                        Err(e) if by_hand => self.say(&e, now),
                        _ => {}
                    }
                }
            }
        }
    }
}
