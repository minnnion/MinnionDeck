//! A plugin's window, opened from Settings › Plugins: whether it runs (and why not), turning it
//! off or on, its own settings (the fields its manifest lists), and Remove.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, Rect, Sense, Ui, pos2, vec2};
use serde_json::Value;

use crate::app::App;
use crate::backend::plugins;
use crate::backend::signing::Trust;
use crate::ui::draw::{Cv, with_alpha};

pub struct PlugDialog {
    id: String,
    /// The settings fields (as the plugin's manifest lists them) and what each has now
    fields: Vec<Value>,
    values: HashMap<String, String>,
    /// Which passwords are already saved
    saved_secret: HashMap<String, bool>,
    opened: Option<f64>,
    closing: Option<f64>,
    /// What the threads said, shown under the buttons
    msg: Arc<Mutex<String>>,
    remove_armed: bool,
    /// When Remove was armed (it shakes once)
    armed_at: Option<f64>,
    /// How tall the last frame drew it
    height: f32,
    /// The message last shown, and when it changed (it slides in)
    msg_seen: String,
    msg_at: f64,
    /// The values as last saved, so Save lights up only when something changed
    initial: HashMap<String, String>,
}

impl PlugDialog {
    pub fn new(id: &str) -> PlugDialog {
        let mut d = PlugDialog { id: id.to_string(), fields: vec![], values: HashMap::new(), saved_secret: HashMap::new(), opened: None, closing: None, msg: Arc::new(Mutex::new(String::new())), remove_armed: false, armed_at: None, height: 420.0, msg_seen: String::new(), msg_at: 0.0, initial: HashMap::new() };
        if let Ok(v) = plugins::settings_view(id) {
            d.fields = v.get("fields").and_then(|f| f.as_array()).cloned().unwrap_or_default();
            if let Some(vals) = v.get("values").and_then(|v| v.as_object()) {
                for f in &d.fields {
                    let key = f.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string();
                    match (f.get("kind").and_then(|k| k.as_str()), vals.get(&key)) {
                        (Some("password"), Some(v)) => {
                            d.saved_secret.insert(key, v.get("set").and_then(|s| s.as_bool()).unwrap_or(false));
                        }
                        (_, Some(Value::String(s))) => {
                            d.values.insert(key, s.clone());
                        }
                        (_, Some(Value::Bool(b))) => {
                            d.values.insert(key, b.to_string());
                        }
                        (_, Some(Value::Number(n))) => {
                            d.values.insert(key, n.to_string());
                        }
                        _ => {}
                    }
                }
            }
        }
        for f in &d.fields {
            if f.get("kind").and_then(|k| k.as_str()) == Some("select") {
                let key = f.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string();
                let first = f.get("options").and_then(|o| o.as_array()).and_then(|o| o.first()).and_then(|o| o.as_str()).unwrap_or("").to_string();
                d.values.entry(key).or_insert(first);
            }
        }
        d.initial = d.values.clone();
        d
    }

    fn say(&self, text: &str) {
        *self.msg.lock().unwrap_or_else(|e| e.into_inner()) = text.to_string();
    }
}

fn s<'a>(f: &'a Value, k: &str) -> &'a str {
    f.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

/// Stepped (pixel-style) progress of something that started at `t0` and takes `secs`.
fn stepped(now: f64, t0: f64, secs: f64) -> f32 {
    crate::ui::widgets::steps(((now - t0) / secs) as f32, 4.0).clamp(0.0, 1.0)
}

/// A card: a bordered panel with a small heading. Returns where its rows start.
fn card(cv: &mut Cv, x: f32, y: f32, w: f32, h: f32, title: &str) -> f32 {
    let th = cv.th;
    cv.shadow(x, y, w, h, 4.0, th.shadow);
    cv.boxed(x, y, w, h, 3.0, th.ink, th.tile);
    cv.text_spaced(x + 16.0, y + 12.0, 16.0, 11.0, 700, th.slate, title, 1.0);
    y + 36.0
}

/// A row in a card: its name and the help under it (wrapped), on the left. Returns its height.
fn row_text(cv: &mut Cv, x: f32, y: f32, w: f32, name: &str, help: &str) -> f32 {
    let th = cv.th;
    cv.text_fit(x, y, 20.0, 15.0, 600, th.ink, name, w);
    if help.is_empty() {
        return 20.0;
    }
    21.0 + cv.para(x, y + 21.0, w, 16.0, 12.0, 400, th.slate, help)
}

