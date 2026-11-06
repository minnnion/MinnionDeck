//! The Edit button window, kept simple: a big live preview on the left, four numbered steps on
//! the right (name, what it does, picture, colour), and one Add button.
use crate::ui::anim;
use crate::app::App;
use crate::backend::model::{COLOURS, Tile};
use crate::ui::draw::{Cv, lerp, with_alpha};
use crate::ui::icons;
use crate::views::deck::{Face, tile_face};
use crate::backend::catalog::{actions, offered_actions, offered_plugins as plugins};
use crate::generated::catalog::Action;
use crate::ui::widgets::scrollbar;
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};

#[derive(Clone)]
pub struct LookDraft {
    pub name: String,
    pub text: String,
    pub icon: String,
    pub colour: String,
    /// Its picture and GIF
    pub media: crate::backend::model::Media,
}

pub struct Editor {
    pub index: Option<usize>,
    pub cell: Option<(usize, usize)>,
    pub name: String,
    /// The chosen action's id in `catalog::ACTIONS` (empty: none yet) and its field values.
    pub action: String,
    pub values: std::collections::HashMap<String, String>,
    /// The action list is open (a new button starts with it open).
    pub picking: bool,
    pub act_search: String,
    /// Action list filter: 0 all, 1 built in, 2.. a plugin.
    pub source: usize,
    pub list_scroll: f32,
    pub list_rect: Option<Rect>,
    /// The picture still follows the action (until the user picks one).
    pub icon_auto: bool,
    pub name_auto: bool,
    pub chosen_at: Option<f64>,
    /// The looks (states) of the button and the one being edited.
    pub looks: Vec<LookDraft>,
    pub sel: usize,
    pub state_at: Option<f64>,
    pub pic_cache: Option<(usize, String, std::sync::Arc<Vec<(String, String)>>)>,
    pub anim: usize,
    /// Picture tab: 0 built in, 1 pixel icons, 2 app logos.
    pub tab: usize,
    pub search: String,
    pub grid_scroll: f32,
    pub grid_rect: Option<Rect>,
    pub scroll: f32,
    pub content: f32,
    pub opened: Option<f64>,
    pub closing: Option<(f64, bool)>,
    pub preview: Option<f64>,
    pub bump: Option<f64>,
    pub nudge: Option<f64>,
    pub is_new: bool,
    /// Import… or Make on the website was pressed (the window does it: it has the library)
    pub want: Option<MediaAsk>,
    /// This editor, so an import that finishes after it closed isn't put on another button
    pub token: u64,
}

/// What the Your pictures and GIFs tabs ask of the window.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MediaAsk {
    Import(crate::backend::media::Kind),
    Website(&'static str),
}

static EDITOR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The press animations offered here (indexes into `anim::NAMES`).
const ANIMS: [usize; 6] = [0, 1, 2, 3, 5, 8];

impl Editor {
    pub fn new(tile: Option<&Tile>, index: Option<usize>, cell: Option<(usize, usize)>) -> Editor {
        let icon = tile
            .map(|t| if t.image.is_empty() { t.icon.clone() } else { t.image.clone() })
            .unwrap_or_else(|| "bolt".into());
        let tab = if icon.starts_with("px:") { 1 } else if icon.starts_with("si:") { 2 } else if icon.starts_with("png:") { 3 } else { 0 };
        Editor {
            index,
            cell,
            name: tile.map(|t| t.label.clone()).unwrap_or_default(),
            action: tile.map(|t| t.action.clone()).unwrap_or_default(),
            values: tile.map(|t| t.values.iter().cloned().collect()).unwrap_or_default(),
            picking: tile.map(|t| t.action.is_empty()).unwrap_or(true),
            act_search: String::new(),
            source: 0,
            list_scroll: 0.0,
            list_rect: None,
            icon_auto: tile.is_none(),
            name_auto: tile.is_none(),
            chosen_at: None,
            looks: {
                let first = tile.map(|t| t.first_name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Default".into());
                let colour = tile.map(|t| t.colour.clone()).unwrap_or_else(|| "sky".into());
                let mut v = vec![LookDraft { name: first, text: String::new(), icon, colour, media: tile.map(|t| t.media.clone()).unwrap_or_default() }];
                if let Some(t) = tile {
                    for l in &t.more {
                        v.push(LookDraft { name: l.name.clone(), text: l.label.clone(), icon: l.icon.clone(), colour: l.colour.clone(), media: l.media.clone() });
                    }
                }
                v
            },
            sel: 0,
            state_at: None,
            pic_cache: None,
            anim: tile.map(|t| t.anim).unwrap_or(1),
            tab,
            search: String::new(),
            grid_scroll: 0.0,
            grid_rect: None,
            scroll: 0.0,
            content: 0.0,
            opened: None,
            closing: None,
            preview: None,
            bump: None,
            nudge: None,
            is_new: tile.is_none(),
            want: None,
            token: EDITOR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        }
    }

    fn missing(&self) -> Option<String> {
        if self.name.trim().is_empty() {
            return Some("Step 1: give the button a name.".into());
        }
        let Some(a) = action_by_id(&self.action) else {
            return Some("Step 2: choose what the button does.".into());
        };
        for f in a.fields {
            let v = self.values.get(f.key).map(|v| v.trim().to_string()).unwrap_or_default();
            if f.required && v.is_empty() {
                return Some(format!("Step 2: fill in {}.", f.label.to_lowercase()));
            }
        }
        None
    }

    /// For the screenshot flags: picks an action as if clicked.
    pub fn choose_pub(&mut self, a: &Action) {
        self.choose(a, -10.0);
    }

    fn choose(&mut self, a: &Action, now: f64) {
        self.action = a.id.to_string();
        self.values.clear();
        for f in a.fields {
            if !f.def.is_empty() {
                self.values.insert(f.key.to_string(), f.def.to_string());
            } else if f.kind == "select" {
                if let Some(o) = f.options.first() {
                    self.values.insert(f.key.to_string(), o.to_string());
                }
            }
        }
        self.picking = false;
        self.chosen_at = Some(now);
        if self.icon_auto {
            for l in self.looks.iter_mut() {
                l.icon = a.icon.to_string();
            }
            self.bump = Some(now);
        }
        // a plugin that switches the button itself (Play / Pause): one look for each of its states
        if a.states.len() >= 2 && self.looks.len() == 1 {
            let base = self.looks[0].clone();
            self.looks[0].name = a.states[0].to_string();
            let alt = ["coral", "mint", "lilac", "sun", "peach"];
            for (k, st) in a.states.iter().enumerate().skip(1) {
                self.looks.push(LookDraft { name: st.to_string(), text: String::new(), icon: base.icon.clone(), media: base.media.clone(), colour: alt[(k - 1) % alt.len()].to_string() });
            }
        }
        if self.name_auto || self.name.trim().is_empty() {
            self.name = a.label.to_string();
            self.name_auto = true;
        }
    }
}

pub fn action_by_id(id: &str) -> Option<&'static Action> {
    actions().iter().find(|a| a.id == id)
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn draw(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut ed) = app.editor.take() else { return };
    let th = app.theme();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let opened = *ed.opened.get_or_insert(now);
    // opening: fades and grows from 94% (0.18s); closing plays it backwards (0.12s)
    let mut p = ease(((now - opened) / 0.18) as f32);
    if let Some((t0, _)) = ed.closing {
        p = p.min(1.0 - ((now - t0) / 0.12) as f32);
    }
    let p = p.clamp(0.0, 1.0);
    let recent = |t: Option<f64>, d: f64| t.map(|t0| now - t0 < d).unwrap_or(false);
    if p < 1.0 || ed.closing.is_some() || ed.preview.is_some() || recent(ed.bump, 0.25) || recent(ed.nudge, 0.35) || recent(ed.state_at, 0.25) || recent(ed.chosen_at, 0.25) {
        ctx.request_repaint();
    }

