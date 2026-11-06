//! A plugin's window, opened from Settings › Plugins: whether it runs (and why not), turning it
//! off or on, its own settings (the fields its manifest lists), and Remove.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, Rect, Sense, Ui, pos2, vec2};
use serde_json::Value;

use crate::app::App;
use crate::backend::plugins;
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
}

impl PlugDialog {
    pub fn new(id: &str) -> PlugDialog {
        let mut d = PlugDialog { id: id.to_string(), fields: vec![], values: HashMap::new(), saved_secret: HashMap::new(), opened: None, closing: None, msg: Arc::new(Mutex::new(String::new())), remove_armed: false };
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
        d
    }

    fn say(&self, text: &str) {
        *self.msg.lock().unwrap_or_else(|e| e.into_inner()) = text.to_string();
    }
}

fn s<'a>(f: &'a Value, k: &str) -> &'a str {
    f.get(k).and_then(|v| v.as_str()).unwrap_or("")
}

pub fn dialog(app: &mut App, ui: &mut Ui, win: Rect) {
    let Some(mut pd) = app.plug_dialog.take() else { return };
    let th = app.theme();
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let opened = *pd.opened.get_or_insert(now);
    let mut p = crate::ui::widgets::steps(((now - opened) / 0.18) as f32, 4.0).clamp(0.0, 1.0);
    if let Some(t0) = pd.closing {
        p = p.min(1.0 - ((now - t0) / 0.12) as f32);
        if p <= 0.0 {
            return;
        }
    }
    let p = p.clamp(0.0, 1.0);
    if p < 1.0 || pd.closing.is_some() {
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
    let settings: Vec<Value> = pd.fields.clone();
    // height: title, status, buttons, then every setting
    let field_h = |f: &Value| -> f32 {
        match s(f, "kind") {
            "switch" => 62.0,
            "button" => 52.0,
            "select" => 90.0,
            _ => 82.0,
        }
    };
    let body_h: f32 = 190.0 + settings.iter().filter(|f| !f.get("hidden").and_then(|h| h.as_bool()).unwrap_or(false)).map(field_h).sum::<f32>() + if settings.is_empty() { 0.0 } else { 56.0 };
    let (dw, dh) = (560.0_f32, (body_h + 54.0).min(win.height() - 50.0));
    let dx = win.min.x + (win.width() - dw) / 2.0;
    let dy = win.min.y + 36.0 + (win.height() - 36.0 - dh) / 2.0 + 10.0 * (1.0 - p);
    let dialog = Rect::from_min_size(pos2(dx, dy), vec2(dw, dh));
    let mut cv = Cv::new(ui, win, dialog.min.to_vec2(), th, "plugdlg");
    cv.hit("pd-body", 0.0, 0.0, dw, dh);
    cv.shadow(0.0, 0.0, dw, dh, 8.0, with_alpha(th.shadow, p));
    cv.boxed(0.0, 0.0, dw, dh, 4.0, th.ink, th.panel);
    cv.fill(4.0, 4.0, dw - 8.0, 46.0, th.shadow);
    let light = Color32::from_rgb(0xe4, 0xea, 0xf6);
    let name = info.as_ref().map(|i| i.name.clone()).unwrap_or_else(|| pd.id.clone());
    crate::app::draw_minnion(&mut cv, 16.0, 15.0, 24.0);
    cv.text_fit(50.0, 16.0, 24.0, 18.0, 700, light, &name, dw - 120.0);
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
    let mut y = 50.0 + 20.0;
    // status
    if let Some(i) = &info {
        let (dot, word) = match i.state.as_str() {
            "ready" => (th.mint, "Running"),
            "waking" => (th.sun, "Starting"),
            "off" => (th.slate, "Turned off"),
            _ => (th.coral, "Stopped"),
        };
        cv.fill(x, y + 6.0, 10.0, 10.0, dot);
        cv.text(x + 18.0, y, 22.0, 15.0, 700, th.ink, word);
        cv.text_r(dw - 24.0, y, 22.0, 14.0, 400, th.slate, &format!("Version {}", i.version));
        y += 28.0;
        if let Some(e) = &i.error {
            y += cv.para(x, y, w, 18.0, 13.0, 400, th.coral, e) + 6.0;
        }
        let by = y + 6.0;
        let o = cv.opt_btn(14.0);
        let off = i.state == "off";
        let label = if off { "Turn on" } else { "Turn off" };
        let bw = cv.measure(14.0, 600, label) + 32.0;
        if cv.button("pd-onoff", x, by, bw, 40.0, label, o) {
            let id = pd.id.clone();
            pd.say(if off { "Turning it on…" } else { "Turning it off…" });
            std::thread::spawn(move || {
                let _ = plugins::set_enabled(&id, off);
            });
        }
        let mut bx = x + bw + 10.0;
        let rl = if pd.remove_armed { "Click again to remove it" } else { "Remove" };
        let rw = cv.measure(14.0, 600, rl) + 32.0;
        let mut ro = cv.opt_btn(14.0);
        if pd.remove_armed {
            ro.bg = th.coral;
            ro.bg_hover = th.coral;
            ro.fg = Color32::WHITE;
        }
        if cv.button("pd-remove", bx, by, rw, 40.0, rl, ro) {
            if pd.remove_armed {
                let id = pd.id.clone();
                std::thread::spawn(move || {
                    let _ = plugins::remove(&id);
                });
                close = true;
            } else {
                pd.remove_armed = true;
            }
        }
        bx += rw + 10.0;
        let _ = bx;
        y = by + 40.0 + 14.0;
    } else {
        cv.text(x, y, 22.0, 14.0, 400, th.slate, "This plugin isn't installed any more.");
        y += 40.0;
    }
    let msg = pd.msg.lock().unwrap_or_else(|e| e.into_inner()).clone();

    // its settings
    let mut buttons: Vec<String> = vec![];
    let mut save = false;
    let visible: Vec<&Value> = settings.iter().filter(|f| !f.get("hidden").and_then(|h| h.as_bool()).unwrap_or(false)).collect();
    if !visible.is_empty() {
        cv.fill(x, y, w, 2.0, th.rule);
        y += 14.0;
    }
    for f in visible {
        let key = s(f, "key").to_string();
        let kind = s(f, "kind");
        let label = s(f, "label");
        match kind {
            "switch" => {
                let on = pd.values.get(&key).map(|v| v == "true").unwrap_or(false);
                cv.text_fit(x, y + 10.0, 22.0, 15.0, 600, th.ink, label, w - 80.0);
                if cv.switch(&format!("pd-sw-{key}"), x + w - 60.0, y, on) {
                    pd.values.insert(key.clone(), (!on).to_string());
                }
                y += 52.0;
            }
            "button" => {
                let o = cv.opt_btn(14.0);
                let bw = cv.measure(14.0, 600, label) + 32.0;
                if cv.button(&format!("pd-b-{key}"), x, y, bw, 40.0, label, o) {
                    buttons.push(key.clone());
                }
                y += 50.0;
            }
            "select" => {
                cv.text(x, y, 19.0, 14.0, 600, th.ink, label);
                y += 24.0;
                let mut cx = x;
                let cur = pd.values.get(&key).cloned().unwrap_or_default();
                for (i, o) in f.get("options").and_then(|o| o.as_array()).into_iter().flatten().filter_map(|o| o.as_str()).enumerate() {
                    let bw = cv.measure(14.0, 600, o) + 26.0;
                    let r = cv.hit(&format!("pd-sel-{key}-{i}"), cx, y, bw, 34.0);
                    let sel = cur == o;
                    cv.boxed(cx, y, bw, 34.0, 3.0, th.ink, if sel { th.mint } else if r.hovered() { th.hover } else { th.tile });
                    cv.text_c(cx, bw, y + 7.0, 20.0, 14.0, 600, if sel { th.on_accent } else { th.ink }, o);
                    if r.clicked() {
                        pd.values.insert(key.clone(), o.to_string());
                    }
                    cx += bw + 8.0;
                }
                y += 56.0;
            }
            _ => {
                cv.text(x, y, 19.0, 14.0, 600, th.ink, label);
                y += 24.0;
                let mut v = if kind == "password" { pd.values.get(&format!("{key}\u{0}new")).cloned().unwrap_or_default() } else { pd.values.get(&key).cloned().unwrap_or_default() };
                let ph = if kind == "password" {
                    if pd.saved_secret.get(&key).copied().unwrap_or(false) { "Saved. Type to change it".to_string() } else { "Not set".to_string() }
                } else {
                    s(f, "placeholder").to_string()
                };
                cv.text_field(&format!("pd-f-{key}"), x, y, w, 40.0, &mut v, 15.0, &ph);
                if kind == "password" {
                    pd.values.insert(format!("{key}\u{0}new"), v);
                } else {
                    pd.values.insert(key.clone(), v);
                }
                y += 48.0;
            }
        }
    }
    if !settings.is_empty() {
        let o = cv.opt_btn(14.0);
        if cv.button("pd-save", x, y, 90.0, 40.0, "Save", o) {
            save = true;
        }
        cv.text_fit(x + 106.0, y + 10.0, 20.0, 14.0, 400, th.slate, &msg, w - 106.0);
    } else if !msg.is_empty() {
        cv.text_fit(x, y, 20.0, 14.0, 400, th.slate, &msg, w);
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
                        vals.insert(key, Value::String(typed));
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
