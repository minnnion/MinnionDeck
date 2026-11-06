//! The window: title bar, the rail on the left and the five views.
use crate::backend::library::Library;
use crate::backend::model::Sample;
use crate::ui::draw::Cv;
use crate::ui::theme::Theme;
use crate::views;
use egui::{Color32, Context, CursorIcon, Rect, ResizeDirection, Sense, ViewportCommand, pos2, vec2};

pub const TITLE_H: f32 = 36.0;
pub const RAIL_W: f32 = 64.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Deck,
    Profiles,
    Phones,
    Marketplace,
    Settings,
}

const VIEWS: [(View, &str, &str); 5] = [
    (View::Deck, "Deck", "grid"),
    (View::Profiles, "Profiles", "people"),
    (View::Phones, "Phones", "phone"),
    (View::Marketplace, "Marketplace", "shop"),
    (View::Settings, "Settings", "gear"),
];

pub struct App {
    pub rail_hop: Option<(usize, f64)>,
    pub toast: Option<(String, f64)>,
    pub side_open: bool,
    pub page_at: f64,
    pub ripples: Vec<(usize, f64)>,
    pub fresh_page: Option<(usize, f64)>,
    pub fresh_tile: Option<(usize, f64)>,
    pub flip: Option<(usize, f64)>,
    /// The right-click menu on the deck, and the button copied with Copy.
    pub menu: Option<crate::views::deck::CtxMenu>,
    pub clipboard: Option<crate::backend::model::Tile>,
    /// A button being dragged, and buttons gliding to a new spot (index, from col, from row, start).
    pub drag: Option<crate::views::deck::Drag>,
    pub slides: Vec<(usize, f32, f32, f64)>,
    /// A button being resized by its corner handle (Edit layout).
    pub resize: Option<crate::views::deck::Resize>,
    /// A grid size change gliding: the old cols and rows, and when it started; `grid_hold`
    /// freezes it part way for screenshots (--grid-change C R P).
    pub grid_anim: Option<(usize, usize, f64)>,
    pub grid_hold: Option<f32>,
    /// The Edit profile window, and a profile card that was just added (pops in).
    pub prof_edit: Option<crate::views::profiles::ProfEdit>,
    pub fresh_profile: Option<(usize, f64)>,
    /// Phones page: preview in landscape, the page it shows (None = the deck's page), when it
    /// last turned or changed page (and which way), a button pressed on it, a Forget waiting to be confirmed.
    pub phone_land: bool,
    pub phone_page: Option<usize>,
    pub phone_turn: f64,
    /// Screenshot helper (--turn P): the turn held at P percent.
    pub phone_hold: Option<f32>,
    pub phone_flip: (f64, f32),
    pub phone_press: Option<(usize, f64)>,
    pub phone_forget: Option<usize>,
    /// The baked background picture, and whether this frame drew on it.
    pub bg: crate::ui::layers::Layer,
    pub bg_baked: bool,
    pub editor: Option<crate::views::editor::Editor>,
    pub view_t: f32,
    pub view_started: f64,
    pub tile_anim: Option<(usize, f64)>,
    pub menu_at: f64,
    pub dark: bool,
    pub view: View,
    pub sample: Sample,
    pub scroll: [f32; 5],
    pub content_h: [f32; 5],
    pub settings_tab: usize,
    /// Settings: when the tab last changed (its slide-in), the search words, the setting that
    /// just changed (its "Saved" tag), when Check for updates was pressed, and text settings.
    pub settings_at: f64,
    pub settings_query: String,
    pub settings_saved: Option<(&'static str, f64)>,
    pub settings_check: f64,
    pub settings_text: std::collections::HashMap<&'static str, String>,
    pub edit_layout: bool,
    pub selected_tile: Option<usize>,
    pub prof_menu: bool,
    pub choice: [usize; 8],
    pub st: std::collections::HashMap<&'static str, usize>,
    pub market_cat: usize,
    pub market_query: String,
    /// Marketplace: the tab (Discover, Installed, Updates), the installed plugins and their
    /// versions (a sample until the backend), the install running and the ones waiting, a Remove
    /// waiting to be confirmed, when updates were last checked, and the details window.
    pub market_tab: usize,
    pub market_installed: Vec<(&'static str, String)>,
    pub market_busy: Option<(usize, f64)>,
    pub market_queue: Vec<usize>,
    /// A real install running (backend/market.rs).
    pub market_job: Option<crate::views::marketplace::MarketJob>,
    /// The marketplace's list is being read now.
    pub market_fetching: bool,
    pub market_remove: Option<usize>,
    pub market_checked: f64,
    pub market_detail: Option<crate::views::marketplace::Detail>,
    pub grid_menu: bool,
    pub auto_switch: bool,
    pub shot: Option<String>,
    /// The screenshot was asked for (it arrives a frame later)
    pub shot_sent: bool,
    pub frames: u32,
    pub pending_view: Option<View>,
    pub wb_hover: [bool; 3],
    /// The library behind the window (library.json), the pages visited (for Back), a button
    /// pressed this frame, and what the buttons that ran said (from their threads).
    pub lib: Library,
    pub history: Vec<usize>,
    pub pending_press: Option<usize>,
    pub ran_tx: std::sync::mpsc::Sender<Result<String, String>>,
    pub ran_rx: std::sync::mpsc::Receiver<Result<String, String>>,
    /// Plugins: what they show on buttons, the list Settings shows (app_plugins.rs).
    pub plug: crate::app_plugins::PluginGlue,
    /// A plugin's window, open from Settings › Plugins.
    pub plug_dialog: Option<crate::views::plugin_dialog::PlugDialog>,
    /// Watches which app is in front, for switching profiles by itself (app_switch.rs).
    pub front: crate::app_switch::FrontWatch,
    /// Profile import and export (app_io.rs) and a handle to wake the window from threads.
    pub io: crate::app_io::Io,
    pub egui: egui::Context,
    /// The Settings page in settings.json.
    pub settings: crate::backend::settings::SettingsFile,
    /// The backups listed in Settings › Backup, when they were read, and the one asked "Sure?".
    pub backups: Vec<crate::backend::backups::BackupInfo>,
    pub backups_at: f64,
    pub backup_confirm: Option<usize>,
    /// The tray icon, Keep running and Start with Windows (app_tray.rs).
    pub tray: crate::app_tray::TrayGlue,
    /// Checking for and installing Minniondeck updates (app_updates.rs).
    pub upd: crate::app_updates::UpdateGlue,
    /// The phone page: phones that are in, pairing (app_phones.rs).
    pub phones: crate::app_phones::PhoneGlue,
    /// A plugin file waiting for Install, and files or links the app was started with (app_open.rs).
    pub install_ask: Option<crate::app_open::InstallAsk>,
    pub pending_open: Vec<String>,
}

pub const APP_ICON: [&str; 16] = [
    "......rr........", "......rr........", ".......o........", ".....oooooo.....",
    "...oommmmmmoo...", "..ommmmmmmmmmo..", "..omwwmmmmwwmo..", ".ommwpmmmmwpmmo.",
    ".ommmmmmmmmmmmo.", ".ommmmmoommmmdo.", ".ommmmmmmmmmmdo.", "..ommmmmmmmmdo..",
    "...oommmmmmoo...", ".....oooooo.....", ".....oo..oo.....", ".....oo..oo.....",
];

/// The 16 x 16 minnion head at any size, nearest-neighbour like the web sprite.
pub fn draw_minnion(cv: &mut Cv, x: f32, y: f32, size: f32) {
    let th = cv.th;
    let k = size / 16.0;
    for (j, row) in APP_ICON.iter().enumerate() {
        for (i, ch) in row.chars().enumerate() {
            let c = match ch {
                'o' => Color32::from_rgb(0xe4, 0xea, 0xf6),
                'm' => th.mint,
                'd' => th.mint_dark,
                'w' => Color32::WHITE,
                'p' => Color32::from_rgb(0x1e, 0x24, 0x33),
                'r' => th.coral,
                _ => continue,
            };
            let (x0, x1) = ((x + i as f32 * k).round(), (x + (i + 1) as f32 * k).round());
            let (y0, y1) = ((y + j as f32 * k).round(), (y + (j + 1) as f32 * k).round());
            cv.fill(x0, y0, x1 - x0, y1 - y0, c);
        }
    }
}

impl App {
    pub fn new(light: bool, view: Option<String>, shot: Option<String>, egui_ctx: egui::Context) -> App {
        // the editor's actions from the plugins installed now, before the library is read (its
        // buttons' pictures may be a plugin's own)
        if std::env::args().all(|a| a != "--shot" && a != "--selftest") {
            crate::backend::catalog::rebuild(&crate::backend::paths::Paths::find().root);
        }
        let (lib, sample, note) = Library::open();
        // plugins start with the window (not for a screenshot, which must not run anything)
        // (a screenshot of a plugin's window needs its plugin running, so --plugin-window starts them)
        let run_plugins = shot.is_none() || std::env::args().any(|a| a == "--plugin-window");
        let (plug, plug_note) = crate::app_plugins::PluginGlue::start(&lib.paths.root, &egui_ctx, run_plugins);
        let note = note.or(plug_note);
        let shot_run = shot.is_none() && !std::env::args().any(|a| a == "--selftest");
        let lib_root = lib.paths.root.clone();
        let settings = crate::backend::settings::SettingsFile::open(&lib.paths, shot_run);
        let (ran_tx, ran_rx) = std::sync::mpsc::channel();
        let v = match view.as_deref() {
            Some("profiles") => View::Profiles,
            Some("phones") => View::Phones,
            Some("marketplace") => View::Marketplace,
            Some("settings") => View::Settings,
            _ => View::Deck,
        };
        let mut app = App {
            rail_hop: None,
            toast: None,
            side_open: true,
            page_at: -10.0,
            ripples: vec![],
            fresh_page: None,
            fresh_tile: None,
            flip: None,
            menu: None,
            clipboard: None,
            drag: None,
            slides: vec![],
            resize: None,
            grid_anim: None,
            grid_hold: None,
            prof_edit: None,
            fresh_profile: None,
            phone_land: true,
            phone_page: None,
            phone_turn: -10.0,
            phone_hold: None,
            phone_flip: (-10.0, 0.0),
            phone_press: None,
            phone_forget: None,
            bg: crate::ui::layers::Layer::new(),
            bg_baked: false,
            editor: None,
            view_t: 9.0,
            view_started: -10.0,
            tile_anim: None,
            menu_at: 0.0,
            dark: !light,
            view: v,
            sample,
            scroll: [0.0; 5],
            content_h: [0.0; 5],
            settings_tab: 0,
            settings_at: -10.0,
            settings_query: String::new(),
            settings_saved: None,
            settings_check: -1000.0,
            settings_text: std::collections::HashMap::new(),
            edit_layout: false,
            selected_tile: None,
            prof_menu: false,
            choice: [0; 8],
            st: std::collections::HashMap::new(),
            market_cat: 0,
            market_query: String::new(),
            market_tab: 0,
            // installed plugins come with the plugin step; none yet
            market_installed: vec![],
            market_busy: None,
            market_queue: vec![],
            market_job: None,
            market_fetching: false,
            market_remove: None,
            market_checked: -1000.0,
            market_detail: None,
            grid_menu: false,
            auto_switch: true,
            shot,
            shot_sent: false,
            frames: 0,
            pending_view: None,
            wb_hover: [false; 3],
            lib,
            history: vec![],
            pending_press: None,
            ran_tx,
            ran_rx,
            plug,
            plug_dialog: None,
            front: crate::app_switch::FrontWatch::start(&egui_ctx, shot_run),
            io: crate::app_io::Io::new(),
            egui: egui_ctx.clone(),
            settings,
            backups: vec![],
            backups_at: -100.0,
            backup_confirm: None,
            tray: crate::app_tray::TrayGlue::start(&egui_ctx, shot_run),
            upd: crate::app_updates::UpdateGlue::new(),
            phones: crate::app_phones::PhoneGlue::start(&lib_root, &egui_ctx, shot_run),
            install_ask: None,
            // what the app was started with: a double-clicked file, or a link
            pending_open: if shot_run { std::env::args().skip(1).filter(|a| !a.starts_with("--")).collect() } else { vec![] },
        };
        // the Settings page as it was left (not for a screenshot)
        app.settings.apply(&mut app.st);
        if !light {
            if let Some(t) = app.st.get("theme") {
                app.dark = *t != 1;
            }
        }
        if let Some(on) = app.st.get("autoSwitch") {
            app.auto_switch = *on == 1;
        }
        app.settings.prev_auto = app.auto_switch;
        app.init_autostart();
        if shot_run {
            let _ = crate::backend::market::load_cache(&app.lib.paths.root);
        }
        if shot_run {
            let _ = crate::backend::updates::Source::load(&app.lib.paths.root);
        }
        if let Some(n) = note {
            app.toast = Some((n, 0.5));
        }
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "--menu") {
            app.selected_tile = Some(5);
            app.menu = Some(crate::views::deck::CtxMenu { tile: Some(5), cell: (1, 1), x: 520.0, y: 330.0, at: -10.0, sub: args.iter().any(|a| a == "--sub") });
        }
        if args.iter().any(|a| a == "--slot-menu") {
            app.clipboard = app.sample.tiles.first().cloned();
            app.menu = Some(crate::views::deck::CtxMenu { tile: None, cell: (3, 2), x: 860.0, y: 560.0, at: -10.0, sub: false });
        }
        // --drag C R: button 1 held over cell (C, R); --drag-page N: held over page row N
        let num = |flag: &str, k: usize| args.iter().position(|a| a == flag).and_then(|i| args.get(i + k)).and_then(|v| v.parse::<usize>().ok());
        let (dc, dp) = (num("--drag", 1).zip(num("--drag", 2)), num("--drag-page", 1));
        if (dc.is_some() || dp.is_some()) && app.sample.tiles.len() > 1 {
            let t = &app.sample.tiles[1];
            app.selected_tile = Some(1);
            app.drag = Some(crate::views::deck::Drag {
                tile: 1,
                grab: (60.0, 60.0),
                pos: (0.0, 0.0),
                src: (t.col, t.row),
                over: None,
                over_tile: None,
                size: (t.w, t.h),
                blocked: false,
                page: None,
                at: -10.0,
                fake: true,
                fake_cell: dc,
                fake_page: dp,
            });
        }
        // --resize T W H: button T stretched to W x H cells in Edit layout
        if let (Some(ti), Some(w), Some(h)) = (num("--resize", 1), num("--resize", 2), num("--resize", 3)) {
            if ti < app.sample.tiles.len() {
                app.edit_layout = true;
                app.selected_tile = Some(ti);
                app.resize = Some(crate::views::deck::Resize { tile: ti, w, h, ok: true, fake: Some((w, h)) });
            }
        }
        // --big: a few buttons bigger than one cell, to see how they look and drag
        if args.iter().any(|a| a == "--big") && app.sample.tiles.len() > 10 {
            app.sample.tiles[10].w = 2;
        }
        // --grid-change C R P: changing from 5 x 3 to C x R, held at P percent of the way
        if let (Some(c), Some(r), Some(p)) = (num("--grid-change", 1), num("--grid-change", 2), num("--grid-change", 3)) {
            app.grid_anim = Some((app.sample.cols, app.sample.rows, 0.0));
            app.grid_hold = Some(p as f32 / 100.0);
            app.sample.cols = c;
            app.sample.rows = r;
        }
        // --prof-edit N: the Edit profile window open on profile N
        if let Some(n) = num("--prof-edit", 1) {
            app.prof_edit = Some(crate::views::profiles::ProfEdit { index: n, opened: Some(-10.0), closing: None, confirm_delete: args.iter().any(|a| a == "--confirm"), focus_name: false });
        }
        if let Some(p) = num("--turn", 1) {
            app.phone_hold = Some(p as f32 / 100.0);
        }
        if args.iter().any(|a| a == "--portrait") {
            app.phone_land = false;
        }
        // --market-tab N (0 Discover, 1 Installed, 2 Updates), --market-detail <plugin id>
        // --market-busy <plugin id>: that plugin's install is running
        if let Some(i) = args.iter().position(|a| a == "--market-busy") {
            if let Some(k) = args.get(i + 1).and_then(|id| crate::backend::market::plugins().iter().position(|p| p.id == id)) {
                app.market_busy = Some((k, -0.6));
            }
        }
        // --settings-tab N, --settings-q <words>, --saved <setting key>
        if let Some(t) = num("--settings-tab", 1) {
            app.settings_tab = t;
        }
        if let Some(i) = args.iter().position(|a| a == "--settings-q") {
            app.settings_query = args.get(i + 1).cloned().unwrap_or_default();
        }
        if let Some(i) = args.iter().position(|a| a == "--saved") {
            if let Some(k) = args.get(i + 1) {
                app.settings_saved = Some((Box::leak(k.clone().into_boxed_str()), 0.0));
            }
        }
        if let Some(t) = num("--market-tab", 1) {
            app.market_tab = t;
        }
        if let Some(i) = args.iter().position(|a| a == "--market-detail") {
            if let Some(k) = args.get(i + 1).and_then(|id| crate::backend::market::plugins().iter().position(|p| p.id == id)) {
                app.market_detail = Some(crate::views::marketplace::Detail { index: k, opened: Some(-10.0), closing: None, scroll: 0.0 });
            }
        }
        if args.iter().any(|a| a == "--grid-menu") {
            app.grid_menu = true;
            app.menu_at = -10.0;
        }
        if args.iter().any(|a| a == "--closed") {
            app.side_open = false;
        }
        if args.iter().any(|a| a == "--editor") {
            app.open_editor(None, Some((0, 0)));
            if let Some(ed) = app.editor.as_mut() {
                // screenshot helpers: scroll the form to the pictures, show the logos tab
                if args.iter().any(|a| a == "--actions") {
                    ed.scroll = 100.0;
                    if args.iter().any(|a| a == "--plugin") {
                        ed.source = 2;
                    }
                }
                if let Some(i) = args.iter().position(|a| a == "--chosen") {
                    if let Some(a) = args.get(i + 1).and_then(|id| crate::views::editor::action_by_id(id)) {
                        ed.choose_pub(a);
                        ed.scroll = 100.0;
                    }
                }
                if let Some(i) = args.iter().position(|a| a == "--scroll") {
                    ed.scroll = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                }
                if args.iter().any(|a| a == "--normal") {
                    ed.tab = 3;
                    ed.name = "Next song".into();
                    ed.looks[0].icon = "png:minnion_spotify/sp-next".into();
                    if let Some(i) = args.iter().position(|a| a == "--gridscroll") {
                        ed.grid_scroll = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                    }
                }
                if args.iter().any(|a| a == "--pictures") {
                    ed.scroll = 330.0;
                }
                if args.iter().any(|a| a == "--logos") {
                    ed.scroll = 330.0;
                    ed.tab = 2;
                    ed.name = "Spotify".into();
                    ed.looks[0].icon = "si:spotify".into();
                }
            }
        }
        app
    }

    pub fn open_editor(&mut self, index: Option<usize>, cell: Option<(usize, usize)>) {
        let tile = index.and_then(|i| self.sample.tiles.get(i)).cloned();
        self.editor = Some(crate::views::editor::Editor::new(tile.as_ref(), index, cell));
    }

    pub fn say(&mut self, msg: &str, now: f64) {
        self.toast = Some((msg.to_string(), now));
    }

    /// Shows page `i` (Back comes back here).
    pub fn go_page(&mut self, i: usize, now: f64) {
        if i < self.sample.pages.len() && !self.sample.pages[i].folder && i != self.sample.page {
            self.history.push(self.sample.page);
            if self.history.len() > 50 {
                self.history.remove(0);
            }
            self.sample.page = i;
            self.page_at = now;
            self.selected_tile = None;
        }
    }

    /// A button on the deck was pressed: Go to page and Back move the deck, everything else
    /// runs on its own thread so a Wait or a slow program never holds the window up.
    pub fn press_tile(&mut self, i: usize, now: f64) {
        let Some(t) = self.sample.tiles.get(i).cloned() else { return };
        match t.action.as_str() {
            "" => self.say("This button does nothing yet. Double-click it to set it up.", now),
            "page" => {
                let want = t.value("page").to_string();
                // a page by name, or a folder: its first page
                let pages = &self.sample.pages;
                let hit = pages.iter().position(|p| p.name == want || p.id == want).and_then(|k| {
                    if !pages[k].folder {
                        return Some(k);
                    }
                    let d = pages[k].depth;
                    pages.iter().enumerate().skip(k + 1).take_while(|(_, p)| p.depth > d).find(|(_, p)| !p.folder).map(|(j, _)| j)
                });
                match hit {
                    Some(k) => self.go_page(k, now),
                    None => self.say(&format!("There is no page called \"{want}\" any more"), now),
                }
            }
            "back" => match self.history.pop() {
                Some(k) if k < self.sample.pages.len() => {
                    self.sample.page = k;
                    self.page_at = now;
                    self.selected_tile = None;
                }
                _ => self.say("There is no page to go back to", now),
            },
            _ => {
                if self.lib.needs_trust() {
                    let name = self.sample.profiles.get(self.sample.active).map(|p| p.name.clone()).unwrap_or_default();
                    if !crate::backend::actions::allow_imported(&name) {
                        return;
                    }
                    self.lib.trust();
                }
                let steps = self.lib.steps_of(&t);
                let button = self.plugin_ctx(&t);
                let tx = self.ran_tx.clone();
                let label = if t.label.is_empty() { "The button".to_string() } else { t.label.clone() };
                std::thread::spawn(move || {
                    let r = crate::backend::actions::run_steps(&steps, &button).map_err(|e| format!("{label}: {e}"));
                    let _ = tx.send(r);
                });
            }
        }
    }

    pub fn theme(&self) -> Theme {
        if self.dark { Theme::dark() } else { Theme::light() }
    }

    fn view_index(v: View) -> usize {
        VIEWS.iter().position(|x| x.0 == v).unwrap()
    }

    pub fn crumb(&self) -> String {
        match self.view {
            View::Deck => {
                let p = &self.sample.profiles[self.sample.active].name;
                let page = &self.sample.pages[self.sample.page].name;
                format!("— {p} / {page}")
            }
            v => format!("— {}", VIEWS[Self::view_index(v)].1),
        }
    }

    fn titlebar(&mut self, ui: &mut egui::Ui, ctx: &Context, win: Rect) {
        let th = self.theme();
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "title");
        let w = win.width();
        if !self.bg_baked {
            cv.fill(0.0, 0.0, w, TITLE_H, th.shadow);
        }
        // logo
        let colour = |c: char| match c {
            'o' => Some(Color32::from_rgb(0xe4, 0xea, 0xf6)),
            'm' => Some(th.mint),
            'd' => Some(th.mint_dark),
            'w' => Some(Color32::WHITE),
            'p' => Some(Color32::from_rgb(0x1e, 0x24, 0x33)),
            'r' => Some(th.coral),
            _ => None,
        };
        for (y, row) in APP_ICON.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if let Some(c) = colour(ch) {
                    cv.fill(10.0 + x as f32, 7.0 + y as f32, 1.0, 1.0, c);
                }
            }
        }
        let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
        let tw = cv.text(36.0, 7.5, 21.0, 15.0, 700, light, "Minniondeck");
        if std::env::var("MD_MEASURE").is_ok() { for (sz,wt,t) in [(15.0,700u16,"Minniondeck"),(15.0,400,"— Settings"),(14.0,600,"Edit layout"),(14.0,400,"5 × 3 grid"),(14.0,600,"Show on phones"),(16.0,400,"My profile /"),(12.0,600,"Marketplace"),(12.0,600,"Profiles"),(18.0,700,"Pair a new phone"),(16.0,600,"+ New profile")] { eprintln!("{t}: {} (size {sz} w{wt})", cv.measure(sz,wt,t)); } }
        let crumb = self.crumb();
        let max = w - 36.0 - tw - 10.0 - 10.0 - 3.0 * 30.0 - 2.0 * 10.0 - 10.0;
        cv.text_fit(36.0 + tw + 10.0, 7.5, 21.0, 15.0, 400, light, &crumb, max);

        // window buttons: minimise, maximise, close
        let labels = ["_", "□", "✕"];
        let hov = [light, light, th.coral];
        for i in 0..3 {
            let x = w - 10.0 - 30.0 * (3 - i) as f32 - 10.0 * (2 - i) as f32;
            let resp = cv.hit(&format!("wb{i}"), x, 6.0, 30.0, 24.0);
            let hot = resp.hovered();
            let border = if hot && i == 2 { th.coral } else { light };
            if hot {
                cv.fill(x, 6.0, 30.0, 24.0, hov[i]);
            }
            cv.border(x, 6.0, 30.0, 24.0, 3.0, border);
            let fg = if hot { if i == 2 { Color32::WHITE } else { th.shadow } } else { light };
            match i {
                0 => { cv.text_c(x, 30.0, 6.0 + (24.0 - 12.0) / 2.0, 12.0, 12.0, 700, fg, labels[i]); }
                1 => { cv.border(x + 11.0, 6.0 + 8.0, 8.0, 8.0, 1.0, fg); }
                _ => {
                    for k in 0..9 { cv.fill(x + 10.0 + k as f32, 6.0 + 7.0 + k as f32, 2.0, 1.0, fg); cv.fill(x + 10.0 + (8 - k) as f32, 6.0 + 7.0 + k as f32, 2.0, 1.0, fg); }
                }
            }
            if resp.clicked() {
                let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                match i {
                    0 => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                    1 => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maxed)),
                    _ => self.request_close(&ctx),
                }
            }
        }
        // drag region
        let drag = Rect::from_min_size(win.min, vec2(w - 130.0, TITLE_H));
        let r = cv.ui.interact(drag, egui::Id::new("titledrag"), Sense::click_and_drag());
        if r.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if r.double_clicked() {
            let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maxed));
        }
    }

    /// The rail: icons only. The selected square slides to the view you pick, the icon hops,
    /// and the name of the view shows beside the rail while you point at it.
    fn rail(&mut self, ui: &mut egui::Ui, win: Rect) {
        let th = self.theme();
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let h = win.height() - TITLE_H;
        let origin = win.min + vec2(0.0, TITLE_H);
        let mut tip: Option<(f32, &str, f32)> = None;
        {
            let mut cv = Cv::new(ui, Rect::from_min_size(origin, vec2(RAIL_W, h)), origin.to_vec2(), th, "rail");
            if !self.bg_baked {
                cv.fill(0.0, 0.0, RAIL_W, h, th.tile);
                cv.fill(RAIL_W - 4.0, 0.0, 4.0, h, th.ink);
            }
            let size = 48.0;
            let bx = (RAIL_W - 4.0 - size) / 2.0;
            let slot_y = |i: usize, v: View| if v == View::Settings { h - 10.0 - size } else { 10.0 + i as f32 * (size + 8.0) };
            // the selected square glides to its view (0.2s)
            let sel_i = VIEWS.iter().position(|x| x.0 == self.view).unwrap_or(0);
            let target = slot_y(sel_i, self.view);
            let sy = ctx.animate_value_with_time(egui::Id::new("rail-sel"), target, 0.2);
            cv.shadow(bx, sy, size, size, 3.0, th.shadow);
            cv.fill(bx, sy, size, size, th.mint);
            cv.border(bx, sy, size, size, 3.0, th.ink);
            for (i, (v, label, icon)) in VIEWS.iter().enumerate() {
                let by = slot_y(i, *v);
                let selected = self.view == *v;
                let resp = cv.hit(&format!("rail{i}"), bx, by, size, size);
                let hot = resp.hovered();
                let on_sel = (sy - by).abs() < size / 2.0;
                if hot && !on_sel {
                    cv.fill(bx, by, size, size, th.hover);
                }
                // hop when chosen (the web app's hop keyframes: up 8px and back, three steps)
                let mut dy = 0.0;
                if let Some((k, t0)) = self.rail_hop {
                    if k == i {
                        let p = crate::ui::widgets::steps(((now - t0) / 0.3) as f32, 3.0);
                        dy = -8.0 * (1.0 - (2.0 * p - 1.0).abs());
                        if p < 1.0 {
                            ctx.request_repaint();
                        }
                    }
                }
                let fg = if on_sel { th.on_accent } else { th.ink };
                cv.glyph(icon, bx + 12.0, by + 12.0 + dy, 24.0, fg);
                let tip_t = ctx.animate_bool_with_time(egui::Id::new(("rail-tip", i)), hot, 0.12);
                if tip_t > 0.0 {
                    tip = Some((by, label, tip_t));
                }
                if resp.clicked() && !selected {
                    self.pending_view = Some(*v);
                    self.rail_hop = Some((i, now));
                }
            }
        }
        // the name beside the rail, sliding out of it
        if let Some((y, label, t)) = tip {
            let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "railtip");
            let tw = cv.measure(14.0, 600, label) + 20.0;
            let x = RAIL_W + 4.0 + 6.0 * t;
            let ty = TITLE_H + y + 10.0;
            let a = t;
            cv.fill(x + 3.0, ty + 3.0, tw, 28.0, crate::ui::draw::with_alpha(th.shadow, a));
            cv.fill(x, ty, tw, 28.0, crate::ui::draw::with_alpha(th.panel, a));
            cv.border(x, ty, tw, 28.0, 3.0, crate::ui::draw::with_alpha(th.ink, a));
            cv.text(x + 10.0, ty + 4.2, 19.6, 14.0, 600, crate::ui::draw::with_alpha(th.ink, a), label);
        }
    }

    /// A short message at the bottom of the window: slides up, stays, fades away.
    fn toast(&mut self, ui: &mut egui::Ui, win: Rect) {
        let Some((msg, t0)) = self.toast.clone() else { return };
        let th = self.theme();
        let ctx = ui.ctx().clone();
        let t = (ctx.input(|i| i.time) - t0) as f32;
        if t > 2.6 {
            self.toast = None;
            return;
        }
        ctx.request_repaint();
        let inn = crate::ui::widgets::steps(t / 0.2, 4.0);
        let out = if t > 2.3 { 1.0 - (t - 2.3) / 0.3 } else { 1.0 };
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "toast");
        let w = cv.measure(15.0, 600, &msg) + 28.0;
        let x = RAIL_W + (win.width() - RAIL_W - w) / 2.0;
        let y = win.height() - 18.0 - 38.0 + 16.0 * (1.0 - inn);
        let a = out.clamp(0.0, 1.0) * inn.max(0.25);
        cv.fill(x + 4.0, y + 4.0, w, 38.0, crate::ui::draw::with_alpha(th.shadow, a));
        cv.fill(x, y, w, 38.0, crate::ui::draw::with_alpha(th.panel, a));
        cv.border(x, y, w, 38.0, 3.0, crate::ui::draw::with_alpha(th.ink, a));
        cv.text(x + 14.0, y + 8.5, 21.0, 15.0, 600, crate::ui::draw::with_alpha(th.ink, a), &msg);
    }

    fn resize_edges(&self, ui: &mut egui::Ui, ctx: &Context, win: Rect) {
        let maxed = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if maxed {
            return;
        }
        let e = 5.0;
        let zones = [
            (Rect::from_min_size(win.min, vec2(win.width(), e)), ResizeDirection::North, CursorIcon::ResizeVertical),
            (Rect::from_min_size(pos2(win.min.x, win.max.y - e), vec2(win.width(), e)), ResizeDirection::South, CursorIcon::ResizeVertical),
            (Rect::from_min_size(win.min, vec2(e, win.height())), ResizeDirection::West, CursorIcon::ResizeHorizontal),
            (Rect::from_min_size(pos2(win.max.x - e, win.min.y), vec2(e, win.height())), ResizeDirection::East, CursorIcon::ResizeHorizontal),
            (Rect::from_min_size(pos2(win.max.x - 12.0, win.max.y - 12.0), vec2(12.0, 12.0)), ResizeDirection::SouthEast, CursorIcon::ResizeNwSe),
        ];
        for (i, (r, dir, cursor)) in zones.iter().enumerate() {
            let resp = ui.interact(*r, egui::Id::new(("resize", i)), Sense::click_and_drag());
            if resp.hovered() {
                ctx.set_cursor_icon(*cursor);
            }
            if resp.drag_started() {
                ctx.send_viewport_cmd(ViewportCommand::BeginResize(*dir));
            }
        }
    }

}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.lib.save(&mut self.sample);
        self.settings.flush(&self.st);
        crate::backend::plugins::shutdown();
        crate::backend::tray::remove();
    }

    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        let c = self.theme().panel;
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let win = ctx.content_rect();
        let th = self.theme();
        ctx.set_visuals(if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() });

        let now = ctx.input(|i| i.time);
        if let Some(v) = self.pending_view.take() {
            if v != self.view {
                self.view_started = now;
            }
            self.view = v;
            self.prof_menu = false;
            self.grid_menu = false;
            self.menu = None;
        }

        let idx = Self::view_index(self.view);
        let body = Rect::from_min_max(win.min + vec2(RAIL_W, TITLE_H), win.max);

        // the background layer: one picture of everything that stands still, the controls go on
        // top. While the view slides in or the pages panel moves, the views draw it live instead.
        let side_t = ctx.animate_bool_with_time(egui::Id::new("side-open"), self.side_open, 0.2);
        let settled = (side_t == 0.0 || side_t == 1.0) && now - self.view_started >= 0.2;
        self.bg_baked = settled;
        if settled {
            let full = crate::views::deck::side_width(win.width());
            let side = if self.view == View::Deck && side_t == 1.0 { full as u32 } else { 0 };
            let key = crate::ui::layers::Key {
                view: if self.view == View::Deck { 0 } else { 1 + idx },
                w: win.width().round() as u32,
                h: win.height().round() as u32,
                dark: self.dark,
                side,
                ppp_x100: (ctx.pixels_per_point() * 100.0).round() as u32,
            };
            let id = self.bg.get(&ctx, key, &th, RAIL_W, TITLE_H);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter().image(id, Rect::from_min_size(win.min, vec2(key.w as f32, key.h as f32)), uv, Color32::WHITE);
        }

        // mouse wheel scrolls the view under the pointer
        if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
            if body.contains(p) && self.market_detail.is_none() && self.prof_edit.is_none() {
                let d = ctx.input(|i| i.smooth_scroll_delta.y);
                let max = (self.content_h[idx] - body.height()).max(0.0);
                self.scroll[idx] = (self.scroll[idx] - d).clamp(0.0, max);
            }
        }

        // the view
        self.view_t = (now - self.view_started) as f32;
        let vp = crate::ui::widgets::steps(self.view_t / 0.2, 4.0);
        if self.view_t < 0.5 {
            ctx.request_repaint();
        }
        let org = body.min.to_vec2() - vec2(0.0, self.scroll[idx] + 8.0 * (1.0 - vp));
        let mut cv = Cv::new(ui, body, org, th, "view");
        let ch = match self.view {
            View::Deck => views::deck::draw(self, &mut cv, body.width(), body.height()),
            View::Profiles => views::profiles::draw(self, &mut cv, body.width(), body.height()),
            View::Phones => views::phones::draw(self, &mut cv, body.width(), body.height()),
            View::Marketplace => views::marketplace::draw(self, &mut cv, body.width(), body.height()),
            View::Settings => views::settings::draw(self, &mut cv, body.width(), body.height()),
        };
        self.content_h[idx] = ch;
        drop(cv);
        if vp < 1.0 {
            let mut ov = Cv::new(ui, body, body.min.to_vec2(), th, "viewfade");
            ov.fill(0.0, 0.0, body.width(), body.height(), crate::ui::draw::with_alpha(th.panel, 1.0 - vp));
        }
        crate::ui::widgets::scrollbar(ui, th, body, self.content_h[idx], &mut self.scroll[idx], "vsb");

        self.rail(ui, win);
        crate::views::editor::draw(self, ui, win);
        if self.view == View::Profiles {
            crate::views::profiles::dialog(self, ui, win);
        }
        if self.view == View::Marketplace {
            crate::views::marketplace::dialog(self, ui, win);
        }
        if self.view == View::Settings {
            crate::views::plugin_dialog::dialog(self, ui, win);
        }
        crate::views::install_dialog::dialog(self, ui, win);
        // a pressed button runs; what failed says why; the library keeps up and saves
        if let Some(i) = self.pending_press.take() {
            self.press_tile(i, now);
        }
        while let Ok(r) = self.ran_rx.try_recv() {
            if let Err(e) = r {
                self.say(&e, now);
            }
        }
        self.pump_plugins(&ctx, now);
        self.pump_front(now);
        self.pump_io(now);
        self.pump_settings(now);
        self.pump_tray(&ctx, now);
        self.pump_updates(&ctx, now);
        self.pump_phones(now);
        if let Some(e) = self.lib.tick(&mut self.sample, now) {
            self.say(&e, now);
        }
        if self.lib.pending() {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        self.toast(ui, win);
        self.titlebar(ui, &ctx, win);
        self.resize_edges(ui, &ctx, win);

        // screenshot mode: --shot file.bmp writes frame 8 and quits
        self.frames += 1;
        if let Some(path) = self.shot.clone() {
            // --plugin-window <plugin id>: that plugin's window, once its plugin has started
            let plugin_window = std::env::args().skip_while(|a| a != "--plugin-window").nth(1);
            let t = ctx.input(|i| i.time);
            if let Some(id) = &plugin_window {
                if self.plug_dialog.is_none() && t > 2.0 {
                    self.plug_dialog = Some(crate::views::plugin_dialog::PlugDialog::new(id));
                }
            }
            let at = std::env::args().skip_while(|a| a != "--shot-at").nth(1).and_then(|v| v.parse::<f64>().ok());
            let ready = match (&plugin_window, at) {
                (_, Some(secs)) => t >= secs && self.frames > 8,
                (Some(_), None) => t > 3.2,
                _ => self.frames == 8,
            };
            if ready && !self.shot_sent {
                self.shot_sent = true;
                let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
                    write_bmp(&path, &img);
                    std::process::exit(0);
                });
                ctx.send_viewport_cmd(ViewportCommand::Screenshot(cb));
            }
            ctx.request_repaint();
        }
    }
}

fn write_bmp(path: &str, img: &egui::ColorImage) {
    let (w, h) = (img.width() as u32, img.height() as u32);
    let row = (w * 3).div_ceil(4) * 4;
    let size: u32 = 54 + row * h;
    let mut out: Vec<u8> = Vec::with_capacity(size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    for y in (0..h as usize).rev() {
        let mut r = Vec::with_capacity(row as usize);
        for x in 0..w as usize {
            let c = img.pixels[y * w as usize + x];
            r.extend_from_slice(&[c.b(), c.g(), c.r()]);
        }
        r.resize(row as usize, 0);
        out.extend_from_slice(&r);
    }
    let _ = std::fs::write(path, out);
}