    ui.interact(win, egui::Id::new("ed-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "edshade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * p));
    }

    let s = 0.94 + 0.06 * p;
    let dw0 = (win.width() - 120.0).min(900.0);
    let dh0 = (win.height() - 70.0).min(640.0);
    let (dw, dh) = (dw0 * s, dh0 * s);
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0;
    let dialog = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let missing = ed.missing();
    let mut close = false;
    let mut save = false;

    // ---- frame and title bar --------------------------------------------------------------
    {
        let mut cv = Cv::new(ui, win, dialog.min.to_vec2(), th, "edframe");
        cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
        cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
        cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
        let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
        crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
        let title = if ed.is_new { "New button" } else { "Edit button" };
        cv.text(50.0, 16.0, 24.0, 18.0, 700, light, title);
        let (cx, cy) = (dw - 4.0 - 10.0 - 34.0, 10.0);
        let r = cv.hit("edclose", cx, cy, 34.0, 32.0);
        if r.hovered() {
            cv.fill(cx, cy, 34.0, 32.0, th.coral);
        }
        cv.border(cx, cy, 34.0, 32.0, 3.0, if r.hovered() { th.coral } else { light });
        let fg = if r.hovered() { Color32::WHITE } else { light };
        for k in 0..10 {
            cv.fill(cx + 11.0 + k as f32, cy + 10.0 + k as f32, 2.0, 2.0, fg);
            cv.fill(cx + 11.0 + (9 - k) as f32, cy + 10.0 + k as f32, 2.0, 2.0, fg);
        }
        if r.clicked() {
            close = true;
        }
    }

    // ---- left: the live preview -----------------------------------------------------------
    let body_y = dy + 50.0;
    let foot_h = 64.0;
    let body_h = dh - 50.0 - foot_h - 4.0;
    let left_w = 260.0 * s;
    {
        let rect = Rect::from_min_size(pos2(dx + 4.0, body_y), vec2(left_w, body_h));
        let mut cv = Cv::new(ui, rect, rect.min.to_vec2(), th, "edprev");
        cv.dots(0.0, 0.0, left_w, body_h, th.ground, th.ground_dot);
        cv.fill(left_w - 3.0, 0.0, 3.0, body_h, th.ink);
        let size = 150.0;
        let (tx, ty) = ((left_w - size) / 2.0, 40.0);
        let xf = ed.preview.and_then(|t0| anim::eval(ed.anim, now - t0));
        if ed.preview.is_some() && xf.is_none() {
            ed.preview = None;
        }
        // a small pop whenever the look changes
        let pop = ed.bump.map(|t0| {
            let q = ((now - t0) as f32 / 0.22).clamp(0.0, 1.0);
            1.0 + 0.08 * (1.0 - (2.0 * q - 1.0).abs())
        });
        let look = ed.looks[ed.sel.min(ed.looks.len() - 1)].clone();
        let label = if !look.text.trim().is_empty() { look.text.trim().to_string() } else if ed.name.trim().is_empty() { "Name".to_string() } else { ed.name.trim().to_string() };
        // changing look flips the preview over, like the button on the deck
        let fx = ed.state_at.map(|t0| (1.0 - 2.0 * ((now - t0) as f32 / 0.22).clamp(0.0, 1.0)).abs().max(0.08)).unwrap_or(1.0);
        let face = Face { media: Some(&look.media), colour: &look.colour, icon: &look.icon, label: &label, dashed: false, hover: 0.0, down: false, xf, pop: pop.unwrap_or(1.0) };
        tile_face(&mut cv, tx + size * (1.0 - fx) / 2.0, ty, size * fx, size, &face);
        if ed.looks.len() > 1 {
            let n = ed.looks.len();
            let total = n as f32 * 12.0 - 4.0;
            for d in 0..n {
                let c = if d == ed.sel { th.ink } else { with_alpha(th.ink, 0.3) };
                cv.fill((left_w - total) / 2.0 + d as f32 * 12.0, ty - 18.0, 8.0, 8.0, c);
            }
            cv.text_c(0.0, left_w, ty + size + 4.0, 18.0, 13.0, 600, th.slate, &format!("Look {} of {}: {}", ed.sel + 1, n, look.name));
        }
        let mut o = cv.opt_btn(14.0);
        o.bw = 3.0;
        o.shadow = 3.0;
        let bw = cv.measure(14.0, 600, "Try it") + 40.0;
        if cv.button("tryit", (left_w - bw) / 2.0, ty + size + 26.0, bw, 36.0, "Try it", o) {
            ed.preview = Some(now);
            if ed.looks.len() > 1 {
                ed.sel = (ed.sel + 1) % ed.looks.len();
                ed.state_at = Some(now);
            }
        }
        cv.para(20.0, ty + size + 80.0, left_w - 40.0, 18.0, 13.0, 400, th.slate, "This is how the button looks on the deck and on your phone.");
    }

    // ---- right: the four steps ------------------------------------------------------------
    let right = Rect::from_min_size(pos2(dx + 4.0 + left_w, body_y), vec2(dw - 8.0 - left_w, body_h));
    if let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) {
        let wheel = ctx.input(|i| i.smooth_scroll_delta.y);
        if wheel != 0.0 {
            if ed.list_rect.map(|r| r.contains(pos)).unwrap_or(false) {
                ed.list_scroll = (ed.list_scroll - wheel).max(0.0);
            } else if ed.grid_rect.map(|r| r.contains(pos)).unwrap_or(false) {
                ed.grid_scroll = (ed.grid_scroll - wheel).max(0.0);
            } else if right.contains(pos) {
                let max = (ed.content - right.height()).max(0.0);
                ed.scroll = (ed.scroll - wheel).clamp(0.0, max);
            }
        }
    }
    {
        let org = right.min.to_vec2() - vec2(0.0, ed.scroll);
        let mut cv = Cv::new(ui, right, org, th, "edform");
        // Go to page keeps the page's id; the chips show its name (a folder opens its first page)
        let pages: Vec<(String, String)> = app.sample.pages.iter().map(|p| (if p.id.is_empty() { p.name.clone() } else { p.id.clone() }, if p.folder { format!("{} (folder)", p.name) } else { p.name.clone() })).collect();
        ed.content = form(&mut cv, &mut ed, right.width(), now, &pages);
    }
    scrollbar(ui, th, right, ed.content, &mut ed.scroll, "edform");

    // ---- footer -------------------------------------------------------------------------------
    {
        let foot = Rect::from_min_size(pos2(dx + 4.0, dy + dh - 4.0 - foot_h), vec2(dw - 8.0, foot_h));
        let mut cv = Cv::new(ui, foot, foot.min.to_vec2(), th, "edfoot");
        let fw = dw - 8.0;
        cv.fill(0.0, 0.0, fw, foot_h, th.panel);
        cv.fill(0.0, 0.0, fw, 3.0, th.ink);
        // what is missing, shaking when Add is pressed too early
        let shake = ed.nudge.map(|t0| {
            let q = ((now - t0) / 0.32) as f32;
            if q < 1.0 { 6.0 * (q * 4.0 * std::f32::consts::TAU / 2.0).sin() * (1.0 - q) } else { 0.0 }
        });
        let (msg, col) = match missing.as_deref() {
            Some(m) => (m, th.slate),
            None => ("Ready. Press Add to put it on the deck.", th.mint),
        };
        if missing.is_none() {
            cv.glyph("check", 20.0, 24.0, 16.0, th.mint);
        }
        cv.text(if missing.is_none() { 44.0 } else { 20.0 } + shake.unwrap_or(0.0), 21.0, 21.0, 15.0, 600, if shake.unwrap_or(0.0) != 0.0 { th.coral } else { col }, msg);
        let label = if ed.is_new { "Add button" } else { "Save" };
        let mut add = cv.opt_btn(15.0);
        add.bg = if missing.is_none() { th.mint } else { th.tile };
        add.bg_hover = if missing.is_none() { th.mint_dark } else { th.hover };
        add.fg = if missing.is_none() { th.on_accent } else { th.slate };
        let aw = cv.measure(15.0, 600, label) + 40.0;
        if cv.button("edadd", fw - 16.0 - aw, 11.0, aw, 42.0, label, add) {
            if missing.is_none() {
                save = true;
            } else {
                ed.nudge = Some(now);
            }
        }
        let o = cv.opt_btn(15.0);
        let cw = cv.measure(15.0, 600, "Cancel") + 32.0;
        if cv.button("edcancel", fw - 16.0 - aw - 10.0 - cw, 11.0, cw, 42.0, "Cancel", o) {
            close = true;
        }
    }

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if (close || save) && ed.closing.is_none() {
        ed.closing = Some((now, save));
    }
    if let Some((t0, saving)) = ed.closing {
        if now - t0 >= 0.12 {
            if saving {
                save_tile(app, &ed, now);
            }
            return;
        }
    }
    if let Some(ask) = ed.want.take() {
        match ask {
            MediaAsk::Import(kind) => app.start_media_import(kind, ed.token, ed.sel),
            MediaAsk::Website(page) => {
                let url = crate::backend::media::site_page(page);
                match crate::backend::actions::open_url(&url) {
                    Ok(()) => app.say("Opened the Minniondeck website. Download what you make, then use Import here.", now),
                    Err(e) => app.say(&e, now),
                }
            }
        }
    }
    app.editor = Some(ed);
}

