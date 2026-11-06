//! Profiles: one simple card per profile (picture, name, what is in it, when it switches, Use and
//! Edit) and a "New profile" card. Everything else (name, colour, picture, when to switch,
//! duplicate, export, delete) is in the Edit profile window.
use crate::app::App;
use crate::backend::model::{AUTO_CHOICES, COLOURS, Profile};
use crate::ui::draw::{Cv, with_alpha};
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};

const P: &str = "A profile is a set of pages. Pick the one you want to use, or let Minniondeck switch for you when an app comes to the front.";
const FOOT: &str = "Profiles save automatically. Export one to share it as a .mdprofile file; double-click a .mdprofile file to add it here.";

/// The pictures a profile can have (8 x 8 pixel glyphs, like the rail).
pub const PROFILE_ICONS: [&str; 16] =
    ["star", "play", "note", "mic", "camera", "keyboard", "globe", "heart", "flag", "headset", "monitor", "home", "bolt", "brush", "broadcast", "clapper"];

/// The Edit profile window.
#[derive(Clone)]
pub struct ProfEdit {
    pub index: usize,
    pub opened: Option<f64>,
    pub closing: Option<f64>,
    /// Delete was clicked once: the button now asks to be clicked again.
    pub confirm_delete: bool,
    /// Put the cursor in the name field when it opens (a new profile).
    pub focus_name: bool,
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// "Switched by hand" or "When OBS Studio is in front".
fn when_text(p: &Profile) -> String {
    if p.auto == 0 { "Switched by hand".into() } else { format!("When {}", AUTO_CHOICES[p.auto]) }
}

fn use_profile(app: &mut App, i: usize, now: f64) {
    if app.sample.active != i {
        app.sample.active = i;
        app.page_at = now;
        let name = app.sample.profiles[i].name.clone();
        app.say(&format!("Switched to {name}"), now);
    }
}

fn open_edit(app: &mut App, i: usize, focus_name: bool) {
    app.prof_edit = Some(ProfEdit { index: i, opened: None, closing: None, confirm_delete: false, focus_name });
}

pub fn draw(app: &mut App, cv: &mut Cv, w: f32, h: f32) -> f32 {
    let th = cv.th;
    let now = cv.now();
    cv.fill(0.0, 0.0, w, h.max(900.0) + 2000.0, th.panel);
    let cw = w - 14.0 - 32.0; // content width inside the 16px padding, minus the scrollbar gutter
    let x = 16.0;

    // ---- head: title, one line on what a profile is, New and Import ----------------------
    cv.text(x, 22.0, 28.0, 20.0, 700, th.ink, "Profiles");
    let mut prim = cv.opt_btn(15.0);
    prim.bg = th.mint;
    prim.bg_hover = crate::ui::draw::lerp(th.mint, Color32::WHITE, 0.15);
    prim.fg = th.on_accent;
    let o = cv.opt_btn(15.0);
    let w_new = cv.measure(15.0, 600, "+ New profile") + 28.0;
    let w_imp = cv.measure(15.0, 600, "Import…") + 28.0;
    let right = x + cw;
    if cv.button("newprof", right - w_new, 16.0, w_new, 40.0, "+ New profile", prim) {
        new_profile(app, now);
    }
    if cv.button("import", right - w_new - 10.0 - w_imp, 16.0, w_imp, 40.0, "Import…", o) {
        app.start_import();
    }
    let lines = cv.para(x, 62.0, (cw - w_new - w_imp - 30.0).min(700.0), 22.0, 15.0, 400, th.slate, P);

    // ---- automatic switching: one clear row ----------------------------------------------
    let ay = 62.0 + lines + 14.0;
    let ah = 64.0;
    cv.boxed(x, ay, cw, ah, 4.0, th.ink, th.tile);
    cv.glyph("bolt", x + 18.0, ay + 20.0, 24.0, if app.auto_switch { th.sun } else { th.slate });
    cv.text(x + 56.0, ay + 11.0, 22.0, 16.0, 700, th.ink, "Switch automatically");
    let n_auto = app.sample.profiles.iter().filter(|p| p.auto != 0).count();
    let sub = if !app.auto_switch {
        "Off. Profiles change only when you pick one.".to_string()
    } else if n_auto == 0 {
        "On, but no profile has an app to switch on yet. Set one in Edit.".to_string()
    } else {
        format!("On. {} switch when their app comes to the front.", plural(n_auto, "profile"))
    };
    cv.text_fit(x + 56.0, ay + 34.0, 19.0, 13.0, 400, th.slate, &sub, cw - 56.0 - 90.0);
    if cv.switch("auto", x + cw - 16.0 - 60.0, ay + 12.0, app.auto_switch) {
        app.auto_switch = !app.auto_switch;
    }

    // ---- the cards -----------------------------------------------------------------------
    let cy = ay + ah + 20.0;
    cv.text_spaced(x, cy, 16.8, 12.0, 600, th.slate, "YOUR PROFILES", 0.72);
    let cy = cy + 28.0;
    let cols = if cw >= 760.0 { 3 } else { 2 };
    let gap = 14.0;
    let card_w = ((cw - gap * (cols as f32 - 1.0)) / cols as f32).floor();
    let card_h = 200.0;
    let n = app.sample.profiles.len();
    for i in 0..=n {
        let (r, c) = (i / cols, i % cols);
        let px = x + c as f32 * (card_w + gap);
        let py = cy + r as f32 * (card_h + gap);
        if i < n {
            card(app, cv, i, px, py, card_w, card_h, now);
        } else {
            new_card(app, cv, px, py, card_w, card_h, now);
        }
    }
    let rows = (n + 1).div_ceil(cols);
    let fy = cy + rows as f32 * card_h + (rows.saturating_sub(1)) as f32 * gap + 18.0;
    let fl = cv.para(x, fy, cw.min(700.0), 21.0, 14.0, 400, th.slate, FOOT);
    fy + fl + 16.0
}

fn new_profile(app: &mut App, now: f64) {
    let n = app.sample.profiles.len() + 1;
    let colour = COLOURS[app.sample.profiles.len() % COLOURS.len()].0;
    app.sample.profiles.push(Profile { id: String::new(), name: format!("Profile {n}"), colour: colour.to_string(), icon: "star".to_string(), auto: 0, pages: 1, folders: 0, buttons: 0 });
    app.fresh_profile = Some((app.sample.profiles.len() - 1, now));
    open_edit(app, app.sample.profiles.len() - 1, true);
}

#[allow(clippy::too_many_arguments)]
fn card(app: &mut App, cv: &mut Cv, i: usize, x: f32, y: f32, w: f32, h: f32, now: f64) {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let p = app.sample.profiles[i].clone();
    let active = i == app.sample.active;
    let resp = cv.hit(&format!("pcard{i}"), x, y, w, h);
    let hv = ctx.animate_bool_with_time(egui::Id::new(("pcard-hover", i)), resp.hovered(), 0.15);
    // a new card pops in (0.9 -> 1) and every card lifts a little when you point at it
    let mut lift = -2.0 * hv;
    if let Some((k, t0)) = app.fresh_profile {
        if k == i {
            let q = ((now - t0) / 0.3) as f32;
            if q < 1.0 {
                lift += 10.0 * (1.0 - ease(q));
                ctx.request_repaint();
            }
        }
    }
    if hv > 0.0 && hv < 1.0 {
        ctx.request_repaint();
    }
    let y = y + lift;
    cv.shadow(x, y, w, h, if active { 6.0 } else { 4.0 + 2.0 * hv }, th.shadow);
    cv.boxed(x, y, w, h, 4.0, th.ink, th.tile);
    if active {
        // the profile in use: a mint band across the top
        cv.fill(x + 4.0, y + 4.0, w - 8.0, 6.0, th.mint);
    }
    let ix = x + 18.0;
    let iw = w - 36.0;

    // picture, name, what is in it
    cv.boxed(ix, y + 22.0, 56.0, 56.0, 3.0, th.ink, th.accent(&p.colour));
    cv.glyph(&p.icon, ix + 12.0, y + 34.0, 32.0, th.on_accent);
    let tx = ix + 56.0 + 14.0;
    let badge_w = if active { cv.measure(12.0, 600, "In use") + 14.0 + 8.0 } else { 0.0 };
    cv.text_fit(tx, y + 26.0, 25.0, 18.0, 700, th.ink, &p.name, x + w - 18.0 - badge_w - tx);
    if active {
        cv.chip(x + w - 18.0 - badge_w + 8.0, y + 28.0, "In use", 12.0, th.mint, th.on_accent);
    }
    let stats = format!("{} · {}", plural(p.pages, "page"), plural(p.buttons, "button"));
    cv.text_fit(tx, y + 54.0, 19.0, 13.0, 400, th.slate, &stats, x + w - 18.0 - tx);

    // when it switches
    cv.fill(ix, y + 96.0, iw, 2.0, th.rule);
    let auto_on = p.auto != 0;
    cv.glyph(if auto_on { "bolt" } else { "people" }, ix, y + 110.0, 16.0, if auto_on && app.auto_switch { th.sun } else { th.slate });
    let mut when = when_text(&p);
    if auto_on && !app.auto_switch {
        when.push_str(" (paused)");
    }
    cv.text_fit(ix + 24.0, y + 108.0, 20.0, 14.0, 400, if auto_on { th.ink } else { th.slate }, &when, iw - 24.0);

    // Use and Edit
    let by = y + h - 18.0 - 42.0;
    let ew = 92.0;
    let uw = iw - ew - 8.0;
    if active {
        cv.fill(ix, by, uw, 42.0, with_alpha(th.mint, 0.18));
        cv.border(ix, by, uw, 42.0, 3.0, th.mint);
        let lw = cv.measure(15.0, 600, "In use") + 24.0;
        let lx = ix + (uw - lw) / 2.0;
        cv.glyph("check", lx, by + 13.0, 16.0, th.mint);
        cv.text(lx + 24.0, by + 10.5, 21.0, 15.0, 600, th.mint, "In use");
    } else {
        let mut prim = cv.opt_btn(15.0);
        prim.bg = th.mint;
        prim.bg_hover = crate::ui::draw::lerp(th.mint, Color32::WHITE, 0.15);
        prim.fg = th.on_accent;
        if cv.button(&format!("use{i}"), ix, by, uw, 42.0, "Use this profile", prim) {
            use_profile(app, i, now);
        }
    }
    if cv.button(&format!("edit{i}"), ix + uw + 8.0, by, ew, 42.0, "Edit", cv.opt_btn(15.0)) {
        open_edit(app, i, false);
    }
    if resp.double_clicked() {
        open_edit(app, i, false);
    }
}

#[allow(clippy::too_many_arguments)]
fn new_card(app: &mut App, cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, now: f64) {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let resp = cv.hit("pnew", x, y, w, h);
    let hv = ctx.animate_bool_with_time(egui::Id::new("pnew-hover"), resp.hovered(), 0.15);
    if hv > 0.0 && hv < 1.0 {
        ctx.request_repaint();
    }
    if hv > 0.0 {
        cv.fill(x, y, w, h, with_alpha(th.hover, hv));
    }
    cv.dashed(x, y, w, h, 3.0, if hv > 0.5 { th.ink } else { th.slate });
    let ps = 24.0 + 8.0 * hv;
    let (pcx, pcy) = (x + w / 2.0, y + h / 2.0 - 16.0);
    let pc = if hv > 0.5 { th.ink } else { th.slate };
    cv.fill((pcx - ps / 2.0).round(), (pcy - 2.0).round(), ps.round(), 4.0, pc);
    cv.fill((pcx - 2.0).round(), (pcy - ps / 2.0).round(), 4.0, ps.round(), pc);
    cv.text_c(x, w, pcy + 26.0, 22.0, 16.0, 700, pc, "New profile");
    cv.text_c(x, w, pcy + 50.0, 19.0, 13.0, 400, th.slate, "Starts with one empty page");
    if resp.clicked() {
        new_profile(app, now);
    }
}

/// The Edit profile window: drawn over everything, like the Edit button window.
pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut pe) = app.prof_edit.take() else { return };
    if pe.index >= app.sample.profiles.len() {
        return;
    }
    let th = app.theme();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let opened = *pe.opened.get_or_insert(now);
    let mut p = ease(((now - opened) / 0.18) as f32);
    if let Some(t0) = pe.closing {
        p = p.min(1.0 - ((now - t0) / 0.12) as f32);
        if p <= 0.0 {
            return;
        }
    }
    let p = p.clamp(0.0, 1.0);
    if p < 1.0 || pe.closing.is_some() {
        ctx.request_repaint();
    }
    let i = pe.index;
    let mut close = false;

    // the shade: blocks the page under it, a click on it closes the window
    let shade = ui.interact(win, egui::Id::new("pe-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "peshade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * p));
    }
    if shade.clicked() {
        close = true;
    }

    let (dw, dh) = (720.0_f32, 590.0_f32.min(win.height() - 50.0));
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 10.0 * (1.0 - p);
    let dialog = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, dialog.min.to_vec2(), th, "pdlg");
    // block clicks on the window itself from reaching the shade
    cv.hit("pe-body", 0.0, 0.0, dw, dh);

    // frame and title bar
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    cv.text(50.0, 16.0, 24.0, 18.0, 700, light, "Edit profile");
    let (cx, cy) = (dw - 4.0 - 10.0 - 34.0, 10.0);
    let r = cv.hit("peclose", cx, cy, 34.0, 32.0);
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

    // ---- left column: preview, name, colour, picture -------------------------------------
    let prof = app.sample.profiles[i].clone();
    let lx = 24.0;
    let lw = 360.0;
    let mut y = 50.0 + 22.0;
    cv.boxed(lx, y, 64.0, 64.0, 3.0, th.ink, th.accent(&prof.colour));
    cv.glyph(&prof.icon, lx + 12.0, y + 12.0, 40.0, th.on_accent);
    let shown = if prof.name.trim().is_empty() { "Untitled profile" } else { prof.name.as_str() };
    cv.text_fit(lx + 80.0, y + 8.0, 27.0, 20.0, 700, th.ink, shown, lw - 80.0);
    let stats = format!("{} · {} · {}", plural(prof.pages, "page"), plural(prof.folders, "folder"), plural(prof.buttons, "button"));
    cv.text_fit(lx + 80.0, y + 38.0, 19.0, 13.0, 400, th.slate, &stats, lw - 80.0);
    y += 64.0 + 22.0;

    cv.text(lx, y, 19.0, 14.0, 600, th.ink, "Name");
    let mut name = prof.name.clone();
    if pe.focus_name {
        pe.focus_name = false;
        cv.ui.memory_mut(|m| m.request_focus(egui::Id::new(("pdlg", "pname"))));
    }
    cv.text_field("pname", lx, y + 24.0, lw, 40.0, &mut name, 15.0, "Profile name");
    if name != prof.name {
        app.sample.profiles[i].name = name;
    }
    y += 24.0 + 40.0 + 18.0;

    let cname = COLOURS.iter().find(|c| c.0 == prof.colour).map(|c| c.1).unwrap_or("Mint");
    cv.text(lx, y, 19.0, 14.0, 600, th.ink, &format!("Colour: {cname}"));
    for (k, (key, _)) in COLOURS.iter().enumerate() {
        let (sx, sy) = (lx + k as f32 * 48.0, y + 24.0);
        let resp = cv.hit(&format!("pcol{k}"), sx, sy, 40.0, 40.0);
        cv.fill(sx, sy, 40.0, 40.0, th.accent(key));
        cv.border(sx, sy, 40.0, 40.0, 3.0, if resp.hovered() { th.focus } else { th.ink });
        if *key == prof.colour {
            cv.border(sx + 3.0, sy + 3.0, 34.0, 34.0, 3.0, th.tile);
            cv.glyph("check", sx + 12.0, sy + 12.0, 16.0, th.on_accent);
        }
        if resp.clicked() {
            app.sample.profiles[i].colour = key.to_string();
        }
    }
    y += 24.0 + 40.0 + 18.0;

    cv.text(lx, y, 19.0, 14.0, 600, th.ink, "Picture");
    for (k, icon) in PROFILE_ICONS.iter().enumerate() {
        let (sx, sy) = (lx + (k % 8) as f32 * 45.0, y + 24.0 + (k / 8) as f32 * 45.0);
        let resp = cv.hit(&format!("pico{k}"), sx, sy, 40.0, 40.0);
        let on = *icon == prof.icon;
        let bg = if on { th.accent(&prof.colour) } else if resp.hovered() { th.hover } else { th.tile };
        cv.fill(sx, sy, 40.0, 40.0, bg);
        cv.border(sx, sy, 40.0, 40.0, 3.0, th.ink);
        cv.glyph(icon, sx + 12.0, sy + 12.0, 16.0, if on { th.on_accent } else { th.ink });
        if resp.clicked() {
            app.sample.profiles[i].icon = icon.to_string();
        }
    }

    // ---- right column: when to switch, and the profile's tools ---------------------------
    let rx = lx + lw + 32.0;
    let rw = dw - rx - 24.0;
    cv.fill(rx - 16.0, 50.0 + 22.0, 2.0, dh - 50.0 - 22.0 - 80.0, th.rule);
    let mut y = 50.0 + 22.0;
    cv.text(rx, y, 19.0, 14.0, 600, th.ink, "Switch to this profile");
    y += 26.0;
    for (k, choice) in AUTO_CHOICES.iter().enumerate() {
        let label = if k == 0 { "Only when I pick it".to_string() } else { format!("When {choice}") };
        let resp = cv.hit(&format!("pauto{k}"), rx, y, rw, 30.0);
        let on = prof.auto == k;
        if on {
            cv.fill(rx, y, rw, 30.0, with_alpha(th.mint, 0.18));
        } else if resp.hovered() {
            cv.fill(rx, y, rw, 30.0, th.hover);
        }
        // a square radio
        cv.boxed(rx + 8.0, y + 6.0, 18.0, 18.0, 3.0, th.ink, if on { th.mint } else { th.tile });
        if on {
            cv.fill(rx + 14.0, y + 12.0, 6.0, 6.0, th.on_accent);
        }
        cv.text_fit(rx + 36.0, y + 4.5, 21.0, 14.0, if on { 600 } else { 400 }, th.ink, &label, rw - 44.0);
        if resp.clicked() {
            app.sample.profiles[i].auto = k;
        }
        y += 32.0;
    }
    if !app.auto_switch && prof.auto != 0 {
        cv.para(rx, y + 2.0, rw, 18.0, 12.0, 400, th.slate, "Switch automatically is off, so this waits until you turn it on.");
    }
    let y = 50.0 + 22.0 + 26.0 + AUTO_CHOICES.len() as f32 * 32.0 + 44.0;
    cv.text(rx, y, 19.0, 14.0, 600, th.ink, "More");
    let half = (rw - 8.0) / 2.0;
    let o = cv.opt_btn(14.0);
    if cv.button("pdup", rx, y + 26.0, half, 38.0, "Duplicate", o) {
        let mut c = app.sample.profiles[i].clone();
        c.name = format!("{} copy", c.name);
        c.auto = 0;
        app.sample.profiles.insert(i + 1, c);
        if app.sample.active > i {
            app.sample.active += 1;
        }
        app.fresh_profile = Some((i + 1, now));
        app.say("Profile duplicated", now);
    }
    if cv.button("pexp", rx + half + 8.0, y + 26.0, half, 38.0, "Export", o) {
        app.start_export(i, now);
    }
    let active = i == app.sample.active;
    let mut del = o;
    del.disabled = active;
    if pe.confirm_delete {
        del.bg = th.coral_dark;
        del.bg_hover = th.coral;
        del.fg = Color32::WHITE;
    } else {
        del.fg = th.coral;
    }
    let dl = if pe.confirm_delete { "Click again to delete" } else { "Delete profile" };
    if cv.button("pdel", rx, y + 26.0 + 38.0 + 10.0, rw, 38.0, dl, del) {
        if pe.confirm_delete {
            let name = app.sample.profiles[i].name.clone();
            app.sample.profiles.remove(i);
            if app.sample.active > i {
                app.sample.active -= 1;
            }
            app.say(&format!("Deleted {name}"), now);
            return;
        }
        pe.confirm_delete = true;
    }
    if active {
        let msg = if app.sample.profiles.len() == 1 { "Your only profile can't be deleted." } else { "This profile is in use. Use another one to delete it." };
        cv.para(rx, y + 26.0 + 2.0 * 38.0 + 18.0, rw, 18.0, 12.0, 400, th.slate, msg);
    }

    // ---- footer: Use and Done ------------------------------------------------------------
    let fy = dh - 4.0 - 64.0;
    cv.fill(4.0, fy, dw - 8.0, 2.0, th.rule);
    let mut prim = cv.opt_btn(15.0);
    prim.bg = th.mint;
    prim.bg_hover = crate::ui::draw::lerp(th.mint, Color32::WHITE, 0.15);
    prim.fg = th.on_accent;
    let w_done = 110.0;
    if cv.button("pdone", dw - 24.0 - w_done, fy + 14.0, w_done, 40.0, "Done", prim) {
        close = true;
    }
    if !active {
        let w_use = cv.measure(15.0, 600, "Use this profile") + 28.0;
        if cv.button("puse", dw - 24.0 - w_done - 10.0 - w_use, fy + 14.0, w_use, 40.0, "Use this profile", cv.opt_btn(15.0)) {
            use_profile(app, i, now);
        }
    }
    cv.text(24.0, fy + 24.0, 19.0, 13.0, 400, th.slate, "Changes save as you make them.");

    let typing = ctx.memory(|m| m.focused().is_some());
    if ctx.input(|inp| inp.key_pressed(egui::Key::Escape)) || (!typing && ctx.input(|inp| inp.key_pressed(egui::Key::Enter))) {
        close = true;
    }
    if ctx.input(|inp| inp.key_pressed(egui::Key::Enter)) && typing {
        ctx.memory_mut(|m| m.surrender_focus(egui::Id::new(("pdlg", "pname"))));
    }
    if close && pe.closing.is_none() {
        if app.sample.profiles[i].name.trim().is_empty() {
            app.sample.profiles[i].name = "Untitled profile".into();
        }
        ctx.memory_mut(|m| m.surrender_focus(egui::Id::new(("pdlg", "pname"))));
        pe.closing = Some(now);
    }
    app.prof_edit = Some(pe);
}
