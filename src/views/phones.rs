//! Phones: the paired phones, pairing a new one, phone app updates, and a live phone preview
//! (landscape or portrait, any page, the buttons can be pressed to try them).
use crate::ui::anim;
use crate::app::App;
use crate::ui::draw::{Cv, with_alpha};
use crate::views::deck::{Face, tile_face};
use egui::Color32;

const STEPS: [&str; 3] = ["Scan this code with your phone's camera.", "Open the link it shows.", "Allow the phone here when it asks."];

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// How long the phone takes to turn between landscape and portrait.
const TURN: f64 = 0.5;

/// A box with pixel-rounded corners (two steps of `r`), its border `bw` thick.
fn px_round(cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, r: f32, col: Color32) {
    cv.fill(x + r, y, w - 2.0 * r, h, col);
    cv.fill(x, y + r, w, h - 2.0 * r, col);
    cv.fill(x + r / 2.0, y + r / 2.0, w - r, h - r, col);
}

fn px_round_boxed(cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, r: f32, bw: f32, border: Color32, fill: Color32) {
    px_round(cv, x, y, w, h, r, border);
    px_round(cv, x + bw, y + bw, w - 2.0 * bw, h - 2.0 * bw, (r - bw / 2.0).max(2.0), fill);
}

/// A section card: a bordered box with a bold title. Returns the y under the title.
fn section(cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, title: &str) -> f32 {
    let th = cv.th;
    cv.boxed(x, y, w, h, 4.0, th.ink, th.tile);
    cv.text(x + 18.0, y + 16.0, 24.0, 17.0, 700, th.ink, title);
    y + 16.0 + 24.0 + 12.0
}

