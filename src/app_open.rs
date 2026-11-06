//! Opening things: a .mdplugin (the install question), a .mdprofile (added as a profile), a .mdkey (a
//! plugin author to trust) and minniondeck://install links (the Marketplace page of that plugin).
//! They come from Settings, from the command line when the app starts, or from a second launch that
//! hands them over (backend/single.rs).
use std::path::PathBuf;

use crate::app::{App, View};
use crate::backend::plugins::{self, PackInfo};
use crate::backend::{dialogs, market, profile_io};

/// The question asked before a plugin file is installed.
pub struct InstallAsk {
    pub path: PathBuf,
    pub info: PackInfo,
    pub opened: Option<f64>,
    pub closing: Option<f64>,
    pub busy: bool,
}

/// The plugin id of a minniondeck://install?id=…&v=… link (browsers may add a slash, or encode it).
pub fn link_plugin(link: &str) -> Option<String> {
    let link = link.trim();
    let rest = link.get(..11).filter(|p| p.eq_ignore_ascii_case("minniondeck")).and(link.get(11..))?.strip_prefix(':')?.trim_start_matches('/');
    let (what, query) = rest.split_once('?')?;
    if !what.trim_end_matches('/').eq_ignore_ascii_case("install") {
        return None;
    }
    query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == "id")
        .map(|(_, v)| v.replace("%5F", "_").replace("%5f", "_"))
        .filter(|id| crate::backend::plugins::safe_name(id))
}

impl App {
    /// Opens what the command line or a second launch gave: a file or a link.
    pub fn open_arg(&mut self, arg: &str, now: f64) {
        let a = arg.trim().trim_matches('"');
        if a.is_empty() {
            return;
        }
        let lower = a.to_ascii_lowercase();
        if lower.starts_with("minniondeck:") {
            return self.open_link(a, now);
        }
        let path = PathBuf::from(a);
        if lower.ends_with(".mdplugin") {
            self.inspect_plugin(path);
        } else if lower.ends_with(".mdprofile") {
            self.import_file(path);
        } else if lower.ends_with(".mdkey") {
            self.add_author_file(path);
        } else {
            self.say("Minniondeck can't open that kind of file", now);
        }
    }

    /// A minniondeck://install link: that plugin's page in the Marketplace, where Install is.
    fn open_link(&mut self, link: &str, now: f64) {
        let Some(id) = link_plugin(link) else { return self.say("That isn't a Minniondeck install link", now) };
        self.pending_view = Some(View::Marketplace);
        match market::plugins().iter().position(|p| p.id == id) {
            Some(index) => self.market_detail = Some(crate::views::marketplace::Detail { index, opened: None, closing: None, scroll: 0.0 }),
            None => {
                self.start_market_refresh(true);
                self.say("That plugin isn't on the Marketplace list yet. Try Check now in a moment.", now);
            }
        }
    }

    // ----- a plugin file -----

    /// Install a plugin file…: choose one, then look inside it.
    pub fn start_plugin_pick(&mut self) {
        let (tx, ctx) = (self.io.tx.clone(), self.egui.clone());
        std::thread::spawn(move || {
            if let Some(path) = dialogs::open_file("Install a plugin", "Minniondeck plugin", "mdplugin") {
                let _ = tx.send(crate::app_io::IoMsg::PluginInspected(inspect(path)));
                ctx.request_repaint();
            }
        });
    }

    fn inspect_plugin(&mut self, path: PathBuf) {
        let (tx, ctx) = (self.io.tx.clone(), self.egui.clone());
        std::thread::spawn(move || {
            let _ = tx.send(crate::app_io::IoMsg::PluginInspected(inspect(path)));
            ctx.request_repaint();
        });
    }

    /// The question's Install button: installs the file and starts the plugin.
    pub fn start_plugin_install(&mut self, now: f64) {
        let Some(ask) = self.install_ask.as_mut() else { return };
        if ask.busy {
            return;
        }
        ask.busy = true;
        let (path, tx, ctx) = (ask.path.clone(), self.io.tx.clone(), self.egui.clone());
        self.say("Installing…", now);
        std::thread::spawn(move || {
            let _ = tx.send(crate::app_io::IoMsg::PluginInstalled(plugins::install_from_file(&path)));
            ctx.request_repaint();
        });
    }

    // ----- an author's key -----

    /// Trust an author…: choose a .mdkey file.
    pub fn start_author_pick(&mut self) {
        let (tx, ctx) = (self.io.tx.clone(), self.egui.clone());
        std::thread::spawn(move || {
            if let Some(path) = dialogs::open_file("Trust a plugin author", "Minniondeck author key", "mdkey") {
                let _ = tx.send(crate::app_io::IoMsg::AuthorAdded(plugins::add_author(&path)));
                ctx.request_repaint();
            }
        });
    }

    fn add_author_file(&mut self, path: PathBuf) {
        let (tx, ctx) = (self.io.tx.clone(), self.egui.clone());
        std::thread::spawn(move || {
            let _ = tx.send(crate::app_io::IoMsg::AuthorAdded(plugins::add_author(&path)));
            ctx.request_repaint();
        });
    }

    // ----- a profile file -----

    /// Adds a .mdprofile as a new profile (the same as Import…, without asking which file).
    pub fn import_file(&mut self, path: PathBuf) {
        let (tx, media, ctx) = (self.io.tx.clone(), self.lib.paths.media.clone(), self.egui.clone());
        std::thread::spawn(move || {
            let _ = tx.send(crate::app_io::IoMsg::Imported(profile_io::import_from(&media, &path)));
            ctx.request_repaint();
        });
    }
}

fn inspect(path: PathBuf) -> Result<(PathBuf, PackInfo), String> {
    plugins::inspect(&path).map(|info| (path, info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_links_are_read_in_the_forms_browsers_send() {
        assert_eq!(link_plugin("minniondeck://install?id=minnion_spotify"), Some("minnion_spotify".into()));
        assert_eq!(link_plugin("minniondeck://install/?id=minnion_spotify&v=0.2.0"), Some("minnion_spotify".into()));
        assert_eq!(link_plugin("MinnionDeck://Install?v=0.26.6&id=minnioncore_deck"), Some("minnioncore_deck".into()));
        assert_eq!(link_plugin(" minniondeck:install?id=minnion%5Flights "), Some("minnion_lights".into()));
        // other links and unsafe names are not install links
        assert_eq!(link_plugin("minniondeck://pair?v=1&h=192.168.1.20"), None);
        assert_eq!(link_plugin("https://install?id=x"), None);
        assert_eq!(link_plugin("minniondeck://install"), None);
        assert_eq!(link_plugin("minniondeck://install?id=..%2F..%2Fwindows"), None);
        assert_eq!(link_plugin("minniondeck://install?id=.hidden"), None);
        assert_eq!(link_plugin("minniondeckx://install?id=ok"), None);
    }
}
