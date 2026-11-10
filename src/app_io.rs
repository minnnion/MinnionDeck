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
    /// Plugin `plugin`'s ready-made profile was downloaded: the profile, its file's SHA-256 and
    /// the plugin's name. `add`: the Marketplace's Add profile (a new profile); else it is checked
    /// against the profiles that follow the plugin (`adopt`: also ones added before they were linked).
    PluginProfile { plugin: String, add: bool, adopt: bool, result: Result<(Value, String, String), String> },
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

    /// Downloads the ready-made profile that comes with plugin `id` and adds it as a new profile
    /// that follows the plugin (it is updated with it).
    pub fn start_market_profile(&mut self, id: &str, now: f64) {
        if !self.tray.on() {
            return self.say("Profiles come from the marketplace when the app is running for real", now);
        }
        self.say("Getting the profile…", now);
        self.fetch_plugin_profile(id, true, false);
    }

    fn fetch_plugin_profile(&mut self, id: &str, add: bool, adopt: bool) {
        let (tx, root, media, ctx, id) = (self.io.tx.clone(), self.lib.paths.root.clone(), self.lib.paths.media.clone(), self.egui.clone(), id.to_string());
        std::thread::spawn(move || {
            let result = crate::backend::market::download_plugin_profile(&root, &id).and_then(|d| {
                let out = profile_io::import_from(&media, &d.path);
                let _ = std::fs::remove_file(&d.path);
                out.map(|b| (b, d.sha, d.plugin_name))
            });
            let _ = tx.send(IoMsg::PluginProfile { plugin: id, add, adopt, result });
            ctx.request_repaint();
        });
    }

    /// After plugin `id` was updated (or the marketplace's list was read): if its ready-made
    /// profile changed, the profiles that follow it are brought up to date (see
    /// Library::plugin_profile_changed). `adopt`: also look at profiles added before they were
    /// linked to their plugin.
    pub fn start_profile_sync(&mut self, id: &str, adopt: bool) {
        self.profile_sync(id, adopt, false);
    }

    /// `fresh`: right after the plugin was updated, the cached list may be old, so its profile is
    /// always fetched and compared.
    pub fn profile_sync(&mut self, id: &str, adopt: bool, fresh: bool) {
        if !self.tray.on() {
            return;
        }
        let plugin_name = crate::backend::market::plugins().iter().find(|p| p.id == id).map(|p| p.name.to_string()).unwrap_or_default();
        let follow = self.lib.plugin_profiles(id, &plugin_name, adopt);
        if follow.is_empty() {
            return;
        }
        // nothing to do when every one already matches the version the list has
        let listed = crate::backend::market::profile_sha(id);
        let behind = follow.iter().any(|p| {
            let have = self.lib.profile_sha(p);
            have.is_empty() || listed.as_deref() != Some(have.as_str())
        });
        if fresh || (behind && listed.is_some()) {
            self.fetch_plugin_profile(id, false, adopt);
        }
    }

    /// Once the marketplace's list is known: every installed plugin's profiles are checked.
    pub fn check_plugin_profiles(&mut self, adopt: bool) {
        let ids: Vec<String> = crate::backend::market::plugins().iter().map(|p| p.id.to_string()).collect();
        for id in ids {
            self.start_profile_sync(&id, adopt);
        }
    }

    /// The link to the website runs from the start, so an open website tab can find the app.
    pub fn init_web_link(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel::<crate::backend::websync::Done>();
        let (out, ctx) = (self.io.tx.clone(), self.egui.clone());
        std::thread::spawn(move || {
            while let Ok((k, r, t, look)) = rx.recv() {
                let _ = out.send(IoMsg::MediaImported(k, r, t, look));
                ctx.request_repaint();
            }
        });
        let c = self.egui.clone();
        let _ = crate::backend::websync::init(&self.lib.paths.media, tx, move || c.request_repaint());
    }

    /// Edit image in the Edit button window: opens the website on this look's picture (or GIF); its
    /// Sync button sends the edited one back here (backend/websync.rs).
    pub fn start_web_edit(&mut self, token: u64, look: usize, want: Option<crate::backend::media::Kind>, now: f64) {
        let Some(l) = self.editor.as_ref().and_then(|e| e.looks.get(look)).cloned() else { return };
        let name = self.editor.as_ref().map(|e| e.name.trim().to_string()).unwrap_or_default();
        use crate::backend::media::{self, Kind};
        // a GIF look edits its GIF; anything else edits its picture. A look with no picture file sends
        // what it looks like (its colour and icon), so there is always something to start from.
        let mut baked = false;
        let gif_look = match want {
            Some(k) => k == Kind::Gif,
            None => !l.media.gif.is_empty() && l.media.pic.is_empty(),
        };
        let (kind, current) = if gif_look {
            let bytes = media::gif(&l.media.gif).and_then(|g| match g.src {
                Some(media::GifSource::File(p)) => media::read_bytes(&p),
                Some(media::GifSource::Drawn { size, fps, frames }) => drawn_gif(size, fps, &frames),
                None => None,
            });
            (Kind::Gif, bytes)
        } else {
            let file = media::picture(&l.media.pic).and_then(|p| p.file).and_then(|f| media::read_bytes(&f));
            let bytes = file.or_else(|| {
                baked = true;
                look_png(&self.theme(), &l.colour, &l.icon)
            });
            (Kind::Picture, bytes)
        };
        *lock_baked() = if baked { Some((token, look)) } else { None };
        let (tx, rx) = std::sync::mpsc::channel::<crate::backend::websync::Done>();
        let (out, ctx) = (self.io.tx.clone(), self.egui.clone());
        std::thread::spawn(move || {
            while let Ok((k, r, t, look)) = rx.recv() {
                let _ = out.send(IoMsg::MediaImported(k, r, t, look));
                ctx.request_repaint();
            }
        });
        let c = self.egui.clone();
        // a picture look also goes in layers: the button, then its icon on its own layer
        let layers = if kind == Kind::Picture { look_layers(&self.theme(), &l.colour, &l.icon, media::picture(&l.media.pic).and_then(|p| p.file).and_then(|f| media::read_bytes(&f))) } else { vec![] };
        if layers.iter().any(|x| x.role == "icon") {
            // the icon comes back inside the picture: the button mustn't draw it a second time
            *lock_baked() = Some((token, look));
        }
        match crate::backend::websync::start(&self.lib.paths.media, tx, move || c.request_repaint(), token, look, kind, &name, current, layers) {
            Ok(url) => {
                self.say("Opening the editor in your browser. Press Sync there when it's done.", now);
                // an editor tab that is open takes it; otherwise a new tab
                std::thread::spawn(move || {
                    if !crate::backend::websync::send_to_open_tab(&url) {
                        let _ = crate::backend::actions::open_url(&url);
                    }
                });
            }
            Err(e) => self.say(&e, now),
        }
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
                // a profile with choices asks which pages first
                IoMsg::Imported(Ok(bundle)) if !crate::backend::library::choices_of(&bundle).is_empty() => {
                    self.choose_pages = Some(crate::views::choose_pages::ChoosePages::new(bundle, None));
                }
                IoMsg::PluginProfile { plugin, add: true, result: Ok((bundle, sha, _)), .. } if !crate::backend::library::choices_of(&bundle).is_empty() => {
                    self.choose_pages = Some(crate::views::choose_pages::ChoosePages::new(bundle, Some((plugin, sha))));
                }
                IoMsg::Imported(Ok(bundle)) => match self.lib.import_bundle(&mut self.sample, &bundle) {
                    Ok((name, pages, buttons)) => {
                        self.fresh_profile = Some((self.sample.profiles.len().saturating_sub(1), now));
                        self.pending_view = Some(View::Profiles);
                        self.say(&format!("Imported {name}: {pages} {}, {buttons} {}", if pages == 1 { "page" } else { "pages" }, if buttons == 1 { "button" } else { "buttons" }), now);
                    }
                    Err(e) => self.say(&format!("Could not import that file: {e}"), now),
                },
                IoMsg::Imported(Err(e)) => self.say(&format!("Could not import that file: {e}"), now),
                IoMsg::PluginProfile { plugin, add: true, result: Ok((bundle, sha, _)), .. } => match self.lib.import_bundle(&mut self.sample, &bundle) {
                    Ok((name, pages, buttons)) => {
                        self.lib.link_last_profile(&plugin, &sha);
                        self.fresh_profile = Some((self.sample.profiles.len().saturating_sub(1), now));
                        self.pending_view = Some(View::Profiles);
                        self.say(&format!("Added {name}: {pages} {}, {buttons} {}. It updates with the plugin", if pages == 1 { "page" } else { "pages" }, if buttons == 1 { "button" } else { "buttons" }), now);
                    }
                    Err(e) => self.say(&format!("Could not add the profile: {e}"), now),
                },
                IoMsg::PluginProfile { add: true, result: Err(e), .. } => self.say(&format!("Could not get the profile: {e}"), now),
                IoMsg::PluginProfile { plugin, add: false, adopt, result: Ok((bundle, sha, plugin_name)) } => {
                    use crate::backend::library::PluginProfile;
                    let linked_before: Vec<String> = self.lib.plugin_profiles(&plugin, &plugin_name, false);
                    for r in self.lib.plugin_profile_changed(&mut self.sample, &plugin, &plugin_name, adopt, &bundle, &sha) {
                        match r {
                            PluginProfile::Same => {}
                            PluginProfile::Updated(n) => self.say(&format!("Your {n} profile was updated with the {plugin_name} plugin. A backup was kept"), now),
                            PluginProfile::Edited(id, n) => {
                                if !self.profile_offers.iter().any(|o| o.profile_id == id) {
                                    let linked = linked_before.contains(&id);
                                    self.profile_offers.push(crate::views::profile_update::ProfileOffer {
                                        profile_id: id,
                                        profile_name: n,
                                        plugin_id: plugin.clone(),
                                        plugin_name: plugin_name.clone(),
                                        bundle: bundle.clone(),
                                        sha: sha.clone(),
                                        linked,
                                        opened: None,
                                    });
                                }
                            }
                        }
                    }
                }
                // a check in the background that couldn't reach the marketplace: try again later
                IoMsg::PluginProfile { add: false, result: Err(_), .. } => {}
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
                                if kind == crate::backend::media::Kind::Picture && *lock_baked() == Some((token, look)) {
                                    l.icon = "none".into();
                                }
                            }
                            ed.bump = Some(now);
                            ed.state_at = Some(now);
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
                    let ok = r.is_ok();
                    match r {
                        Ok(n) if by_hand => self.say(&format!("The Marketplace has {n} {}", if n == 1 { "plugin" } else { "plugins" }), now),
                        Err(e) if by_hand => self.say(&e, now),
                        _ => {}
                    }
                    // a plugin's profile may have a new version
                    if ok {
                        self.check_plugin_profiles(false);
                    }
                }
            }
        }
    }
}

