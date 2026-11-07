//! Asked when a plugin's ready-made profile has a new version but the user changed their copy
//! (or added it before profiles followed their plugin): update mine, keep mine, or add the new
//! one as another profile. Untouched profiles never ask: they are updated by themselves.
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};
use serde_json::Value;

use crate::app::App;
use crate::ui::draw::{Cv, with_alpha};

/// One profile waiting for an answer.
pub struct ProfileOffer {
    pub profile_id: String,
    pub profile_name: String,
    pub plugin_id: String,
    pub plugin_name: String,
    /// The new profile, as profile_io::import_from gives it, and its file's SHA-256
    pub bundle: Value,
    pub sha: String,
    /// Linked before (the user changed it) or added before profiles were linked
    pub linked: bool,
    pub opened: Option<f64>,
}

enum Answer {
    Update,
    Keep,
    Copy,
    Later,
}

pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    if app.profile_offers.is_empty() || app.editor.is_some() || app.install_ask.is_some() {
        return;
    }
    let th = app.theme();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let offer = &mut app.profile_offers[0];
    let opened = *offer.opened.get_or_insert(now);
    let p = crate::ui::widgets::steps(((now - opened) / 0.18) as f32, 4.0).clamp(0.0, 1.0);
    if p < 1.0 {
        ctx.request_repaint();
    }
    let (name, plugin, linked) = (offer.profile_name.clone(), offer.plugin_name.clone(), offer.linked);
    let more = app.profile_offers.len() - 1;
    let mut answer: Option<Answer> = None;

    ui.interact(win, egui::Id::new("pu-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "pushade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * p));
    }
    let (dw, dh) = (560.0_f32, 340.0_f32.min(win.height() - 50.0));
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 10.0 * (1.0 - p);
    let rect = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, rect.min.to_vec2(), th, "profupd");
    cv.hit("pu-body", 0.0, 0.0, dw, dh);
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    cv.text(50.0, 16.0, 24.0, 18.0, 700, light, "A profile has a new version");

    let (x, w) = (24.0, dw - 48.0);
    let mut y = 50.0 + 22.0;
    cv.text_fit(x, y, 28.0, 20.0, 700, th.ink, &name, w);
    y += 34.0;
    let why = if linked {
        format!("The {plugin} plugin's profile changed. You changed your \"{name}\" too, so it wasn't updated by itself.")
    } else {
        format!("The {plugin} plugin's profile is different from your \"{name}\". It may be older, or you may have changed it.")
    };
    y += cv.para(x, y, w, 20.0, 14.0, 400, th.ink, &why) + 10.0;
    y += cv.para(x, y, w, 19.0, 13.0, 400, th.slate, "Update mine puts the plugin's pages and buttons in its place (your name, colour, icon and app rule stay). A backup is kept either way.") + 6.0;
    if more > 0 {
        cv.text(x, y, 19.0, 13.0, 600, th.slate, &format!("{more} more after this one"));
    }

    // Update mine · Add as a new profile · Keep mine
    let by = dh - 24.0 - 44.0;
    let o = cv.opt_btn(14.0);
    let mut po = cv.opt_btn(14.0);
    po.bg = th.mint;
    po.bg_hover = th.mint;
    po.fg = th.on_accent;
    let w_up = cv.measure(14.0, 600, "Update mine") + 32.0;
    let w_cp = cv.measure(14.0, 600, "Add as a new profile") + 32.0;
    let w_keep = cv.measure(14.0, 600, "Keep mine") + 32.0;
    if cv.button("pu-keep", dw - 24.0 - w_keep, by, w_keep, 44.0, "Keep mine", o) {
        answer = Some(Answer::Keep);
    }
    if cv.button("pu-copy", dw - 24.0 - w_keep - 10.0 - w_cp, by, w_cp, 44.0, "Add as a new profile", o) {
        answer = Some(Answer::Copy);
    }
    if cv.button("pu-update", x, by, w_up, 44.0, "Update mine", po) {
        answer = Some(Answer::Update);
    }
    // Escape: ask again next time
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        answer = Some(Answer::Later);
    }
    let Some(a) = answer else { return };
    let offer = app.profile_offers.remove(0);
    match a {
        Answer::Update => match app.lib.replace_profile(&mut app.sample, &offer.profile_id, &offer.bundle, &offer.plugin_id, &offer.sha) {
            Ok(n) => app.say(&format!("{n} is the plugin's new version now. A backup was kept"), now),
            Err(e) => app.say(&format!("Couldn't update {name}: {e}"), now),
        },
        Answer::Keep => {
            app.lib.keep_profile(&offer.profile_id, &offer.plugin_id, &offer.sha);
            app.say(&format!("Kept your {name}. It won't ask about this version again"), now);
        }
        Answer::Copy => match app.lib.import_bundle(&mut app.sample, &offer.bundle) {
            Ok((n, _, _)) => {
                app.lib.link_last_profile(&offer.plugin_id, &offer.sha);
                app.lib.keep_profile(&offer.profile_id, &offer.plugin_id, &offer.sha);
                app.say(&format!("Added the new version as {n}. Yours is kept as it was"), now);
            }
            Err(e) => app.say(&format!("Couldn't add it: {e}"), now),
        },
        Answer::Later => {}
    }
}
