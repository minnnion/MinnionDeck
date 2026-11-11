//! A profile with choices (a plugin's profile with a page for each service, like OBS Studio and
//! Twitch): which pages to add, as tick boxes, before it is added. Pages that belong to no choice
//! are always added.
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};
use serde_json::Value;

use crate::app::{App, View};
use crate::ui::draw::{Cv, with_alpha};

pub struct ChoosePages {
    /// The profile as profile_io hands it over, with all its pages
    pub bundle: Value,
    /// (plugin, its profile file's SHA-256) when it's a plugin's profile, so it updates with it
    pub plugin: Option<(String, String)>,
    pub choices: Vec<(String, String)>,
    pub ticks: Vec<bool>,
    pub opened: Option<f64>,
    pub closing: Option<f64>,
}

impl ChoosePages {
    pub fn new(bundle: Value, plugin: Option<(String, String)>) -> ChoosePages {
        let choices = crate::backend::library::choices_of(&bundle);
        let ticks = vec![true; choices.len()];
        ChoosePages { bundle, plugin, choices, ticks, opened: None, closing: None }
    }
}

pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut ask) = app.choose_pages.take() else { return };
    let th = app.theme();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let opened = *ask.opened.get_or_insert(now);
    let mut p = crate::ui::widgets::steps(((now - opened) / 0.18) as f32, 4.0).clamp(0.0, 1.0);
    if let Some(t0) = ask.closing {
        p = p.min(1.0 - ((now - t0) / 0.12) as f32);
        if p <= 0.0 {
            return;
        }
    }
    let p = p.clamp(0.0, 1.0);
    if p < 1.0 || ask.closing.is_some() {
        ctx.request_repaint();
    }
    let (mut close, mut add) = (false, false);

    let shade = ui.interact(win, egui::Id::new("cp-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "cpshade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * p));
    }
    if shade.clicked() {
        close = true;
    }

    let name = ask.bundle["profile"]["name"].as_str().unwrap_or("Profile").to_string();
    let row_h = 52.0;
    let (dw, dh) = (480.0_f32, (50.0 + 24.0 + 30.0 + 44.0 + ask.choices.len() as f32 * (row_h + 8.0) + 20.0 + 40.0 + 24.0).min(win.height() - 50.0));
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 10.0 * (1.0 - p);
    let dialog = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, dialog.min.to_vec2(), th, "choosepages");
    cv.hit("cp-body", 0.0, 0.0, dw, dh);
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    cv.text_fit(50.0, 16.0, 24.0, 18.0, 700, light, &format!("Add {name}"), dw - 80.0);

    let (x, w) = (24.0, dw - 48.0);
    let mut y = 50.0 + 24.0;
    let q = ask.bundle["profile"]["choose"].as_str().unwrap_or("Which pages do you want?").to_string();
    cv.text_fit(x, y, 26.0, 18.0, 700, th.ink, &q, w);
    y += 30.0;
    y += cv.para(x, y, w, 18.0, 13.0, 400, th.slate, "Each one adds its own page. You can add the others later by adding the profile again.") + 14.0;
    for (i, (_, label)) in ask.choices.iter().enumerate() {
        let r = cv.hit(&format!("cp-{i}"), x, y, w, row_h);
        let on = ask.ticks[i];
        cv.boxed(x, y, w, row_h, 3.0, th.ink, if on { with_alpha(th.mint, 0.18) } else if r.hovered() { th.hover } else { th.tile });
        // the tick box
        cv.boxed(x + 14.0, y + 14.0, 24.0, 24.0, 3.0, th.ink, if on { th.mint } else { th.tile });
        if on {
            cv.glyph("check", x + 18.0, y + 18.0, 16.0, th.on_accent);
        }
        cv.text_fit(x + 52.0, y + 15.0, 22.0, 16.0, 600, th.ink, label, w - 66.0);
        if r.clicked() {
            ask.ticks[i] = !on;
        }
        y += row_h + 8.0;
    }
    let any = ask.ticks.iter().any(|t| *t);
    let by = dh - 24.0 - 40.0;
    let mut o = cv.opt_btn(14.0);
    o.bw = 3.0;
    o.shadow = 3.0;
    let mut ao = o;
    if any {
        ao.bg = th.mint;
        ao.bg_hover = th.mint;
        ao.fg = th.on_accent;
    } else {
        ao.disabled = true;
    }
    let aw = cv.measure(14.0, 600, "Add") + 48.0;
    if cv.button("cp-add", dw - 24.0 - aw, by, aw, 40.0, "Add", ao) {
        add = true;
    }
    let cw = cv.measure(14.0, 600, "Cancel") + 32.0;
    if cv.button("cp-cancel", dw - 24.0 - aw - 10.0 - cw, by, cw, 40.0, "Cancel", o) {
        close = true;
    }
    if !any {
        cv.text_fit(x, by + 10.0, 20.0, 13.0, 400, th.slate, "Tick at least one.", w - aw - cw - 30.0);
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }

    if add && ask.closing.is_none() {
        let picked: Vec<String> = ask.choices.iter().zip(&ask.ticks).filter(|(_, t)| **t).map(|(c, _)| c.0.clone()).collect();
        let chosen = crate::backend::library::with_choices(&ask.bundle, &picked);
        // a profile file imported by hand belongs to its one plugin too, so its picks count
        let owner = ask.plugin.clone().or_else(|| {
            let names = ask.bundle["plugins"].as_array().filter(|n| n.len() == 1)?;
            let name = names[0].as_str()?;
            app.plug.rows.iter().find(|r| r.name == name).map(|r| (r.id.clone(), String::new()))
        });
        let done = match &owner {
            Some((plugin, sha)) => app.lib.add_chosen_profile(&mut app.sample, plugin, sha, &picked, &chosen),
            None => app.lib.import_bundle(&mut app.sample, &chosen).map(|(name, _, _)| name),
        };
        match done {
            Ok(name) => {
                let at = app.sample.profiles.iter().position(|p| p.name == name).unwrap_or(app.sample.profiles.len().saturating_sub(1));
                app.fresh_profile = Some((at, now));
                app.pending_view = Some(View::Profiles);
                app.say(&format!("Added {name}"), now);
            }
            Err(e) => app.say(&format!("Could not add it: {e}"), now),
        }
        close = true;
    }
    if close && ask.closing.is_none() {
        ask.closing = Some(now);
    }
    app.choose_pages = Some(ask);
}