/// The Edit window and look whose picture was made from its colour and icon (for Edit image).
static WEB_BAKED: std::sync::Mutex<Option<(u64, usize)>> = std::sync::Mutex::new(None);

fn lock_baked() -> std::sync::MutexGuard<'static, Option<(u64, usize)>> {
    WEB_BAKED.lock().unwrap_or_else(|e| e.into_inner())
}

/// A look as the deck shows it, without its name: its colour, and its icon in the middle. 256 x 256.
fn look_png(th: &crate::ui::theme::Theme, colour: &str, icon: &str) -> Option<Vec<u8>> {
    const S: u32 = 256;
    let bg = if colour == "tile" || colour.is_empty() { th.tile } else { th.accent(colour) };
    let fg = if colour == "tile" || colour.is_empty() { th.ink } else { th.ink_on(colour) };
    let mut img = image::RgbaImage::from_pixel(S, S, image::Rgba([bg.r(), bg.g(), bg.b(), 255]));
    if let Some(ic) = crate::backend::phoneweb::pics::icon_png(icon, [fg.r(), fg.g(), fg.b(), 255]).and_then(|b| image::load_from_memory(&b).ok()) {
        let ic = ic.to_rgba8();
        // whole-pixel scale, about half the button, as on the deck
        let k = ((S as f32 * 0.5) / ic.width().max(ic.height()) as f32).floor().max(1.0) as u32;
        let (w, h) = (ic.width() * k, ic.height() * k);
        let big = image::imageops::resize(&ic, w, h, image::imageops::FilterType::Nearest);
        image::imageops::overlay(&mut img, &big, ((S - w) / 2) as i64, ((S - h) / 2) as i64);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

/// A look in layers for the editor, bottom first: the button (its picture, or its colour) and its
/// icon alone on a clear layer, placed as on the deck. Each 256 x 256.
fn look_layers(th: &crate::ui::theme::Theme, colour: &str, icon: &str, picture: Option<Vec<u8>>) -> Vec<crate::backend::websync::Layer> {
    use crate::backend::websync::Layer;
    const S: u32 = 256;
    let png = |img: image::RgbaImage| {
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok().map(|_| out)
    };
    let mut layers = vec![];
    let plain = colour == "tile" || colour.is_empty();
    let bg = if plain { th.tile } else { th.accent(colour) };
    // the bottom: the look's own picture, filling the button, or its colour
    let picture = picture.and_then(|b| image::load_from_memory(&b).ok()).map(|p| p.resize_to_fill(S, S, image::imageops::FilterType::Nearest).to_rgba8());
    match picture {
        Some(p) => {
            if let Some(b) = png(p) {
                layers.push(Layer { name: "Picture".into(), role: "picture", icon: String::new(), png: b });
            }
        }
        None => {
            if let Some(b) = png(image::RgbaImage::from_pixel(S, S, image::Rgba([bg.r(), bg.g(), bg.b(), 255]))) {
                layers.push(Layer { name: "Button".into(), role: "background", icon: String::new(), png: b });
            }
        }
    }
    // the icon, on its own
    if !icon.is_empty() && icon != "none" {
        let fg = if plain { th.ink } else { th.ink_on(colour) };
        if let Some(ic) = crate::backend::phoneweb::pics::icon_png(icon, [fg.r(), fg.g(), fg.b(), 255]).and_then(|b| image::load_from_memory(&b).ok()) {
            let ic = ic.to_rgba8();
            let k = ((S as f32 * 0.5) / ic.width().max(ic.height()) as f32).floor().max(1.0) as u32;
            let (w, h) = (ic.width() * k, ic.height() * k);
            let big = image::imageops::resize(&ic, w, h, image::imageops::FilterType::Nearest);
            let mut layer = image::RgbaImage::new(S, S);
            image::imageops::overlay(&mut layer, &big, ((S - w) / 2) as i64, ((S - h) / 2) as i64);
            if let Some(b) = png(layer) {
                layers.push(Layer { name: "Icon".into(), role: "icon", icon: icon.to_string(), png: b });
            }
        }
    }
    layers
}

/// A GIF drawn in the GIF maker, as a GIF file (each pixel 8 x 8, so it isn't tiny).
fn drawn_gif(size: usize, fps: f32, frames: &[Vec<Option<[u8; 4]>>]) -> Option<Vec<u8>> {
    use image::codecs::gif::{GifEncoder, Repeat};
    if size == 0 || frames.is_empty() {
        return None;
    }
    let k = (256 / size).max(1) as u32;
    let n = size as u32 * k;
    let delay = image::Delay::from_numer_denom_ms((1000.0 / fps.max(1.0)) as u32, 1);
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).ok()?;
        for f in frames {
            let mut img = image::RgbaImage::new(n, n);
            for (i, px) in f.iter().enumerate().take(size * size) {
                let c = px.unwrap_or([0, 0, 0, 0]);
                let (x0, y0) = ((i % size) as u32 * k, (i / size) as u32 * k);
                for y in 0..k {
                    for x in 0..k {
                        img.put_pixel(x0 + x, y0 + y, image::Rgba(c));
                    }
                }
            }
            enc.encode_frame(image::Frame::from_parts(img, 0, 0, delay)).ok()?;
        }
    }
    Some(out)
}

