//! The password question for a password backup (Settings › Backup › Move to another PC): to save
//! one, a password typed twice; to restore one, the password. The file is chosen after OK.
use egui::{Color32, Rect, Sense, Ui, pos2, vec2};

use crate::app::App;
use crate::backend::backups::MIN_PASSWORD;
use crate::ui::draw::{Cv, with_alpha};

pub struct PortableAsk {
    /// true: save a password backup; false: restore one
    pub export: bool,
    pub password: String,
    pub again: String,
    pub error: Option<String>,
    pub busy: bool,
    pub opened: Option<f64>,
}

impl PortableAsk {
    pub fn new(export: bool) -> PortableAsk {
        PortableAsk { export, password: String::new(), again: String::new(), error: None, busy: false, opened: None }
    }
}

/// Why OK can't go ahead yet, or None when it can.
pub fn problem(export: bool, password: &str, again: &str) -> Option<String> {
    if password.chars().count() < MIN_PASSWORD {
        return Some(format!("Use at least {MIN_PASSWORD} characters."));
    }
    if export && password != again {
        return Some("The two passwords aren't the same.".into());
    }
    None
}

pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut ask) = app.portable.take() else { return };
    let th = app.theme();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let opened = *ask.opened.get_or_insert(now);
    let p = crate::ui::widgets::steps(((now - opened) / 0.18) as f32, 4.0).clamp(0.0, 1.0);
    if p < 1.0 {
        ctx.request_repaint();
    }
    let mut close = false;
    let mut go = false;

    let _ = ui.interact(win, egui::Id::new("pb-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "pbshade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * p));
    }
    let (dw, dh) = (520.0_f32, if ask.export { 400.0_f32 } else { 320.0 }.min(win.height() - 50.0));
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 10.0 * (1.0 - p);
    let dialog = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, dialog.min.to_vec2(), th, "portabledlg");
    cv.hit("pb-body", 0.0, 0.0, dw, dh);
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    cv.text(50.0, 16.0, 24.0, 18.0, 700, light, if ask.export { "Save a password backup" } else { "Restore a password backup" });

    let (x, w) = (24.0, dw - 48.0);
    let mut y = 70.0;
    let intro = if ask.export {
        "Your profiles, buttons and pictures, locked with a password. It opens in MinnionDeck on any PC. Without the password nobody can open it, and neither can you."
    } else {
        "Everything here now is replaced by the backup (a backup of it is made first, so this can be undone). Type the backup's password, then choose the file."
    };
    y += cv.para(x, y, w, 19.0, 14.0, 400, th.slate, intro) + 12.0;
    cv.password_field("pb-pass", x, y, w, 40.0, &mut ask.password, 15.0, "Password");
    y += 50.0;
    if ask.export {
        cv.password_field("pb-again", x, y, w, 40.0, &mut ask.again, 15.0, "The same password again");
        y += 50.0;
    }
    let why = problem(ask.export, &ask.password, &ask.again);
    let shown = ask.error.clone().or_else(|| why.clone().filter(|_| !ask.password.is_empty()));
    if let Some(e) = &shown {
        cv.para(x, y, w, 19.0, 14.0, 600, th.coral, e);
    }

    let by = dh - 24.0 - 44.0;
    let o = cv.opt_btn(15.0);
    let cw = cv.measure(15.0, 600, "Cancel") + 36.0;
    if cv.button("pb-cancel", dw - 24.0 - cw, by, cw, 44.0, "Cancel", o) && !ask.busy {
        close = true;
    }
    let label = if ask.busy { "Working…" } else if ask.export { "Choose where…" } else { "Choose the file…" };
    let ow = cv.measure(15.0, 600, label).max(cv.measure(15.0, 600, "Working…")) + 36.0;
    let mut po = cv.opt_btn(15.0);
    po.bg = th.mint;
    po.bg_hover = th.mint;
    po.fg = th.on_accent;
    po.disabled = ask.busy || why.is_some();
    if cv.button("pb-ok", dw - 24.0 - cw - 10.0 - ow, by, ow, 44.0, label, po) {
        go = true;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ask.busy {
        close = true;
    }
    if close {
        return;
    }
    if go && why.is_none() && !ask.busy {
        ask.busy = true;
        ask.error = None;
        let password = ask.password.clone();
        let export = ask.export;
        app.portable = Some(ask);
        if export {
            app.start_portable_export(password);
        } else {
            app.start_portable_import(password);
        }
        return;
    }
    app.portable = Some(ask);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_password_must_be_long_enough_and_typed_the_same_twice() {
        assert!(problem(true, "short", "short").unwrap().contains("at least"));
        assert!(problem(true, "long enough", "long enougj").unwrap().contains("same"));
        assert_eq!(problem(true, "long enough", "long enough"), None);
        // restoring asks it once
        assert_eq!(problem(false, "long enough", ""), None);
    }
}