pub fn draw(app: &mut App, cv: &mut Cv, w: f32, h: f32) -> f32 {
    let th = cv.th;
    let now = cv.now();
    cv.fill(0.0, 0.0, w, h.max(900.0) + 400.0, th.panel);
    let cw = w - 14.0 - 32.0;
    let x = 16.0;
    cv.text(x, 22.0, 28.0, 20.0, 700, th.ink, "Phones");
    cv.text(x, 54.0, 22.0, 15.0, 400, th.slate, "Use a phone or tablet as your deck. It shows the same pages and buttons as this PC.");

    let top = 92.0;
    let gap = 18.0;
    // left: phones, pairing, updates. right: the preview
    let left_w = (cw * 0.45).clamp(400.0, 520.0);
    let rx = x + left_w + gap;
    let rw = cw - left_w - gap;

    // ---- your phones ----------------------------------------------------------------------
    let phones = app.sample.phones.clone();
    let rows_h = if phones.is_empty() { 64.0 } else { phones.len() as f32 * 64.0 };
    let ph = 52.0 + rows_h + 14.0;
    let mut y = section(cv, x, top, left_w, ph, "Your phones");
    let online = phones.iter().filter(|p| p.online).count();
    if !phones.is_empty() {
        let s = format!("{online} of {} connected", phones.len());
        cv.text_r(x + left_w - 18.0, top + 19.0, 19.0, 13.0, 400, th.slate, &s);
    }
    if phones.is_empty() {
        cv.dashed(x + 18.0, y, left_w - 36.0, 52.0, 3.0, th.rule);
        cv.text_c(x + 18.0, left_w - 36.0, y + 16.0, 20.0, 14.0, 400, th.slate, "No phones yet. Pair one below.");
    }
    let mut forget: Option<usize> = None;
    for (i, p) in phones.iter().enumerate() {
        let ry = y + i as f32 * 64.0;
        if i > 0 {
            cv.fill(x + 18.0, ry - 6.0, left_w - 36.0, 2.0, th.rule);
        }
        // the device: a small phone or tablet drawing
        let (dw, dh) = if p.tablet { (40.0, 30.0) } else { (24.0, 40.0) };
        let (dx, dy) = (x + 18.0 + (40.0 - dw) / 2.0, ry + (52.0 - dh) / 2.0 - 2.0);
        px_round_boxed(cv, dx, dy, dw, dh, 4.0, 3.0, th.ink, if p.online { th.mint } else { th.panel });
        let tx = x + 18.0 + 40.0 + 14.0;
        cv.text_fit(tx, ry + 4.0, 22.0, 16.0, 700, th.ink, &p.name, left_w - 200.0);
        let sub = if p.online {
            let page = app.sample.pages.get(app.sample.page).map(|g| g.name.clone()).unwrap_or_default();
            format!("{} · showing {page}", p.model)
        } else {
            format!("{} · last seen {}", p.model, p.seen)
        };
        cv.text_fit(tx, ry + 27.0, 19.0, 13.0, 400, th.slate, &sub, left_w - 200.0);
        // status and Forget
        let bx = x + left_w - 18.0 - 76.0;
        let confirm = app.phone_forget == Some(i);
        let mut o = cv.opt_btn(13.0);
        o.bw = 3.0;
        o.shadow = 3.0;
        if confirm {
            o.bg = th.coral_dark;
            o.bg_hover = th.coral;
            o.fg = Color32::WHITE;
        }
        if cv.button(&format!("forget{i}"), bx, ry + 9.0, 76.0, 32.0, if confirm { "Sure?" } else { "Forget" }, o) {
            if confirm {
                forget = Some(i);
            } else {
                app.phone_forget = Some(i);
            }
        }
        let (dot, label) = if p.online { (th.mint, "Connected") } else { (th.slate, "Offline") };
        let lw = cv.measure(13.0, 600, label);
        let lx = bx - 14.0 - lw;
        cv.fill(lx - 14.0, ry + 20.0, 8.0, 8.0, dot);
        cv.text(lx, ry + 15.0, 19.0, 13.0, 600, if p.online { th.ink } else { th.slate }, label);
    }
    if let Some(i) = forget {
        let name = app.sample.phones[i].name.clone();
        let id = app.sample.phones[i].id.clone();
        app.forget_phone(&id);
        app.sample.phones.remove(i);
        app.phone_forget = None;
        app.say(&format!("Forgot {name}"), now);
    }
    y = top + ph + gap;

    // ---- pair a new phone ------------------------------------------------------------------
    let hub = app.phones.hub.clone();
    let pending = hub.as_ref().map(|h| h.pending()).unwrap_or_default();
    let ask_h = pending.len() as f32 * 64.0;
    let pair_h = 52.0 + ask_h + 164.0 + 8.0 + 44.0 + 44.0 + 30.0;
    let mut py = section(cv, x, y, left_w, pair_h, "Pair a new phone");
    // a phone asking to be allowed: compare the number, then Allow
    for p in &pending {
        cv.boxed(x + 18.0, py, left_w - 36.0, 54.0, 3.0, th.ink, th.tile);
        cv.fill(x + 21.0, py + 3.0, 6.0, 48.0, th.sun);
        cv.text_fit(x + 38.0, py + 6.0, 21.0, 15.0, 700, th.ink, &format!("{} wants to connect", p.name), left_w - 250.0);
        cv.text_fit(x + 38.0, py + 29.0, 19.0, 13.0, 400, th.slate, &format!("Is the number {} on the phone too?", p.number), left_w - 250.0);
        let mut o = cv.opt_btn(13.0);
        o.bw = 3.0;
        o.shadow = 3.0;
        let bx = x + left_w - 18.0 - 12.0 - 92.0 - 62.0;
        if cv.button(&format!("deny{}", p.conn), bx, py + 11.0, 62.0, 32.0, "No", o) {
            if let Some(h) = &hub {
                h.deny(p.conn);
            }
        }
        o.bg = th.mint;
        o.bg_hover = th.mint;
        o.fg = th.on_accent;
        if cv.button(&format!("allow{}", p.conn), bx + 70.0, py + 11.0, 92.0, 32.0, "Allow", o) {
            if let Some(h) = &hub {
                h.approve(p.conn);
                app.say(&format!("{} allowed", p.name), now);
            }
        }
        py += 64.0;
    }
    match &hub {
        Some(h) => {
            let url = h.url();
            let qr_data = app.phones.qr_for(&url).map(|(m, n)| (m.clone(), n));
            match qr_data {
                Some((modules, n)) => qr(cv, x + 18.0, py, 150.0, &modules, n),
                None => {
                    cv.text(x + 18.0, py + 60.0, 19.0, 13.0, 400, th.slate, "No QR code could be made.");
                }
            }
            let tx = x + 18.0 + 150.0 + 18.0;
            let tw = left_w - (tx - x) - 18.0;
            let mut ly = py;
            for (i, s) in STEPS.iter().enumerate() {
                cv.boxed(tx, ly, 26.0, 26.0, 3.0, th.ink, th.mint);
                cv.text_c(tx, 26.0, ly + 3.0, 20.0, 14.0, 700, th.on_accent, &format!("{}", i + 1));
                let hgt = cv.para(tx + 36.0, ly + 3.0, tw - 36.0, 18.0, 13.0, 400, th.ink, s);
                ly += hgt.max(26.0) + 10.0;
            }
            let cy = py + 164.0 + 8.0;
            let code = h.code();
            cv.text(x + 18.0, cy, 21.0, 14.0, 400, th.slate, "No camera? Open");
            let aw = cv.measure(14.0, 600, &h.base_url());
            cv.text_fit(x + 18.0 + 112.0, cy, 21.0, 14.0, 600, th.ink, &h.base_url(), left_w - 36.0 - 112.0);
            let _ = aw;
            cv.text(x + 18.0, cy + 22.0, 22.0, 14.0, 400, th.slate, "and type the code");
            cv.text(x + 18.0 + 140.0, cy + 18.0, 26.0, 20.0, 700, th.ink, &format!("{} {}", &code[..3], &code[3..]));
            // the same page over https, for a Wi-Fi you don't trust (the phone warns once)
            if let Some(s) = h.secure_url() {
                cv.text_fit(x + 18.0, cy + 44.0, 19.0, 12.0, 400, th.slate, &format!("On a Wi-Fi you don't trust, use {s} (the phone warns once)"), left_w - 36.0);
            }
            let wy = cy + 44.0 + if h.tls_port != 0 { 26.0 } else { 0.0 };
            cv.fill(x + 18.0, wy - 6.0, left_w - 36.0, 2.0, th.rule);
            let step = ((now * 2.5) as usize) % 4;
            for d in 0..3 {
                cv.fill(x + 18.0 + d as f32 * 10.0, wy + 12.0, 6.0, 6.0, if d < step { th.mint } else { with_alpha(th.slate, 0.4) });
            }
            cv.ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            let others = if h.addrs.len() > 1 { format!("  (other addresses: {})", h.addrs[1..].join(", ")) } else { String::new() };
            cv.text_fit(x + 18.0 + 38.0, wy + 6.0, 19.0, 13.0, 400, th.slate, &format!("Waiting for a phone on this Wi-Fi…{others}"), left_w - 36.0 - 38.0);
        }
        None => {
            let why = app.phones.error.clone().unwrap_or_else(|| "The phone page isn't running in this view.".into());
            cv.dashed(x + 18.0, py, left_w - 36.0, 80.0, 3.0, th.rule);
            cv.para(x + 30.0, py + 12.0, left_w - 60.0, 19.0, 14.0, 400, th.coral, &why);
        }
    }
    y += pair_h + gap;

    // ---- the phone page --------------------------------------------------------------------
    let up_h = 52.0 + 40.0 + 14.0 + 40.0 + 18.0;
    let uy = section(cv, x, y, left_w, up_h, "Nothing to install");
    cv.para(x + 18.0, uy, left_w - 36.0, 20.0, 14.0, 400, th.slate, "Phones and tablets use a page this PC serves on your Wi-Fi. Windows may ask once to allow Minniondeck on private networks: choose Allow.");
    let o = cv.opt_btn(14.0);
    let w1 = cv.measure(14.0, 600, "Copy the address") + 28.0;
    if cv.button("copyurl", x + 18.0, uy + 54.0, w1, 40.0, "Copy the address", o) {
        match &hub {
            Some(h) => {
                cv.ui.ctx().copy_text(h.base_url());
                app.say("Address copied", now);
            }
            None => app.say("The phone page isn't running", now),
        }
    }
    let left_bottom = y + up_h;

    // ---- the preview -----------------------------------------------------------------------
    let right_bottom = preview(app, cv, rx, top, rw, now);
    left_bottom.max(right_bottom) + 24.0
}