#[cfg(test)]
mod web_edit_tests {
    use super::*;

    #[test]
    fn a_look_without_a_picture_is_sent_as_it_looks() {
        let th = crate::ui::theme::Theme::dark();
        let png = look_png(&th, "sky", "play").expect("a picture");
        let img = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (256, 256));
        // its colour at the corner, its icon in the middle
        let corner = img.get_pixel(2, 2).0;
        assert_eq!(&corner[..3], &[th.sky.r(), th.sky.g(), th.sky.b()]);
        assert!((0..256).any(|x| img.get_pixel(x, 128).0 != corner), "the icon is drawn");
        // in layers: the button's colour, then the icon alone on a clear layer
        let ls = look_layers(&th, "sky", "play", None);
        assert_eq!(ls.iter().map(|l| l.role).collect::<Vec<_>>(), vec!["background", "icon"]);
        let icon = image::load_from_memory(&ls[1].png).unwrap().to_rgba8();
        assert_eq!(icon.get_pixel(2, 2).0[3], 0, "clear around the icon");
        assert!(icon.pixels().any(|p| p.0[3] == 255), "the icon is on it");
        // a drawn GIF becomes a GIF file
        let frames = vec![vec![Some([255, 0, 0, 255]); 16 * 16], vec![None; 16 * 16]];
        let gif = drawn_gif(16, 8.0, &frames).expect("a gif");
        assert!(gif.starts_with(b"GIF8"));
    }
}