fn save_tile(app: &mut App, ed: &Editor, now: f64) {
    let apply = |t: &mut Tile| {
        t.label = ed.name.trim().to_string();
        let first = &ed.looks[0];
        let (icon, image) = if first.icon.contains(':') { ("bolt".to_string(), first.icon.clone()) } else { (first.icon.clone(), String::new()) };
        t.icon = icon;
        t.image = image;
        t.colour = first.colour.clone();
        t.first_name = first.name.clone();
        t.more = ed.looks[1..].iter().map(|l| crate::backend::model::Look { name: l.name.clone(), label: l.text.clone(), icon: l.icon.clone(), colour: l.colour.clone(), media: l.media.clone() }).collect();
        t.cur = 0;
        t.anim = ed.anim;
        t.action = ed.action.clone();
        t.values = ed.values.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    };
    match (ed.index, ed.cell) {
        (Some(i), _) if i < app.sample.tiles.len() => {
            apply(&mut app.sample.tiles[i]);
            app.fresh_tile = Some((i, now));
            app.say("Button saved", now);
        }
        (_, Some((c, r))) => {
            let mut t = Tile::new(app.sample.page, c, r);
            apply(&mut t);
            app.sample.tiles.push(t);
            app.fresh_tile = Some((app.sample.tiles.len() - 1, now));
            if let Some(pg) = app.sample.pages.get_mut(app.sample.page) {
                pg.count += 1;
            }
            app.say("Button added", now);
        }
        _ => {}
    }
}

/// A step heading: a numbered mint square and the question.
fn step(cv: &mut Cv, x: f32, y: f32, n: usize, title: &str, done: bool) {
    let th = cv.th;
    cv.fill(x, y, 26.0, 26.0, if done { th.mint } else { th.tile });
    cv.border(x, y, 26.0, 26.0, 3.0, th.ink);
    if done {
        cv.glyph("check", x + 5.0, y + 5.0, 16.0, th.on_accent);
    } else {
        cv.text_c(x, 26.0, y + 3.0, 20.0, 15.0, 700, th.ink, &n.to_string());
    }
    cv.text(x + 36.0, y + 1.0, 24.0, 17.0, 700, th.ink, title);
}

