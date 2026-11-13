//! The question asked before a plugin author's key (.mdkey) is trusted: who it says it is, the
//! key's fingerprint to compare with what the author published, and Trust or Cancel. A key file
//! that is double-clicked, handed over by a second launch or picked in Settings all come here:
//! nothing is trusted without the click.
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};

use crate::app::App;
use crate::ui::draw::{Cv, with_alpha};

pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut ask) = app.author_ask.take() else { return };
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
    let mut trust = false;

    let shade = ui.interact(win, egui::Id::new("aa-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "aashade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * p));
    }
    if shade.clicked() && !ask.busy {
        close = true;
    }

    let info = ask.info.clone();
    let (dw, dh) = (520.0_f32, 360.0_f32.min(win.height() - 50.0));
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 10.0 * (1.0 - p);
    let dialog = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, dialog.min.to_vec2(), th, "authordlg");
    cv.hit("aa-body", 0.0, 0.0, dw, dh);
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    cv.text(50.0, 16.0, 24.0, 18.0, 700, light, "Trust this plugin author?");

    let (x, w) = (24.0, dw - 48.0);
    let mut y = 50.0 + 20.0;
    cv.text_fit(x, y, 28.0, 20.0, 700, th.ink, &info.key.name, w);
    y += 32.0;
    cv.text(x, y, 20.0, 14.0, 400, th.slate, "Key");
    cv.text(x + 40.0, y, 20.0, 14.0, 700, th.ink, &info.fingerprint);
    y += 30.0;
    if info.known {
        y += cv.para(x, y, w, 19.0, 14.0, 400, th.slate, "This PC already trusts this key. Nothing changes.") + 6.0;
    } else {
        y += cv.para(x, y, w, 19.0, 14.0, 400, th.ink, "Plugins signed with this key will run on this PC without asking again. A plugin is a program: it can do anything you can do.") + 6.0;
        y += cv.para(x, y, w, 19.0, 14.0, 400, th.slate, "The name in a key file can be anything. Trust it only if the key above matches the one the author shows on their own page.") + 6.0;
    }
    if info.name_taken {
        y += cv.para(x, y, w, 19.0, 14.0, 600, th.coral, "Another key is already trusted under this name. This file may come from someone passing as them.") + 6.0;
    }
    let _ = y;

    let by = dh - 24.0 - 44.0;
    let o = cv.opt_btn(15.0);
    let cw = cv.measure(15.0, 600, "Cancel") + 36.0;
    if cv.button("aa-cancel", dw - 24.0 - cw, by, cw, 44.0, if info.known { "Close" } else { "Cancel" }, o) && !ask.busy {
        close = true;
    }
    if !info.known {
        let label = if ask.busy { "Adding…" } else { "Trust" };
        let tw = cv.measure(15.0, 600, "Adding…").max(cv.measure(15.0, 600, label)) + 36.0;
        // Trust is not the highlighted button: Cancel is the safe answer
        let mut po = cv.opt_btn(15.0);
        po.disabled = ask.busy;
        if cv.button("aa-trust", dw - 24.0 - cw - 10.0 - tw, by, tw, 44.0, label, po) {
            trust = true;
        }
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ask.busy {
        close = true;
    }
    if close && ask.closing.is_none() {
        ask.closing = Some(now);
    }
    app.author_ask = Some(ask);
    if trust {
        app.start_author_trust();
    }
}