/// How tall the help under a row's name is.
fn help_h(cv: &Cv, w: f32, help: &str) -> f32 {
    if help.is_empty() { 0.0 } else { 3.0 + cv.wrap_lines(w, 12.0, 400, help) as f32 * 16.0 }
}

pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut pd) = app.plug_dialog.take() else { return };
    let th = app.theme();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let opened = *pd.opened.get_or_insert(now);
    let mut p = stepped(now, opened, 0.18);
    if let Some(t0) = pd.closing {
        p = p.min(1.0 - ((now - t0) / 0.12) as f32);
        if p <= 0.0 {
            return;
        }
    }
    let p = p.clamp(0.0, 1.0);
    // the sections come in one after another, then the status keeps a slow pulse
    let intro_done = now - opened > 0.6;
    if !intro_done || pd.closing.is_some() {
        ctx.request_repaint();
    }
    let mut close = false;

    let shade = ui.interact(win, egui::Id::new("pd-blocker"), Sense::click_and_drag());
    {
        let mut cv = Cv::new(ui, win, win.min.to_vec2(), th, "pdshade");
        cv.fill(0.0, 0.0, win.width(), win.height(), with_alpha(th.shadow, 0.6 * p));
    }
    if shade.clicked() {
        close = true;
    }

    let info = app.plug.rows.iter().find(|r| r.id == pd.id).cloned();
    let market = crate::backend::market::plugins().iter().find(|m| m.id == pd.id);
    let icon = crate::backend::catalog::plugins().iter().find(|c| c.id == pd.id).map(|c| c.icon).unwrap_or("bolt");
    let colour = market.map(|m| m.colour).unwrap_or("mint");
    let settings: Vec<Value> = pd.fields.clone();
    let visible: Vec<Value> = settings.iter().filter(|f| !f.get("hidden").and_then(|h| h.as_bool()).unwrap_or(false)).cloned().collect();

    // the height is what the last frame drew (the first frame guesses)
    let (dw, dh) = (600.0_f32, pd.height.clamp(200.0, win.height() - 50.0));
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 12.0 * (1.0 - p);
    let dialog = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, dialog.min.to_vec2(), th, "plugdlg");
    cv.hit("pd-body", 0.0, 0.0, dw, dh);
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);

    // the title bar
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    let name = info.as_ref().map(|i| i.name.clone()).unwrap_or_else(|| pd.id.clone());
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    cv.text_fit(50.0, 16.0, 24.0, 18.0, 700, light, "Plugin", dw - 120.0);
    let (cx, cy) = (dw - 4.0 - 10.0 - 34.0, 10.0);
    let r = cv.hit("pdclose", cx, cy, 34.0, 32.0);
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

    let (x, w) = (24.0, dw - 48.0);
    let mut y = 50.0 + 22.0;
    // each section: how far in it is (0 = not yet), and how far below its place it still sits
    let section = |i: usize| -> f32 { stepped(now, opened + 0.08 + 0.07 * i as f64, 0.16) };

    // ---- the plugin: its picture, name, who made it, and how it is doing ----
    let q = section(0);
    if q > 0.0 {
        let oy = y + 10.0 * (1.0 - q);
        // the picture pops: a little bigger, then settles
        let pop = if q < 1.0 { 1.0 + 0.12 * (1.0 - q) } else { 1.0 };
        let s = (68.0 * pop).round();
        let (ix, iy) = (x - (s - 68.0) / 2.0, oy - (s - 68.0) / 2.0);
        cv.shadow(ix, iy, s, s, 4.0, th.shadow);
        cv.boxed(ix, iy, s, s, 3.0, th.ink, th.accent(colour));
        let inner = ((s - 16.0) / 16.0).floor().max(1.0) * 16.0;
        cv.icon(icon, ix + (s - inner) / 2.0, iy + (s - inner) / 2.0, inner, th.on_accent);

        let tx = x + 68.0 + 18.0;
        let status_w = 132.0;
        let tw = w - 68.0 - 18.0 - status_w - 12.0;
        cv.text_fit(tx, oy - 2.0, 30.0, 22.0, 700, th.ink, &name, tw);
        let author = match info.as_ref().map(|i| &i.signature) {
            Some(Trust::Trusted { author } | Trust::User { author } | Trust::Untrusted { author }) => author.clone(),
            _ => market.map(|m| m.author.to_string()).unwrap_or_default(),
        };
        let version = info.as_ref().map(|i| i.version.clone()).unwrap_or_default();
        let by = match (author.is_empty(), version.is_empty()) {
            (false, false) => format!("by {author} · Version {version}"),
            (false, true) => format!("by {author}"),
            (true, false) => format!("Version {version}"),
            _ => String::new(),
        };
        cv.text_fit(tx, oy + 28.0, 18.0, 13.0, 400, th.slate, &by, tw);
        // trusted or not, as a chip
        if let Some(i) = &info {
            let (word, bg) = match &i.signature {
                Trust::Trusted { .. } => ("Trusted", th.mint),
                Trust::User { .. } => ("Trusted by you", th.sky),
                Trust::Untrusted { .. } => ("Not trusted", th.sun),
                Trust::Unsigned => ("Not signed", th.sun),
                Trust::Bad { .. } => ("Signature doesn't match", th.coral),
            };
            let cw = cv.chip(tx, oy + 50.0, word, 11.0, bg, th.on_accent);
            if i.actions > 0 {
                let n = if i.actions == 1 { "1 button".to_string() } else { format!("{} buttons", i.actions) };
                cv.text(tx + cw + 10.0, oy + 52.0, 18.0, 12.0, 400, th.slate, &n);
            }
        }

        // the status, on the right: a dot that pulses while it runs
        if let Some(i) = &info {
            let (dot, word) = match i.state.as_str() {
                "ready" => (th.mint, "Running"),
                "waking" => (th.sun, "Starting"),
                "off" => (th.slate, "Turned off"),
                _ => (th.coral, "Stopped"),
            };
            let (sx, sy, sh) = (x + w - status_w, oy + 4.0, 34.0);
            cv.boxed(sx, sy, status_w, sh, 3.0, th.ink, th.panel);
            let (ddx, ddy) = (sx + 14.0, sy + 12.0);
            let t = now - opened;
            match i.state.as_str() {
                "ready" => {
                    // a square ring grows out of the dot and fades, every 1.6 s
                    let k = crate::ui::widgets::steps(((t % 1.6) / 1.0) as f32, 5.0);
                    if k < 1.0 {
                        let g = 2.0 + 8.0 * k;
                        cv.border(ddx - g, ddy - g, 10.0 + 2.0 * g, 10.0 + 2.0 * g, 2.0, with_alpha(dot, 0.8 * (1.0 - k)));
                    }
                    cv.fill(ddx, ddy, 10.0, 10.0, dot);
                }
                "waking" => {
                    // blinks while it starts
                    if (t * 3.0) as i64 % 2 == 0 {
                        cv.fill(ddx, ddy, 10.0, 10.0, dot);
                    } else {
                        cv.border(ddx, ddy, 10.0, 10.0, 2.0, dot);
                    }
                }
                _ => cv.fill(ddx, ddy, 10.0, 10.0, dot),
            }
            cv.text(sx + 34.0, sy + 6.0, 22.0, 14.0, 700, th.ink, word);
            if intro_done && pd.closing.is_none() && matches!(i.state.as_str(), "ready" | "waking") {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }
    y += 68.0 + 22.0;
    // why it can't run, when it can't
    if let Some(e) = info.as_ref().and_then(|i| i.error.clone()) {
        let h = cv.wrap_lines(w - 32.0, 13.0, 400, &e) as f32 * 18.0 + 20.0;
        if q > 0.0 {
            cv.boxed(x, y, w, h, 3.0, th.coral, with_alpha(th.coral, 0.12));
            cv.para(x + 16.0, y + 10.0, w - 32.0, 18.0, 13.0, 400, th.ink, &e);
        }
        y += h + 16.0;
    }

    // ---- the plugin card: run it, or remove it ----
    let q = section(1);
    if let Some(i) = &info {
        let ch = 36.0 + 64.0 + 2.0 + 64.0;
        if q > 0.0 {
            let oy = y + 10.0 * (1.0 - q);
            let ry = card(&mut cv, x, oy, w, ch, "ON THIS PC");
            let off = i.state == "off";
            let _ = row_text(&mut cv, x + 16.0, ry + 8.0, w - 32.0 - 80.0, "Run this plugin", if off { "It's turned off: its buttons do nothing until you turn it on." } else { "Turn it off to stop its buttons without removing it." });
            if cv.switch("pd-onoff", x + w - 16.0 - 60.0, ry + 6.0, !off) {
                let id = pd.id.clone();
                pd.say(if off { "Turning it on…" } else { "Turning it off…" });
                std::thread::spawn(move || {
                    let _ = plugins::set_enabled(&id, off);
                });
            }
            cv.fill(x + 16.0, ry + 64.0, w - 32.0, 2.0, th.rule);
            let ry = ry + 66.0;
            let _ = row_text(&mut cv, x + 16.0, ry + 8.0, w - 32.0 - 200.0, "Remove from this PC", if pd.remove_armed { "Sure? Click again to remove it." } else { "You can install it again from the Marketplace." });
            let rl = if pd.remove_armed { "Click again" } else { "Remove" };
            let rw = cv.measure(14.0, 600, rl) + 32.0;
            // armed: it shakes once, then waits in red
            let shake = pd.armed_at.map(|t0| {
                let k = ((now - t0) / 0.3) as f32;
                if k < 1.0 { ((now - t0) * 50.0).sin() as f32 * 4.0 * (1.0 - k) } else { 0.0 }
            }).unwrap_or(0.0).round();
            if shake != 0.0 {
                ctx.request_repaint();
            }
            let mut ro = cv.opt_btn(14.0);
            ro.bw = 3.0;
            ro.shadow = 3.0;
            if pd.remove_armed {
                ro.bg = th.coral;
                ro.bg_hover = th.coral;
                ro.fg = Color32::WHITE;
            } else {
                ro.fg = th.coral;
            }
            if cv.button("pd-remove", x + w - 16.0 - rw + shake, ry + 10.0, rw, 40.0, rl, ro) {
                if pd.remove_armed {
                    let id = pd.id.clone();
                    std::thread::spawn(move || {
                        let _ = plugins::remove(&id);
                    });
                    close = true;
                } else {
                    pd.remove_armed = true;
                    pd.armed_at = Some(now);
                }
            }
        }
        y += ch + 18.0;
    } else {
        cv.text(x, y, 22.0, 14.0, 400, th.slate, "This plugin isn't installed any more.");
        y += 40.0;
    }

    // ---- its settings ----
    let msg = pd.msg.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if msg != pd.msg_seen {
        pd.msg_seen = msg.clone();
        pd.msg_at = now;
    }
    let mut buttons: Vec<String> = vec![];
    let mut save = false;
    let q = section(2);
    if !visible.is_empty() {
        let oy = y + 10.0 * (1.0 - q);
        let top = oy;
        let mut ry = oy + 36.0;
        // first the plugin's own action buttons, side by side
        let acts: Vec<&Value> = visible.iter().filter(|f| s(f, "kind") == "button").collect();
        // laid out once to know the card's height, then drawn
        let mut rows_h = 0.0;
        if !acts.is_empty() {
            let mut bx = 0.0;
            let mut lines = 1.0;
            for f in &acts {
                let bw = cv.measure(14.0, 600, s(f, "label")) + 32.0;
                if bx > 0.0 && bx + bw > w - 32.0 {
                    bx = 0.0;
                    lines += 1.0;
                }
                bx += bw + 10.0;
            }
            rows_h += lines * 50.0 + 8.0;
        }
        for f in visible.iter().filter(|f| s(f, "kind") != "button") {
            rows_h += match s(f, "kind") {
                "switch" => 22.0f32.max(help_h(&cv, w - 32.0 - 80.0, s(f, "help")) + 20.0).max(40.0) + 18.0,
                "select" => 26.0 + help_h(&cv, w - 32.0, s(f, "help")) + 36.0 + 18.0,
                _ => 26.0 + help_h(&cv, w - 32.0, s(f, "help")) + 40.0 + 18.0,
            };
        }
        let has_fields = visible.iter().any(|f| s(f, "kind") != "button");
        let footer = if has_fields { 64.0 } else if !msg.is_empty() { 44.0 } else { 0.0 };
        if footer == 0.0 && !acts.is_empty() {
            rows_h -= 8.0;
        }
        let ch = 36.0 + rows_h + footer;
        if q > 0.0 {
            card(&mut cv, x, top, w, ch, "SETTINGS");
            if !acts.is_empty() {
                let (mut bx, mut by) = (x + 16.0, ry + 2.0);
                for f in &acts {
                    let key = s(f, "key").to_string();
                    let label = s(f, "label");
                    let bw = cv.measure(14.0, 600, label) + 32.0;
                    if bx > x + 16.0 && bx + bw > x + w - 16.0 {
                        bx = x + 16.0;
                        by += 50.0;
                    }
                    let mut o = cv.opt_btn(14.0);
                    o.bw = 3.0;
                    o.shadow = 3.0;
                    if cv.button(&format!("pd-b-{key}"), bx, by, bw, 40.0, label, o) {
                        buttons.push(key);
                    }
                    bx += bw + 10.0;
                }
                ry = by + 40.0 + 18.0;
            }
            for f in visible.iter().filter(|f| s(f, "kind") != "button") {
                let key = s(f, "key").to_string();
                let kind = s(f, "kind");
                let label = s(f, "label");
                let help = s(f, "help");
                match kind {
                    "switch" => {
                        let on = pd.values.get(&key).map(|v| v == "true").unwrap_or(false);
                        let th_ = row_text(&mut cv, x + 16.0, ry + 2.0, w - 32.0 - 80.0, label, help);
                        if cv.switch(&format!("pd-sw-{key}"), x + w - 16.0 - 60.0, ry, on) {
                            pd.values.insert(key.clone(), (!on).to_string());
                        }
                        ry += (th_ + 2.0).max(40.0) + 18.0;
                    }
                    "select" => {
                        cv.text(x + 16.0, ry, 19.0, 14.0, 600, th.ink, label);
                        let hh = help_h(&cv, w - 32.0, help);
                        if hh > 0.0 {
                            cv.para(x + 16.0, ry + 22.0, w - 32.0, 16.0, 12.0, 400, th.slate, help);
                        }
                        let oy2 = ry + hh;
                        let mut sx = x + 16.0;
                        let cur = pd.values.get(&key).cloned().unwrap_or_default();
                        for (i, o) in f.get("options").and_then(|o| o.as_array()).into_iter().flatten().filter_map(|o| o.as_str()).enumerate() {
                            let bw = cv.measure(14.0, 600, o) + 26.0;
                            let r = cv.hit(&format!("pd-sel-{key}-{i}"), sx, oy2 + 26.0, bw, 36.0);
                            let sel = cur == o;
                            cv.boxed(sx, oy2 + 26.0, bw, 36.0, 3.0, th.ink, if sel { th.mint } else if r.hovered() { th.hover } else { th.panel });
                            cv.text_c(sx, bw, oy2 + 34.0, 20.0, 14.0, 600, if sel { th.on_accent } else { th.ink }, o);
                            if r.clicked() {
                                pd.values.insert(key.clone(), o.to_string());
                            }
                            sx += bw + 8.0;
                        }
                        ry += hh + 26.0 + 36.0 + 18.0;
                    }
                    _ => {
                        cv.text(x + 16.0, ry, 19.0, 14.0, 600, th.ink, label);
                        let hh = help_h(&cv, w - 32.0, help);
                        if hh > 0.0 {
                            cv.para(x + 16.0, ry + 22.0, w - 32.0, 16.0, 12.0, 400, th.slate, help);
                        }
                        ry += hh;
                        let mut v = if kind == "password" { pd.values.get(&format!("{key}\u{0}new")).cloned().unwrap_or_default() } else { pd.values.get(&key).cloned().unwrap_or_default() };
                        let ph = if kind == "password" {
                            if pd.saved_secret.get(&key).copied().unwrap_or(false) { "Saved. Type to change it".to_string() } else { "Not set".to_string() }
                        } else {
                            s(f, "placeholder").to_string()
                        };
                        cv.text_field(&format!("pd-f-{key}"), x + 16.0, ry + 26.0, w - 32.0, 40.0, &mut v, 15.0, &ph);
                        if kind == "password" {
                            pd.values.insert(format!("{key}\u{0}new"), v);
                        } else {
                            pd.values.insert(key.clone(), v);
                        }
                        ry += 26.0 + 40.0 + 18.0;
                    }
                }
            }
            // the footer: what happened, and Save (bright when something changed)
            let fy = top + ch - footer + 8.0;
            if footer > 0.0 {
                cv.fill(x + 16.0, fy - 2.0, w - 32.0, 2.0, th.rule);
            }
            let dirty = pd.values.iter().any(|(k, v)| if k.ends_with("\u{0}new") { !v.is_empty() } else { pd.initial.get(k) != Some(v) });
            let mut save_w = 0.0;
            if has_fields {
                save_w = 96.0;
                let mut o = cv.opt_btn(14.0);
                o.bw = 3.0;
                o.shadow = 3.0;
                if dirty {
                    o.bg = th.mint;
                    o.bg_hover = th.mint;
                    o.fg = th.on_accent;
                } else {
                    o.disabled = true;
                }
                if cv.button("pd-save", x + w - 16.0 - save_w, fy + 8.0, save_w, 38.0, "Save", o) {
                    save = true;
                }
            }
            let my = if has_fields { fy + 17.0 } else { fy + 10.0 };
            message(&mut cv, &pd, &msg, x + 16.0, my, w - 32.0 - save_w - 12.0, now, &ctx);
        }
        y += ch + 18.0;
    } else if !msg.is_empty() && q > 0.0 {
        message(&mut cv, &pd, &msg, x, y, w, now, &ctx);
        y += 30.0;
    }
    pd.height = y + 6.0;
    if (pd.height - dh).abs() > 1.0 {
        ctx.request_repaint();
    }

    // the threads' work, after the drawing
    let id = pd.id.clone();
    if save {
        let mut vals = serde_json::Map::new();
        for f in &settings {
            let key = s(f, "key").to_string();
            match s(f, "kind") {
                "switch" => {
                    vals.insert(key.clone(), Value::Bool(pd.values.get(&key).map(|v| v == "true").unwrap_or(false)));
                }
                "password" => {
                    let typed = pd.values.get(&format!("{key}\u{0}new")).cloned().unwrap_or_default();
                    if !typed.is_empty() {
                        vals.insert(key.clone(), Value::String(typed));
                        pd.saved_secret.insert(key.clone(), true);
                        pd.values.insert(format!("{key}\u{0}new"), String::new());
                    }
                }
                "button" => {}
                _ => {
                    if let Some(v) = pd.values.get(&key) {
                        vals.insert(key, Value::String(v.clone()));
                    }
                }
            }
        }
        pd.initial = pd.values.iter().filter(|(k, _)| !k.ends_with("\u{0}new")).map(|(k, v)| (k.clone(), v.clone())).collect();
        pd.say("Saving…");
        let out = pd.msg.clone();
        let c = ctx.clone();
        let id = id.clone();
        std::thread::spawn(move || {
            let r = plugins::save_settings(&id, &vals).unwrap_or_else(|e| e);
            *out.lock().unwrap_or_else(|e| e.into_inner()) = r;
            c.request_repaint();
        });
    }
    for key in buttons {
        pd.say("Working…");
        let out = pd.msg.clone();
        let c = ctx.clone();
        let id = id.clone();
        std::thread::spawn(move || {
            let r = plugins::settings_button(&id, &key).unwrap_or_else(|e| e);
            *out.lock().unwrap_or_else(|e| e.into_inner()) = r;
            c.request_repaint();
        });
    }
    if !msg.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        close = true;
    }
    if close && pd.closing.is_none() {
        pd.closing = Some(now);
    }
    app.plug_dialog = Some(pd);
}