fn form(cv: &mut Cv, ed: &mut Editor, w: f32, now: f64, pages: &[(String, String)]) -> f32 {
    let th = cv.th;
    cv.fill(0.0, 0.0, w, 5000.0, th.panel);
    let x = 24.0;
    let fw = w - 48.0 - 14.0;
    let mut y = 20.0;

    // 1 name
    step(cv, x, y, 1, "What is it called?", !ed.name.trim().is_empty());
    y += 36.0;
    let mut name = std::mem::take(&mut ed.name);
    let before = name.clone();
    cv.text_field("name", x, y, fw, 40.0, &mut name, 15.0, "For example: Mute mic");
    if name != before {
        ed.name_auto = false;
    }
    ed.name = name;
    y += 40.0 + 28.0;

    // 2 what it does
    let chosen = action_by_id(&ed.action);
    let done = ed.missing().map(|m| !m.starts_with("Step 2")).unwrap_or(true) && chosen.is_some();
    step(cv, x, y, 2, "What does it do?", done);
    y += 38.0;
    if ed.picking || chosen.is_none() {
        y += action_list(cv, ed, x, y, fw, now) + 26.0;
    } else if let Some(a) = chosen {
        y += chosen_action(cv, ed, a, x, y, fw, now, pages) + 26.0;
    }

    // 3 looks and pictures
    step(cv, x, y, 3, "How does it look?", true);
    y += 38.0;
    y += looks_strip(cv, ed, x, y, fw, now) + 16.0;
    // tab ids: 0 simple, 3 the plugins' own icons, 1 pixel icons, 2 app logos
    // 4 and 5: your pictures and GIFs, for the look being edited
    let tabs = [
        (0, "Simple", icons::BUILTIN.len()),
        (3, "Plugins", crate::backend::catalog::plugin_icons().len()),
        (1, "Pixel icons", icons::px_names().len()),
        (2, "App logos", icons::si_icons().len()),
        (4, "Your pictures", crate::backend::media::pictures().len()),
        (5, "GIFs", crate::backend::media::gifs().len()),
    ];
    let mut tx = x;
    for &(i, name, n) in tabs.iter() {
        let label = format!("{name} {n}");
        let tw = cv.measure(14.0, 600, &label) + 24.0;
        if tx > x && tx + tw > x + fw {
            tx = x;
            y += 34.0 + 8.0;
        }
        let r = cv.hit(&format!("tab{i}"), tx, y, tw, 34.0);
        let sel = ed.tab == i;
        if sel {
            cv.shadow(tx, y, tw, 34.0, 3.0, th.shadow);
        }
        cv.fill(tx, y, tw, 34.0, if sel { th.mint } else if r.hovered() { th.hover } else { th.tile });
        cv.border(tx, y, tw, 34.0, 3.0, th.ink);
        let fg = if sel { th.on_accent } else { th.ink };
        let nw = cv.text(tx + 12.0, y + 7.2, 19.6, 14.0, 600, fg, name);
        let n = &n;
        cv.text(tx + 12.0 + nw + 4.0, y + 9.0, 16.0, 11.0, 600, if sel { th.on_accent } else { th.slate }, &n.to_string());
        if r.clicked() {
            ed.tab = i;
            ed.grid_scroll = 0.0;
        }
        tx += tw + 8.0;
    }
    y += 34.0 + 10.0;
    if ed.tab == 4 || ed.tab == 5 {
        y += media_panel(cv, ed, x, y, fw, now) + 26.0;
    } else {
    let mut search = std::mem::take(&mut ed.search);
    let before = search.clone();
    cv.text_field("isearch", x, y, fw, 38.0, &mut search, 14.0, "Search pictures, like mic, fenix or spotify");
    if search != before {
        ed.grid_scroll = 0.0;
    }
    ed.search = search;
    y += 38.0 + 10.0;
    y += picture_grid(cv, ed, x, y, fw, now) + 26.0;
    }

    // 4 colour and press animation
    step(cv, x, y, 4, "Colour and press effect", true);
    y += 38.0;
    let mut sw: Vec<(&str, &str)> = vec![("tile", "Plain")];
    sw.extend(COLOURS.iter().copied());
    for (i, (key, _)) in sw.iter().enumerate() {
        let sx = x + i as f32 * 46.0;
        let r = cv.hit(&format!("col{i}"), sx, y, 38.0, 38.0);
        let sel = ed.looks[ed.sel].colour == *key;
        let up = if sel { -3.0 } else if r.hovered() { -1.0 } else { 0.0 };
        cv.shadow(sx, y + up, 38.0, 38.0, if sel { 4.0 } else { 2.0 }, th.shadow);
        cv.fill(sx, y + up, 38.0, 38.0, th.accent(key));
        cv.border(sx, y + up, 38.0, 38.0, 3.0, th.ink);
        if sel {
            cv.glyph("check", sx + 11.0, y + up + 11.0, 16.0, if *key == "tile" { th.ink } else { th.on_accent });
        }
        if r.clicked() && !sel {
            ed.looks[ed.sel].colour = key.to_string();
            ed.bump = Some(now);
        }
    }
    y += 38.0 + 16.0;
    let mut ax = x;
    for (k, &ai) in ANIMS.iter().enumerate() {
        let label = anim::NAMES[ai];
        let bw = cv.measure(14.0, 600, label) + 24.0;
        let r = cv.hit(&format!("an{k}"), ax, y, bw, 34.0);
        let sel = ed.anim == ai;
        cv.fill(ax, y, bw, 34.0, if sel { th.mint } else if r.hovered() { th.hover } else { th.tile });
        cv.border(ax, y, bw, 34.0, 3.0, th.ink);
        cv.text_c(ax, bw, y + 7.2, 19.6, 14.0, 600, if sel { th.on_accent } else { th.ink }, label);
        if r.clicked() {
            ed.anim = ai;
            ed.preview = Some(now);
        }
        ax += bw + 6.0;
    }
    y += 34.0 + 8.0;
    y += cv.para(x, y, fw, 18.0, 13.0, 400, th.slate, "The effect plays when the button is pressed. Pick one to see it on the preview.");
    y + 24.0
}

/// Your pictures (tab 4) or GIFs (tab 5) for the look being edited: Import…, Make on the website,
/// None and each one as a cell, then how it shows. Returns its height.
fn media_panel(cv: &mut Cv, ed: &mut Editor, x: f32, y0: f32, w: f32, now: f64) -> f32 {
    use crate::backend::media::{self, Kind};
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let gifs = ed.tab == 5;
    let mut y = y0;
    let multi = ed.looks.len() > 1;
    let whose = if multi { format!(" Only look {}, {}, gets it; each look can have its own.", ed.sel + 1, ed.looks[ed.sel].name) } else { String::new() };
    let help = if gifs {
        format!("GIFs are drawn on the Minniondeck website (GIF maker). Press Download GIF there, then Import it here.{whose}")
    } else {
        format!("Pictures are made on the Minniondeck website (Images). Press Download PNG there, then Import it here. A picture fills the button behind its icon and name.{whose}")
    };
    y += cv.para(x, y, w, 18.0, 13.0, 400, th.slate, &help) + 10.0;
    // Import… and Make on the website
    let o = cv.opt_btn(14.0);
    let iw = cv.measure(14.0, 600, "Import…") + 28.0;
    if cv.button(if gifs { "gimport" } else { "pimport" }, x, y, iw, 38.0, "Import…", o) {
        ed.want = Some(MediaAsk::Import(if gifs { Kind::Gif } else { Kind::Picture }));
    }
    let ww = cv.measure(14.0, 600, "Make on the website") + 28.0;
    if cv.button(if gifs { "gsite" } else { "psite" }, x + iw + 10.0, y, ww, 38.0, "Make on the website", o) {
        ed.want = Some(MediaAsk::Website(if gifs { "gifs" } else { "images" }));
    }
    y += 38.0 + 14.0;
    // the cells: None, then each picture or GIF
    let items: Vec<(String, String)> = if gifs {
        media::gifs().into_iter().map(|g| (g.id, if g.group == "GIFs" { g.name } else { format!("{} · {}", g.name, g.group) })).collect()
    } else {
        media::pictures().into_iter().map(|p| (p.id, p.name)).collect()
    };
    let cur = if gifs { ed.looks[ed.sel].media.gif.clone() } else { ed.looks[ed.sel].media.pic.clone() };
    let cell = 76.0;
    let per = (((w + 8.0) / (cell + 8.0)).floor() as usize).max(1);
    let all: Vec<(String, String)> = std::iter::once((String::new(), "None".to_string())).chain(items).collect();
    for (k, (id, name)) in all.iter().enumerate() {
        let (cx, cy) = (x + (k % per) as f32 * (cell + 8.0), y + (k / per) as f32 * (cell + 26.0));
        let r = cv.hit(&format!("med{}_{k}", ed.tab), cx, cy, cell, cell);
        let sel = *id == cur;
        if sel {
            cv.shadow(cx, cy, cell, cell, 3.0, th.shadow);
        }
        cv.fill(cx, cy, cell, cell, if r.hovered() { th.hover } else { th.tile });
        let (ix, iy, isz) = (cx + 6.0, cy + 6.0, cell - 12.0);
        if id.is_empty() {
            cv.glyph("cross", cx + cell / 2.0 - 8.0, cy + cell / 2.0 - 8.0, 16.0, th.slate);
        } else if gifs {
            match crate::ui::media_tex::gif_frame(&ctx, id, now, 1) {
                Some((tid, size, next)) => {
                    let (bx, by, bw, bh) = crate::ui::media_tex::contain_box(size, isz, isz);
                    cv.tex(tid, ix + bx, iy + by, bw, bh, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)));
                    if let Some(n) = next {
                        ctx.request_repaint_after(std::time::Duration::from_secs_f64(n.clamp(0.02, 1.0)));
                    }
                }
                None => {
                    cv.text_c(cx, cell, cy + cell / 2.0 - 9.0, 18.0, 12.0, 400, th.slate, "missing");
                }
            }
        } else {
            match crate::ui::media_tex::picture(&ctx, id) {
                Some((tid, size)) => {
                    let (bx, by, bw, bh) = crate::ui::media_tex::contain_box(size, isz, isz);
                    cv.tex(tid, ix + bx, iy + by, bw, bh, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)));
                }
                None => {
                    cv.text_c(cx, cell, cy + cell / 2.0 - 9.0, 18.0, 12.0, 400, th.slate, "missing");
                }
            }
        }
        cv.border(cx, cy, cell, cell, 3.0, if sel { th.mint } else { th.ink });
        if sel {
            cv.fill(cx + cell - 22.0, cy + 2.0, 20.0, 20.0, th.mint);
            cv.glyph("check", cx + cell - 20.0, cy + 4.0, 16.0, th.on_accent);
        }
        cv.text_fit(cx, cy + cell + 4.0, 18.0, 12.0, 600, th.ink, name, cell);
        if r.clicked() {
            let m = &mut ed.looks[ed.sel].media;
            if gifs {
                m.gif = id.clone();
            } else {
                m.pic = id.clone();
            }
            ed.bump = Some(now);
        }
    }
    let rows = all.len().div_ceil(per);
    y += rows as f32 * (cell + 26.0) + 6.0;
    // how it shows
    let m = ed.looks[ed.sel].media.clone();
    if !gifs && !m.pic.is_empty() {
        let mut v = if m.contain { "contain".to_string() } else { "cover".to_string() };
        let opts = [("cover".to_string(), "Fill the button".to_string()), ("contain".to_string(), "Show it all".to_string())];
        y += chips(cv, "pfit", x, y, w, &opts, &mut v) + 8.0;
        if (v == "contain") != m.contain {
            ed.looks[ed.sel].media.contain = v == "contain";
            ed.bump = Some(now);
        }
    }
    if gifs && !m.gif.is_empty() {
        cv.text(x, y, 19.0, 14.0, 600, th.ink, "Where it plays");
        y += 24.0;
        let mut v = if m.gif_icon { "icon".to_string() } else { "bg".to_string() };
        let opts = [("bg".to_string(), "Background".to_string()), ("icon".to_string(), "Icon".to_string())];
        y += chips(cv, "gplace", x, y, w, &opts, &mut v) + 10.0;
        let mut s = m.speed.min(2).to_string();
        cv.text(x, y, 19.0, 14.0, 600, th.ink, "Speed");
        y += 24.0;
        let sopts = [("0".to_string(), "Slow".to_string()), ("1".to_string(), "Normal".to_string()), ("2".to_string(), "Fast".to_string())];
        y += chips(cv, "gspeed", x, y, w, &sopts, &mut s) + 8.0;
        let speed: u8 = s.parse().unwrap_or(1);
        let lm = &mut ed.looks[ed.sel].media;
        if (v == "icon") != lm.gif_icon || speed != lm.speed {
            lm.gif_icon = v == "icon";
            lm.speed = speed;
            ed.bump = Some(now);
        }
        let note = if m.gif_icon { "Plays in the icon's place." } else { "Plays behind the icon and name. The name gets a dark backing so it stays easy to read." };
        y += cv.para(x, y, w, 18.0, 13.0, 400, th.slate, &format!("{note} Speed changes GIFs drawn in the GIF maker; a GIF file keeps its own timing.")) + 4.0;
    }
    y - y0
}