/// The preview card: orientation switch, the phone, and the page pager under it.
fn preview(app: &mut App, cv: &mut Cv, x: f32, y: f32, w: f32, now: f64) -> f32 {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let land = app.phone_land;
    // the phone's size (a 16:9 screen either way) fitted to the card
    let max_w = w - 36.0 - 2.0 * 18.0;
    let bezel = 14.0;
    let size = |land: bool| {
        let (scr_w, scr_h) = if land {
            let sw = max_w.min(520.0);
            (sw, (sw * 9.0 / 16.0).round())
        } else {
            (250.0_f32, (250.0_f32 * 16.0 / 9.0).round())
        };
        (scr_w + 2.0 * bezel + 8.0, scr_h + 2.0 * bezel + 8.0)
    };
    // turning (0.5s): the body morphs from the old shape to the new one, dipping a little in
    // the middle as if lifted; the screen fades out, swaps its layout while hidden, fades back in
    let t = app.phone_hold.unwrap_or(((now - app.phone_turn) / TURN) as f32).clamp(0.0, 1.0);
    if t < 1.0 {
        ctx.request_repaint();
    }
    let e = if t < 0.5 { 4.0 * t * t * t } else { 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0 };
    let (w0, h0) = size(!land);
    let (w1, h1) = size(land);
    let (pw, ph) = if t < 1.0 { (w0 + (w1 - w0) * e, h0 + (h1 - h0) * e) } else { (w1, h1) };
    let fade = if t < 1.0 { 1.0 - (2.0 * t - 1.0).abs() } else { 0.0 };
    let show_land = if t < 0.5 { !land } else { land };
    let card_h = 52.0 + ph + 24.0 + 40.0 + 14.0 + 22.0 + 18.0;
    let iy = section(cv, x, y, w, card_h, "Preview");

    // Landscape / Portrait
    let seg_w = 104.0;
    let sx0 = x + w - 18.0 - 2.0 * seg_w;
    for (k, label) in ["Landscape", "Portrait"].iter().enumerate() {
        let on = (k == 0) == land;
        let bx = sx0 + k as f32 * seg_w;
        let r = cv.hit(&format!("orient{k}"), bx, y + 12.0, seg_w, 32.0);
        cv.fill(bx, y + 12.0, seg_w, 32.0, if on { th.mint } else if r.hovered() { th.hover } else { th.tile });
        cv.border(bx, y + 12.0, seg_w, 32.0, 3.0, th.ink);
        cv.text_c(bx, seg_w, y + 12.0 + 6.0, 20.0, 13.0, if on { 700 } else { 600 }, if on { th.on_accent } else { th.ink }, label);
        if r.clicked() && !on {
            app.phone_land = k == 0;
            app.phone_turn = now;
        }
    }

    let k = 1.0 - 0.08 * (std::f32::consts::PI * t).sin();
    let (fw, fh) = ((pw * k).round(), (ph * k).round());
    let fx = (x + (w - fw) / 2.0).round();
    let fy = (iy + 6.0 + (ph - fh) / 2.0).round();
    phone(app, cv, fx, fy, fw, fh, bezel, now, show_land, fade);

    // pager under the phone: < Page name >  and a dot per page
    let pages: Vec<usize> = app.sample.pages.iter().enumerate().filter(|(_, p)| !p.folder).map(|(i, _)| i).collect();
    let cur = app.phone_page.unwrap_or(app.sample.page);
    let pos = pages.iter().position(|&p| p == cur).unwrap_or(0);
    let py = iy + 6.0 + ph + 20.0;
    let name = app.sample.pages.get(cur).map(|p| p.name.clone()).unwrap_or_default();
    let o = cv.opt_btn(14.0);
    let cx = x + w / 2.0;
    if cv.button("ppprev", cx - 130.0, py, 40.0, 40.0, "<", o) && pos > 0 {
        app.phone_page = Some(pages[pos - 1]);
        app.phone_flip = (now, -1.0);
    }
    if cv.button("ppnext", cx + 90.0, py, 40.0, 40.0, ">", o) && pos + 1 < pages.len() {
        app.phone_page = Some(pages[pos + 1]);
        app.phone_flip = (now, 1.0);
    }
    cv.text_c(cx - 80.0, 160.0, py + 1.0, 22.0, 16.0, 700, th.ink, &name);
    let n = pages.len();
    for d in 0..n {
        let dx = cx - (n as f32 * 14.0 - 6.0) / 2.0 + d as f32 * 14.0;
        let r = cv.hit(&format!("ppdot{d}"), dx - 3.0, py + 25.0, 14.0, 14.0);
        cv.fill(dx, py + 28.0, 8.0, 8.0, if d == pos { th.ink } else { with_alpha(th.ink, 0.3) });
        if r.clicked() && d != pos {
            app.phone_flip = (now, if d > pos { 1.0 } else { -1.0 });
            app.phone_page = Some(pages[d]);
        }
    }
    cv.text_c(x, w, py + 40.0 + 14.0, 19.0, 13.0, 400, th.slate, "This is what your phones show. Press a button to try it.");
    y + card_h
}

