//! The question asked before a plugin file is installed: what it is, who made it, whether it will
//! run, and Install or Cancel.
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};

use crate::app::App;
use crate::backend::signing::Trust;
use crate::ui::draw::{Cv, with_alpha};

pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut ask) = app.install_ask.take() else { return };
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
    let mut close = false;
    let mut install = false;

    let shade = ui.interact(win, egui::Id::new("ia-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "iashade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * p));
    }
    if shade.clicked() && !ask.busy {
        close = true;
    }

    let info = ask.info.clone();
    let (signer, signer_ok) = match &info.signature {
        Trust::Trusted { author } | Trust::User { author } => (format!("Signed by {author}, who this PC trusts."), true),
        Trust::Untrusted { author } => (format!("Signed by {author}, who this PC doesn't trust."), false),
        Trust::Unsigned => ("Not signed.".to_string(), false),
        Trust::Bad { why } => (why.clone(), false),
    };
    let damaged = matches!(info.signature, Trust::Bad { .. });
    let older = info.downgrade();
    let (dw, dh) = (520.0_f32, (330.0_f32 + if info.key.is_some() { 24.0 } else { 0.0 } + if older { 44.0 } else { 0.0 }).min(win.height() - 50.0));
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 10.0 * (1.0 - p);
    let dialog = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, dialog.min.to_vec2(), th, "installdlg");
    cv.hit("ia-body", 0.0, 0.0, dw, dh);
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    cv.text(50.0, 16.0, 24.0, 18.0, 700, light, if info.installed.is_some() { "Update a plugin" } else { "Install a plugin" });

    let (x, w) = (24.0, dw - 48.0);
    let mut y = 50.0 + 20.0;
    cv.text_fit(x, y, 28.0, 20.0, 700, th.ink, &info.name, w);
    y += 32.0;
    let buttons = if info.actions == 1 { "1 button".to_string() } else { format!("{} buttons", info.actions) };
    cv.text(x, y, 20.0, 14.0, 400, th.slate, &format!("Version {} · adds {buttons}", info.version));
    y += 30.0;
    cv.fill(x, y + 4.0, 10.0, 10.0, if signer_ok { th.mint } else { th.sun });
    y += cv.para(x + 18.0, y, w - 18.0, 19.0, 14.0, 600, th.ink, &signer) + 4.0;
    // the name in a signature can be anything: the key is what to compare with the author's page
    if let Some(k) = &info.key {
        cv.text(x + 18.0, y, 20.0, 14.0, 400, th.slate, "Key");
        cv.text(x + 18.0 + 40.0, y, 20.0, 14.0, 700, th.ink, k);
        y += 24.0;
    }
    y += 4.0;
    match (&info.why, damaged) {
        (_, true) => {
            y += cv.para(x, y, w, 19.0, 14.0, 400, th.coral, "It was changed after it was signed, so it won't be installed.") + 6.0;
        }
        (Some(why), _) => {
            y += cv.para(x, y, w, 19.0, 14.0, 400, th.slate, &format!("It installs, but it won't run yet. {why}")) + 6.0;
        }
        (None, _) => {
            y += cv.para(x, y, w, 19.0, 14.0, 400, th.slate, "It starts right after it installs.") + 6.0;
        }
    }
    if let Some(v) = &info.installed {
        if older {
            y += cv.para(x, y, w, 19.0, 14.0, 600, th.coral, &format!("This is an older version than the one installed ({v}). Install it only if you mean to go back.")) + 6.0;
        } else {
            y += cv.para(x, y, w, 19.0, 14.0, 400, th.slate, &format!("It replaces version {v}. Its saved settings stay.")) + 6.0;
        }
    }
    let _ = y;
    // Install and Cancel
    let by = dh - 24.0 - 44.0;
    let o = cv.opt_btn(15.0);
    let cw = cv.measure(15.0, 600, "Cancel") + 36.0;
    if cv.button("ia-cancel", dw - 24.0 - cw, by, cw, 44.0, "Cancel", o) && !ask.busy {
        close = true;
    }
    let label = if ask.busy { "Installing…" } else if older { "Install older" } else if info.installed.is_some() { "Update" } else { "Install" };
    let iw = cv.measure(15.0, 600, "Installing…").max(cv.measure(15.0, 600, label)) + 36.0;
    let mut po = cv.opt_btn(15.0);
    po.bg = th.mint;
    po.bg_hover = th.mint;
    po.fg = th.on_accent;
    po.disabled = ask.busy || damaged;
    if cv.button("ia-install", dw - 24.0 - cw - 10.0 - iw, by, iw, 44.0, label, po) {
        install = true;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ask.busy {
        close = true;
    }
    if close && ask.closing.is_none() {
        ask.closing = Some(now);
    }
    app.install_ask = Some(ask);
    if install {
        app.start_plugin_install(now);
    }
}