/// The pictures of the chosen tab, filtered by the search, 48px cells. Returns its height.
fn picture_grid(cv: &mut Cv, ed: &mut Editor, x: f32, y: f32, w: f32, now: f64) -> f32 {
    let th = cv.th;
    let h = 228.0;
    cv.fill(x, y, w, h, th.tile);
    cv.border(x, y, w, h, 3.0, th.ink);
    ed.grid_rect = Some(cv.abs(x, y, w, h));
    let q = ed.search.trim().to_lowercase();
    let fresh = !matches!(&ed.pic_cache, Some((t, s, _)) if *t == ed.tab && *s == q);
    if fresh {
        let all: Vec<(String, String)> = match ed.tab {
            0 => icons::BUILTIN.iter().map(|(id, n)| (id.to_string(), n.to_string())).collect(),
            1 => icons::px_names().iter().map(|n| (format!("px:{n}"), icons::px_name(n))).collect(),
            3 => crate::backend::catalog::plugin_icons().iter().map(|(plugin, key)| (format!("png:{key}"), format!("{} · {plugin}", key.rsplit('/').next().unwrap_or(key).replace(['-', '_'], " ")))).collect(),
            _ => icons::si_icons().iter().map(|s| (format!("si:{}", s.slug), s.name.to_string())).collect(),
        };
        let list: Vec<(String, String)> = all.into_iter().filter(|(id, n)| q.is_empty() || n.to_lowercase().contains(&q) || id.contains(&q)).collect();
        ed.pic_cache = Some((ed.tab, q.clone(), std::sync::Arc::new(list)));
    }
    let ids = ed.pic_cache.as_ref().map(|c| c.2.clone()).unwrap_or_default();
    let cell = 48.0;
    let gap = 6.0;
    let inner = w - 6.0 - 12.0 - 14.0;
    let cols = (((inner + gap) / (cell + gap)).floor() as usize).max(1);
    let rows = ids.len().div_ceil(cols);
    let total = 12.0 + rows as f32 * (cell + gap) + 6.0;
    let view = h - 6.0 - 26.0;
    let max = (total - view).max(0.0);
    ed.grid_scroll = ed.grid_scroll.min(max);
    let clip = cv.clip_push(x + 3.0, y + 3.0, w - 6.0, view);
    let top = y + 3.0 + 6.0 - ed.grid_scroll;
    let mut hovered: Option<String> = None;
    let mut chosen: Option<String> = None;
    if ids.is_empty() {
        cv.text(x + 14.0, y + 14.0, 19.0, 14.0, 400, th.slate, "Nothing matches. Try a shorter word.");
    }
    for r in 0..rows {
        let ry = top + r as f32 * (cell + gap);
        if ry + cell < y || ry > y + h {
            continue;
        }
        for c in 0..cols {
            let Some((id, name)) = ids.get(r * cols + c) else { break };
            let cx = x + 3.0 + 6.0 + c as f32 * (cell + gap);
            let resp = cv.hit(&format!("pic{id}"), cx, ry, cell, cell);
            let sel = *id == ed.looks[ed.sel].icon;
            if sel {
                cv.fill(cx, ry, cell, cell, th.mint);
                cv.border(cx, ry, cell, cell, 3.0, th.ink);
            } else if resp.hovered() {
                cv.fill(cx, ry, cell, cell, th.hover);
                cv.border(cx, ry, cell, cell, 3.0, th.slate);
                hovered = Some(name.clone());
            }
            let s = if id.starts_with("si:") && icons::brand_art().contains_key(&id[3..]) { 32.0 } else if id.starts_with("png:") { 32.0 } else { 24.0 };
            if id.starts_with("png:") {
                cv.fill(cx + 5.0, ry + 5.0, cell - 10.0, cell - 10.0, chip_bg(th, id));
            }
            cv.icon(id, cx + (cell - s) / 2.0, ry + (cell - s) / 2.0, s, if sel { th.on_accent } else { th.ink });
            if resp.clicked() {
                chosen = Some(id.clone());
            }
        }
    }
    cv.clip_pop(clip);
    // a line under the grid naming what is under the pointer, or what is chosen
    cv.fill(x + 3.0, y + h - 3.0 - 26.0, w - 6.0, 26.0, th.panel);
    let shown = hovered.unwrap_or_else(|| {
        let cur = ids.iter().find(|(id, _)| *id == ed.looks[ed.sel].icon).map(|(_, n)| n.clone());
        format!("{} · {} pictures", cur.map(|n| format!("Chosen: {n}")).unwrap_or_else(|| "Click a picture".into()), ids.len())
    });
    cv.text_fit(x + 12.0, y + h - 3.0 - 22.0, 18.0, 13.0, 600, th.slate, &shown, w - 24.0);
    if max > 0.0 {
        let tx = x + w - 3.0 - 14.0;
        cv.fill(tx, y + 3.0, 14.0, view, th.panel);
        cv.fill(tx + 6.0, y + 3.0, 2.0, view, th.ground_dot);
        let thh = (view * view / total).max(24.0);
        let ty = y + 3.0 + ed.grid_scroll / max * (view - thh);
        cv.fill(tx + 3.0, ty + 3.0, 8.0, thh - 6.0, th.mint);
        cv.fill(tx + 7.0, ty + 3.0, 4.0, thh - 6.0, th.mint_dark);
        cv.border(tx + 3.0, ty + 3.0, 8.0, thh - 6.0, 2.0, th.ink);
    }
    if let Some(id) = chosen {
        ed.looks[ed.sel].icon = id;
        ed.icon_auto = false;
        ed.bump = Some(now);
    }
    h
}