/// What the plugin or a save said: it slides in when it changes, and "…" turns into dots that walk.
fn message(cv: &mut Cv, pd: &PlugDialog, msg: &str, x: f32, y: f32, w: f32, now: f64, ctx: &egui::Context) {
    if msg.is_empty() {
        return;
    }
    let th = cv.th;
    let k = stepped(now, pd.msg_at, 0.16);
    if k < 1.0 {
        ctx.request_repaint();
    }
    let ox = -8.0 * (1.0 - k);
    let busy = msg.ends_with('…');
    let text = if busy {
        let dots = 1 + ((now * 3.0) as usize % 3);
        format!("{}{}", msg.trim_end_matches('…'), ".".repeat(dots))
    } else {
        msg.to_string()
    };
    // a small mark in front: a moving square while busy, a tick when it's done
    if busy {
        let step = (now * 6.0) as usize % 4;
        let (mx, my) = [(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)][step];
        cv.border(x + ox, y + 3.0, 14.0, 14.0, 2.0, th.slate);
        cv.fill(x + ox + 2.0 + mx, y + 5.0 + my, 4.0, 4.0, th.slate);
        ctx.request_repaint_after(std::time::Duration::from_millis(80));
    }
    let tx = if busy { x + ox + 22.0 } else { x + ox };
    cv.text_fit(tx, y, 20.0, 14.0, 400, th.slate, &text, w - 22.0);
}