/// The phone: pixel-rounded body, the screen with a status bar, the page's buttons, page dots.
#[allow(clippy::too_many_arguments)]
fn phone(app: &mut App, cv: &mut Cv, x: f32, y: f32, pw: f32, ph: f32, bezel: f32, now: f64, land: bool, fade: f32) {
    let th = cv.th;
    let ctx = cv.ui.ctx().clone();
    let turn = 1.0 - fade;
    // the body: ink outline, a metal rim lit from the top left, a black bezel, the screen
    let rim = Color32::from_rgb(0x3b, 0x44, 0x58);
    let rim_hi = Color32::from_rgb(0x5d, 0x69, 0x83);
    let rim_lo = Color32::from_rgb(0x2a, 0x31, 0x40);
    let bez = Color32::from_rgb(0x0b, 0x0e, 0x14);
    let (ow, rw) = (4.0, 5.0);
    let r = 26.0;
    round_rect(cv, x + 6.0, y + 6.0, pw, ph, r, th.shadow);
    // side buttons stick out of the rim: volume on one side, power on the other
    let key = |cv: &mut Cv, bx: f32, by: f32, bw: f32, bh: f32| {
        cv.fill(bx, by, bw, bh, th.ink);
        cv.fill(bx + 2.0, by + 2.0, bw - 4.0, bh - 4.0, rim);
    };
    if fade > 0.5 {
        // mid-turn: no side buttons while the body is changing shape
    } else if land {
        key(cv, x + pw * 0.62, y - 5.0, 44.0, 9.0);
        key(cv, x + pw * 0.62 + 52.0, y - 5.0, 28.0, 9.0);
        key(cv, x + pw * 0.24, y + ph - 4.0, 52.0, 9.0);
    } else {
        key(cv, x - 5.0, y + ph * 0.16, 9.0, 34.0);
        key(cv, x - 5.0, y + ph * 0.16 + 42.0, 9.0, 34.0);
        key(cv, x + pw - 4.0, y + ph * 0.22, 9.0, 52.0);
    }
    round_rect(cv, x, y, pw, ph, r, th.ink);
    round_rect(cv, x + ow, y + ow, pw - 2.0 * ow, ph - 2.0 * ow, r - ow, rim_lo);
    // the light catches the top and left of the rim
    round_rect(cv, x + ow, y + ow, pw - 2.0 * ow - 2.0, ph - 2.0 * ow - 2.0, r - ow, rim_hi);
    round_rect(cv, x + ow + 2.0, y + ow + 2.0, pw - 2.0 * ow - 4.0, ph - 2.0 * ow - 4.0, r - ow - 2.0, rim);
    let bi = ow + rw;
    round_rect(cv, x + bi, y + bi, pw - 2.0 * bi, ph - 2.0 * bi, r - bi, bez);
    let (sx, sy) = (x + 4.0 + bezel, y + 4.0 + bezel);
    let (sw, sh) = (pw - 8.0 - 2.0 * bezel, ph - 8.0 - 2.0 * bezel);
    // the camera: a lens with a glint, in the bezel
    let (lx, ly) = if land { (x + bi + (sx - x - bi - 8.0) / 2.0, y + ph / 2.0 - 4.0) } else { (x + pw / 2.0 - 4.0, y + bi + (sy - y - bi - 8.0) / 2.0) };
    round_rect(cv, lx, ly, 8.0, 8.0, 2.0, Color32::from_rgb(0x22, 0x2a, 0x3a));
    cv.fill(lx + 2.0, ly + 2.0, 4.0, 4.0, Color32::from_rgb(0x14, 0x1a, 0x26));
    cv.fill(lx + 2.0, ly + 2.0, 2.0, 2.0, Color32::from_rgb(0x6f, 0x86, 0xb0));
    // the earpiece / speaker slot on the other short side
    if land {
        cv.fill(x + pw - bi - (sx - x - bi) / 2.0 - 2.0, y + ph / 2.0 - 18.0, 4.0, 36.0, Color32::from_rgb(0x1c, 0x22, 0x30));
    }
    cv.dots(sx, sy, sw, sh, th.ground, th.ground_dot);

    // status bar: time, profile and page, signal and battery
    let bar = 26.0;
    cv.fill(sx, sy, sw, bar, th.panel);
    cv.fill(sx, sy + bar - 2.0, sw, 2.0, th.ink);
    cv.text(sx + 10.0, sy + 5.0, 17.0, 12.0, 700, th.ink, "9:41");
    let prof = app.sample.profiles.get(app.sample.active).map(|p| p.name.clone()).unwrap_or_default();
    let cur = app.phone_page.unwrap_or(app.sample.page);
    let page = app.sample.pages.get(cur).map(|p| p.name.clone()).unwrap_or_default();
    let title = format!("{prof} / {page}");
    let tw = cv.measure(12.0, 600, &title).min(sw - 140.0);
    cv.text_fit(sx + (sw - tw) / 2.0, sy + 5.0, 17.0, 12.0, 600, th.ink, &title, sw - 140.0);
    let bx = sx + sw - 10.0 - 22.0;
    cv.border(bx, sy + 8.0, 20.0, 10.0, 2.0, th.ink);
    cv.fill(bx + 20.0, sy + 11.0, 2.0, 4.0, th.ink);
    cv.fill(bx + 3.0, sy + 11.0, 11.0, 4.0, th.mint);
    for b in 0..3 {
        let hh = 4.0 + b as f32 * 3.0;
        cv.fill(bx - 22.0 + b as f32 * 5.0, sy + 18.0 - hh, 3.0, hh, th.ink);
    }

    // the grid: square buttons fitted to the screen; page dots at the bottom
    let dots_h = 20.0;
    let pad = 10.0;
    // portrait turns the grid on its side (5 x 3 shows as 3 wide, 5 tall)
    let (dcols, drows) = (app.sample.cols.max(1), app.sample.rows.max(1));
    let (cols, rows) = if land { (dcols, drows) } else { (drows, dcols) };
    // wider than the 4px button shadow, so each button stands apart (light theme too)
    let gap = 10.0;
    let aw = sw - 2.0 * pad;
    let ah = sh - bar - dots_h - 2.0 * pad;
    let cell = ((aw - gap * (cols as f32 - 1.0)) / cols as f32).min((ah - gap * (rows as f32 - 1.0)) / rows as f32).floor();
    let gw = cols as f32 * cell + (cols as f32 - 1.0) * gap;
    let gh = rows as f32 * cell + (rows as f32 - 1.0) * gap;
    let gx = sx + (sw - gw) / 2.0;
    let gy = sy + bar + pad + (ah - gh) / 2.0;
    // a page change slides the buttons in from the side they come from
    let (f0, dir) = app.phone_flip;
    let fp = ease(((now - f0) / 0.22) as f32);
    if fp < 1.0 {
        ctx.request_repaint();
    }
    let slide = dir * 24.0 * (1.0 - fp);
    let clip = cv.clip_push(sx, sy + bar, sw, sh - bar);
    let step = cell + gap;
    let tiles: Vec<(usize, crate::backend::model::Tile)> = app
        .sample
        .tiles
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, t)| t.page == cur && t.col + t.w <= dcols && t.row + t.h <= drows)
        .map(|(i, mut t)| {
            if !land {
                (t.col, t.row, t.w, t.h) = (t.row, t.col, t.h, t.w);
            }
            (i, t)
        })
        .collect();
    let mut occ = vec![vec![false; cols]; rows];
    for (_, t) in &tiles {
        for r in t.row..t.row + t.h {
            for c in t.col..t.col + t.w {
                occ[r][c] = true;
            }
        }
    }
    for r in 0..rows {
        for c in 0..cols {
            if !occ[r][c] {
                cv.dashed(gx + c as f32 * step + slide, gy + r as f32 * step, cell, cell, 2.0, th.rule);
            }
        }
    }
    let span = |n: usize| n as f32 * cell + (n as f32 - 1.0) * gap;
    let mut anim_on = false;
    for (i, t) in &tiles {
        let (tx, ty) = (gx + t.col as f32 * step + slide, gy + t.row as f32 * step);
        let (tw_, th_) = (span(t.w), span(t.h));
        let resp = cv.hit(&format!("ptile{i}"), tx, ty, tw_, th_);
        let hv = ctx.animate_bool_with_time(egui::Id::new(("ptile-hover", *i)), resp.hovered(), 0.15);
        anim_on |= hv > 0.0 && hv < 1.0;
        let xf = app.phone_press.filter(|(j, _)| j == i).and_then(|(_, t0)| anim::eval(t.anim, now - t0));
        anim_on |= xf.is_some();
        let (label, icon, colour) = t.shown();
        let face = Face { media: Some(t.media_now()), colour, icon, label, dashed: false, hover: hv, down: resp.is_pointer_button_down_on(), xf, pop: 0.96 + 0.04 * turn };
        tile_face(cv, tx, ty, tw_, th_, &face);
        // the press: a white square that grows and fades, like on the deck
        if let Some((j, t0)) = app.phone_press {
            if j == *i {
                let p = ((now - t0) as f32 / 0.45).clamp(0.0, 1.0);
                if p < 1.0 {
                    let q = ease(p);
                    let (rw, rh) = ((tw_ - 8.0) * (0.2 + 0.8 * q), (th_ - 8.0) * (0.2 + 0.8 * q));
                    cv.border(tx + (tw_ - rw) / 2.0, ty + (th_ - rh) / 2.0, rw, rh, 3.0, with_alpha(Color32::WHITE, 0.45 * (1.0 - p)));
                    anim_on = true;
                }
            }
        }
        if resp.clicked() {
            app.phone_press = Some((*i, now));
        }
    }
    if anim_on {
        ctx.request_repaint();
    }
    // the screen's page dots
    let pages: Vec<usize> = app.sample.pages.iter().enumerate().filter(|(_, p)| !p.folder).map(|(i, _)| i).collect();
    let pos = pages.iter().position(|&p| p == cur).unwrap_or(0);
    let n = pages.len();
    let dy = sy + sh - dots_h + 6.0;
    for d in 0..n {
        let dx = sx + sw / 2.0 - (n as f32 * 12.0 - 6.0) / 2.0 + d as f32 * 12.0;
        if d == pos {
            cv.fill(dx, dy, 6.0, 6.0, th.ink);
        } else {
            cv.border(dx, dy, 6.0, 6.0, 2.0, with_alpha(th.ink, 0.5));
        }
    }
    cv.clip_pop(clip);
    // turning: the screen dims out and back in
    if fade > 0.0 {
        cv.fill(sx, sy, sw, sh, with_alpha(th.ground, fade));
    }
    // the screen has rounded corners too
    corner_mask(cv, sx, sy, sw, sh, 10.0, bez);
}