/// The chosen action: a card saying what and where from, a Change button, then its settings.
#[allow(clippy::too_many_arguments)]
fn chosen_action(cv: &mut Cv, ed: &mut Editor, a: &'static Action, x: f32, y0: f32, w: f32, now: f64, pages: &[(String, String)]) -> f32 {
    let th = cv.th;
    let mut y = y0;
    // the card slides in from the right when picked
    let slide = ed.chosen_at.map(|t0| 20.0 * (1.0 - ease(((now - t0) / 0.2) as f32))).unwrap_or(0.0);
    let cx = x + slide;
    cv.shadow(cx, y, w, 64.0, 4.0, th.shadow);
    cv.fill(cx, y, w, 64.0, th.mint);
    cv.border(cx, y, w, 64.0, 3.0, th.ink);
    cv.fill(cx + 12.0, y + 12.0, 40.0, 40.0, chip_bg(th, a.icon));
    cv.border(cx + 12.0, y + 12.0, 40.0, 40.0, 3.0, th.ink);
    cv.icon(a.icon, cx + 16.0, y + 16.0, 32.0, th.ink);
    cv.text_fit(cx + 64.0, y + 10.0, 22.0, 16.0, 700, th.on_accent, a.label, w - 64.0 - 110.0);
    let from = if a.plugin.is_empty() { format!("Built in · {}", a.group) } else { format!("From the {} plugin", a.plugin_name) };
    cv.text_fit(cx + 64.0, y + 33.0, 18.0, 13.0, 600, th.on_accent, &from, w - 64.0 - 110.0);
    let mut o = cv.opt_btn(14.0);
    o.bw = 3.0;
    o.shadow = 3.0;
    let bw = cv.measure(14.0, 600, "Change") + 28.0;
    if cv.button("actchange", cx + w - 12.0 - bw, y + 14.0, bw, 36.0, "Change", o) {
        ed.picking = true;
        ed.list_scroll = 0.0;
    }
    y += 64.0 + 16.0;

    // what the plugin says about it
    let mut notes: Vec<String> = vec![];
    if !a.help.is_empty() {
        notes.push(a.help.to_string());
    }
    if a.states.len() >= 2 {
        notes.push(format!("The button shows {} by itself.", a.states.join(" or ")));
    }
    if a.knob {
        notes.push("A knob: turn it with the mouse wheel on the PC, or drag it on the phone.".into());
    }
    if a.screen {
        notes.push("A screen: it shows what the plugin sends, and a press does nothing.".into());
    }
    for n in &notes {
        y += cv.para(x, y, w, 18.0, 13.0, 400, th.slate, n) + 6.0;
    }
    if a.fields.is_empty() {
        y += cv.para(x, y, w, 18.0, 13.0, 600, th.ink, "Nothing to set up. It is ready to use.");
        return y - y0;
    }

    // its settings, one under another
    for f in a.fields {
        y += 8.0;
        let mut lw = cv.text(x, y, 19.0, 14.0, 600, th.ink, f.label);
        if f.unit.len() > 0 {
            lw += cv.text(x + lw + 6.0, y + 1.0, 18.0, 13.0, 400, th.slate, &format!("({})", f.unit)) + 6.0;
        }
        if f.required {
            cv.text(x + lw + 8.0, y + 1.0, 18.0, 13.0, 400, th.slate, "needed");
        }
        y += 24.0;
        let key = f.key.to_string();
        let mut v = ed.values.get(&key).cloned().unwrap_or_default();
        match f.kind {
            "keys" => {
                cv.key_field(&format!("f_{key}"), x, y, w, 40.0, &mut v, 15.0, "Click here, then press the keys");
                y += 40.0;
            }
            "select" | "pages" => {
                let opts: Vec<(String, String)> = if f.kind == "pages" { pages.to_vec() } else { f.options.iter().map(|o| (o.to_string(), o.to_string())).collect() };
                y += chips(cv, &format!("f_{key}"), x, y, w, &opts, &mut v);
            }
            "dynamic" => {
                // the plugin lists the choices while it runs; until it does (or when it isn't
                // running) the value can be typed
                let asked: serde_json::Map<String, serde_json::Value> = ed.values.iter().map(|(k, v)| (k.to_string(), serde_json::Value::String(v.clone()))).collect();
                match crate::backend::plugins::dynamic_options(a.id, f.key, &serde_json::Value::Object(asked)) {
                    crate::backend::plugins::Dyn::Ready(list) if !list.is_empty() => {
                        let opts: Vec<(String, String)> = list.iter().map(|c| (c.value.clone(), c.label.clone())).collect();
                        y += chips(cv, &format!("f_{key}"), x, y, w, &opts, &mut v);
                    }
                    crate::backend::plugins::Dyn::Loading => {
                        cv.text(x, y + 10.0, 20.0, 14.0, 400, th.slate, "Asking the plugin…");
                        y += 40.0;
                    }
                    _ => {
                        cv.text_field(&format!("f_{key}"), x, y, w, 40.0, &mut v, 15.0, "The plugin lists these when it runs");
                        y += 40.0;
                    }
                }
            }
            _ => {
                let ph = if !f.placeholder.is_empty() { f.placeholder } else if f.kind == "number" { "0" } else { "" };
                cv.text_field(&format!("f_{key}"), x, y, w, 40.0, &mut v, 15.0, ph);
                y += 40.0;
            }
        }
        ed.values.insert(key, v);
        if !f.help.is_empty() {
            y += 6.0;
            y += cv.para(x, y, w, 18.0, 13.0, 400, th.slate, f.help);
        }
    }
    y - y0
}

/// A choice as a row of chips that wraps. Returns its height.
/// Each option is (what is kept, what it says).
fn chips(cv: &mut Cv, id: &str, x: f32, y: f32, w: f32, opts: &[(String, String)], v: &mut String) -> f32 {
    let th = cv.th;
    let (mut cx, mut cy) = (x, y);
    for (i, (val, o)) in opts.iter().enumerate() {
        let bw = cv.measure(14.0, 600, o) + 26.0;
        if cx > x && cx + bw > x + w {
            cx = x;
            cy += 40.0;
        }
        let r = cv.hit(&format!("{id}_{i}"), cx, cy, bw, 34.0);
        let sel = v == val;
        if sel {
            cv.shadow(cx, cy, bw, 34.0, 3.0, th.shadow);
        }
        cv.fill(cx, cy, bw, 34.0, if sel { th.mint } else if r.hovered() { th.hover } else { th.tile });
        cv.border(cx, cy, bw, 34.0, 3.0, th.ink);
        cv.text_c(cx, bw, cy + 7.2, 19.6, 14.0, 600, if sel { th.on_accent } else { th.ink }, o);
        if r.clicked() {
            *v = val.clone();
        }
        cx += bw + 6.0;
    }
    if opts.is_empty() {
        cv.text(x, y + 8.0, 19.0, 13.0, 400, th.slate, "Nothing to choose from yet.");
    }
    cy + 34.0 - y
}

/// Every action there is: a search, a row of sources (built in and each plugin, with its
/// picture), and the matching actions as cards. Returns its height.
fn action_list(cv: &mut Cv, ed: &mut Editor, x: f32, y0: f32, w: f32, now: f64) -> f32 {
    let th = cv.th;
    let mut y = y0;
    let mut q = std::mem::take(&mut ed.act_search);
    let before = q.clone();
    cv.text_field("actsearch", x, y, w, 40.0, &mut q, 15.0, &format!("Search {} actions, like volume, timer or gear", offered_actions().len()));
    if q != before {
        ed.list_scroll = 0.0;
    }
    ed.act_search = q;
    y += 40.0 + 10.0;

    // sources
    let builtin = actions().iter().filter(|a| a.plugin.is_empty()).count();
    let mut sources: Vec<(String, &str, usize)> = vec![("All".into(), "grid", offered_actions().len()), ("Built in".into(), "bolt", builtin)];
    for p in plugins() {
        sources.push((p.name.to_string(), p.icon, p.count));
    }
    // an even grid: every source the same size, picture on a little panel, count on the right
    let gap = 6.0;
    let cols = (((w + gap) / (128.0 + gap)).floor() as usize).clamp(2, 5);
    let bw = ((w - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let bh = 36.0;
    let ctx = cv.ui.ctx().clone();
    for (i, (name, icon, n)) in sources.iter().enumerate() {
        let (sx, sy) = (x + (i % cols) as f32 * (bw + gap), y + (i / cols) as f32 * (bh + gap));
        let r = cv.hit(&format!("src{i}"), sx, sy, bw, bh);
        let sel = ed.source == i;
        let hv = ctx.animate_bool_with_time(egui::Id::new(("src-h", i)), r.hovered() && !sel, 0.12);
        let lift = -2.0 * hv;
        let sy = sy + lift;
        if sel || hv > 0.0 {
            cv.shadow(sx, sy, bw, bh, if sel { 3.0 } else { 3.0 * hv }, th.shadow);
        }
        cv.fill(sx, sy, bw, bh, if sel { th.mint } else { lerp(th.tile, th.hover, hv) });
        cv.border(sx, sy, bw, bh, 3.0, th.ink);
        let fg = if sel { th.on_accent } else { th.ink };
        cv.fill(sx + 6.0, sy + 6.0, 24.0, 24.0, chip_bg(th, icon));
        cv.border(sx + 6.0, sy + 6.0, 24.0, 24.0, 2.0, th.ink);
        cv.icon(icon, sx + 10.0, sy + 10.0, 16.0, th.ink);
        let count = n.to_string();
        let cw = cv.measure(11.0, 600, &count);
        let cx = sx + bw - 8.0 - cw;
        cv.text(cx, sy + 10.5, 15.0, 11.0, 600, if sel { th.on_accent } else { th.slate }, &count);
        cv.text_fit(sx + 36.0, sy + 9.0, 18.0, 13.0, 600, fg, name, cx - 6.0 - (sx + 36.0));
        if r.clicked() {
            ed.source = i;
            ed.list_scroll = 0.0;
        }
    }
    let rows = sources.len().div_ceil(cols);
    y += rows as f32 * (bh + gap) - gap + 12.0;

    // the matching actions
    let q = ed.act_search.trim().to_lowercase();
    let list: Vec<&'static Action> = offered_actions()
        .into_iter()
        .filter(|a| match ed.source {
            0 => true,
            1 => a.plugin.is_empty(),
            k => plugins().get(k - 2).map(|p| p.id == a.plugin).unwrap_or(false),
        })
        .filter(|a| {
            q.is_empty()
                || a.label.to_lowercase().contains(&q)
                || a.group.to_lowercase().contains(&q)
                || a.plugin_name.to_lowercase().contains(&q)
        })
        .collect();
    let box_h = 264.0;
    cv.fill(x, y, w, box_h, th.tile);
    cv.border(x, y, w, box_h, 3.0, th.ink);
    ed.list_rect = Some(cv.abs(x, y, w, box_h));
    let cols = if w > 460.0 { 2 } else { 1 };
    let gap = 6.0;
    let inner = w - 6.0 - 12.0 - 14.0;
    let cw = ((inner - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let ch = 52.0;
    let rows = list.len().div_ceil(cols);
    let total = 12.0 + rows as f32 * (ch + gap);
    let view = box_h - 6.0;
    let max = (total - view).max(0.0);
    ed.list_scroll = ed.list_scroll.min(max);
    let clip = cv.clip_push(x + 3.0, y + 3.0, w - 6.0, view);
    let top = y + 3.0 + 6.0 - ed.list_scroll;
    let mut pick: Option<&'static Action> = None;
    if list.is_empty() {
        cv.text(x + 14.0, y + 14.0, 19.0, 14.0, 400, th.slate, "No action matches. Try another word, or pick All.");
    }
    let ctx = cv.ui.ctx().clone();
    for (k, a) in list.iter().enumerate() {
        let (r, c) = (k / cols, k % cols);
        let ay = top + r as f32 * (ch + gap);
        if ay + ch < y || ay > y + box_h {
            continue;
        }
        let ax = x + 3.0 + 6.0 + c as f32 * (cw + gap);
        let resp = cv.hit(&format!("a_{}", a.id), ax, ay, cw, ch);
        let sel = ed.action == a.id;
        let hv = ctx.animate_bool_with_time(egui::Id::new(("act-row", a.id)), resp.hovered(), 0.12);
        let nudge = 3.0 * hv;
        cv.fill(ax, ay, cw, ch, if sel { th.mint } else { lerp(th.tile, th.hover, hv) });
        cv.border(ax, ay, cw, ch, 3.0, if sel || hv > 0.5 { th.ink } else { th.rule });
        cv.fill(ax + 8.0 + nudge, ay + 8.0, 36.0, 36.0, chip_bg(th, a.icon));
        cv.border(ax + 8.0 + nudge, ay + 8.0, 36.0, 36.0, 2.0, th.ink);
        cv.icon(a.icon, ax + 8.0 + nudge + 6.0, ay + 14.0, 24.0, th.ink);
        let fg = if sel { th.on_accent } else { th.ink };
        cv.text_fit(ax + 54.0 + nudge, ay + 7.0, 20.0, 14.0, 700, fg, a.label, cw - 62.0);
        let sub = if a.plugin.is_empty() { a.group } else { a.plugin_name };
        cv.text_fit(ax + 54.0 + nudge, ay + 27.0, 17.0, 12.0, 400, if sel { th.on_accent } else { th.slate }, sub, cw - 62.0);
        if resp.clicked() {
            pick = Some(a);
        }
    }
    cv.clip_pop(clip);
    if max > 0.0 {
        let tx = x + w - 3.0 - 14.0;
        cv.fill(tx, y + 3.0, 14.0, view, th.panel);
        cv.fill(tx + 6.0, y + 3.0, 2.0, view, th.ground_dot);
        let thh = (view * view / total).max(24.0);
        let ty = y + 3.0 + ed.list_scroll / max * (view - thh);
        cv.fill(tx + 3.0, ty + 3.0, 8.0, thh - 6.0, th.mint);
        cv.fill(tx + 7.0, ty + 3.0, 4.0, thh - 6.0, th.mint_dark);
        cv.border(tx + 3.0, ty + 3.0, 8.0, thh - 6.0, 2.0, th.ink);
    }
    y += box_h + 8.0;
    let hint = format!("{} actions · pick one and its settings show here", list.len());
    cv.text(x, y, 18.0, 13.0, 400, th.slate, &hint);
    y += 18.0;
    if let Some(a) = pick {
        ed.choose(a, now);
    }
    y - y0
}

/// Plugin pictures are drawn for light buttons (dark lines on clear), so their chip is light.
fn chip_bg(th: crate::ui::theme::Theme, icon: &str) -> Color32 {
    if icon.starts_with("png:") { Color32::from_rgb(0xdf, 0xe8, 0xf1) } else { th.panel }
}

/// The button's looks (states): a chip for each, + Add look, and the name and text of the one
/// being edited. Each press of the button shows the next look. Returns its height.
fn looks_strip(cv: &mut Cv, ed: &mut Editor, x: f32, y0: f32, w: f32, now: f64) -> f32 {
    let th = cv.th;
    let mut y = y0;
    let n = ed.looks.len();
    let help = if n == 1 {
        "One look. Add a look to make the button change each time you press it, like Mic on and Mic off."
    } else {
        "Each press shows the next look. Click a look to change its picture, colour and text."
    };
    y += cv.para(x, y, w, 18.0, 13.0, 400, th.slate, help) + 10.0;
    let (mut cx, mut cy) = (x, y);
    let chip_h = 52.0;
    let mut remove: Option<usize> = None;
    let mut select: Option<usize> = None;
    for i in 0..n {
        let l = ed.looks[i].clone();
        let cw = (cv.measure(13.0, 600, &l.name).max(cv.measure(11.0, 600, "Look 9")) + 52.0 + 16.0 + if i > 0 { 22.0 } else { 0.0 }).min(210.0);
        if cx > x && cx + cw > x + w {
            cx = x;
            cy += chip_h + 8.0;
        }
        let r = cv.hit(&format!("look{i}"), cx, cy, cw, chip_h);
        let sel = ed.sel == i;
        let up = if sel { -2.0 } else if r.hovered() { -1.0 } else { 0.0 };
        cv.shadow(cx, cy + up, cw, chip_h, if sel { 4.0 } else { 2.0 }, th.shadow);
        cv.fill(cx, cy + up, cw, chip_h, if sel { th.mint } else if r.hovered() { th.hover } else { th.tile });
        cv.border(cx, cy + up, cw, chip_h, 3.0, th.ink);
        // a tiny button showing that look
        cv.fill(cx + 8.0, cy + up + 8.0, 36.0, 36.0, th.accent(&l.colour));
        cv.border(cx + 8.0, cy + up + 8.0, 36.0, 36.0, 2.0, th.ink);
        let fg_icon = if l.colour == "tile" { th.ink } else { th.on_accent };
        cv.icon(&l.icon, cx + 16.0, cy + up + 16.0, 20.0, fg_icon);
        let fg = if sel { th.on_accent } else { th.ink };
        cv.text(cx + 52.0, cy + up + 8.0, 15.0, 11.0, 600, if sel { th.on_accent } else { th.slate }, &format!("Look {}", i + 1));
        cv.text_fit(cx + 52.0, cy + up + 25.0, 18.0, 13.0, 600, fg, &l.name, cw - 52.0 - if i > 0 { 26.0 } else { 8.0 });
        if i > 0 {
            // remove this look
            let (bx, by) = (cx + cw - 22.0, cy + up + 6.0);
            let rr = cv.hit(&format!("lookx{i}"), bx, by, 16.0, 16.0);
            let c = if rr.hovered() { th.coral_dark } else { fg };
            for k in 0..8 {
                cv.fill(bx + 3.0 + k as f32, by + 4.0 + k as f32, 2.0, 1.0, c);
                cv.fill(bx + 3.0 + (7 - k) as f32, by + 4.0 + k as f32, 2.0, 1.0, c);
            }
            if rr.clicked() {
                remove = Some(i);
            }
        }
        if r.clicked() && remove.is_none() {
            select = Some(i);
        }
        cx += cw + 8.0;
    }
    // + Add look
    if n < 8 {
        let aw = cv.measure(13.0, 600, "+ Add look") + 28.0;
        if cx > x && cx + aw > x + w {
            cx = x;
            cy += chip_h + 8.0;
        }
        let r = cv.hit("lookadd", cx, cy, aw, chip_h);
        if r.hovered() {
            cv.fill(cx, cy, aw, chip_h, th.hover);
        }
        cv.dashed(cx, cy, aw, chip_h, 3.0, if r.hovered() { th.ink } else { th.slate });
        cv.text_c(cx, aw, cy + 16.5, 19.0, 13.0, 600, th.ink, "+ Add look");
        if r.clicked() {
            let base = ed.looks[ed.sel].clone();
            let alt = ["coral", "mint", "lilac", "sun", "peach", "sky"];
            let colour = alt.iter().find(|c| !ed.looks.iter().any(|l| l.colour == **c)).unwrap_or(&"coral").to_string();
            ed.looks.push(LookDraft { name: if n == 1 { "On".into() } else { format!("Look {}", n + 1) }, media: base.media.clone(), text: String::new(), icon: base.icon, colour });
            ed.sel = ed.looks.len() - 1;
            ed.state_at = Some(now);
            if n == 1 && ed.looks[0].name == "Default" {
                ed.looks[0].name = "Off".into();
            }
        }
    }
    if let Some(i) = remove {
        ed.looks.remove(i);
        ed.sel = ed.sel.min(ed.looks.len() - 1);
        ed.state_at = Some(now);
    } else if let Some(i) = select {
        if ed.sel != i {
            ed.sel = i;
            ed.state_at = Some(now);
        }
    }
    y = cy + chip_h + 14.0;

    // the name of this look, and the text it shows on the button
    if ed.looks.len() > 1 {
        let half = (w - 12.0) / 2.0;
        cv.text(x, y, 19.0, 14.0, 600, th.ink, "Name of this look");
        cv.text(x + half + 12.0, y, 19.0, 14.0, 600, th.ink, "Text on the button");
        y += 24.0;
        let k = ed.sel;
        let mut nm = std::mem::take(&mut ed.looks[k].name);
        cv.text_field(&format!("lookname{k}"), x, y, half, 40.0, &mut nm, 15.0, "For example: Muted");
        ed.looks[k].name = nm;
        let mut tx = std::mem::take(&mut ed.looks[k].text);
        let ph = if ed.name.trim().is_empty() { "Same as the button name".to_string() } else { format!("Same as \"{}\"", ed.name.trim()) };
        cv.text_field(&format!("looktext{k}"), x + half + 12.0, y, half, 40.0, &mut tx, 15.0, &ph);
        ed.looks[k].text = tx;
        y += 40.0 + 12.0;
        let msg = format!("Picture and colour below are for \"{}\".", ed.looks[k].name);
        cv.text(x, y, 18.0, 13.0, 600, th.slate, &msg);
        y += 18.0;
    }
    y - y0
}