/// A rectangle with round corners drawn in 2px pixel steps (a quarter circle of radius `r`).
fn round_rect(cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, r: f32, col: Color32) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    let steps = (r / 2.0).ceil() as usize;
    for k in 0..steps {
        let yy = k as f32 * 2.0;
        let dy = r - yy - 1.0;
        let inset = (r - (r * r - dy * dy).max(0.0).sqrt()).round();
        let inset = (inset / 2.0).round() * 2.0;
        cv.fill(x + inset, y + yy, w - 2.0 * inset, 2.0, col);
        cv.fill(x + inset, y + h - yy - 2.0, w - 2.0 * inset, 2.0, col);
    }
    cv.fill(x, y + steps as f32 * 2.0, w, h - 4.0 * steps as f32, col);
}

/// Paints `col` over the four corners of a box, outside a quarter circle of radius `r`.
fn corner_mask(cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, r: f32, col: Color32) {
    let steps = (r / 2.0).ceil() as usize;
    for k in 0..steps {
        let yy = k as f32 * 2.0;
        let dy = r - yy - 1.0;
        let inset = ((r - (r * r - dy * dy).max(0.0).sqrt()) / 2.0).round() * 2.0;
        if inset <= 0.0 {
            continue;
        }
        for (cy, cx) in [(y + yy, x), (y + yy, x + w - inset), (y + h - yy - 2.0, x), (y + h - yy - 2.0, x + w - inset)] {
            cv.fill(cx, cy, inset, 2.0, col);
        }
    }
}

/// A stand-in QR code: three finder squares and a fixed scatter of modules.
fn qr(cv: &mut Cv, x: f32, y: f32, size: f32, modules: &[bool], n: usize) {
    let th = cv.th;
    // always dark on white, so any camera reads it
    cv.fill(x, y, size, size, Color32::WHITE);
    cv.border(x, y, size, size, 4.0, th.ink);
    let cell = ((size - 16.0) / n as f32).floor().max(1.0);
    let inner = cell * n as f32;
    let (ox, oy) = (x + (size - inner) / 2.0, y + (size - inner) / 2.0);
    let dark = Color32::from_rgb(0x1e, 0x24, 0x33);
    for r in 0..n {
        for c in 0..n {
            if modules[r * n + c] {
                cv.fill(ox + c as f32 * cell, oy + r as f32 * cell, cell, cell, dark);
            }
        }
    }
}
