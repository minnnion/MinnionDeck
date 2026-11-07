//! The window's side of phones: shows the phone page's deck (a snapshot of the pages and buttons
//! open now, with what plugins show on them), runs what a phone presses, and keeps the Phones
//! page's list and pairing requests current. The server is backend/phoneweb.
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use serde_json::{Value, json};

use crate::app::App;
use crate::backend::actions::{ButtonCtx, Step};
use crate::backend::phoneweb::{self, Hub, Kind, PhoneEvent};
use crate::backend::{model::Phone, plugins};

/// How often the deck is made again for connected phones.
const SNAPSHOT_EVERY: f64 = 0.1;

pub struct PhoneGlue {
    pub hub: Option<Arc<Hub>>,
    rx: Option<Receiver<PhoneEvent>>,
    snap_at: f64,
    /// Why the phone page isn't running, when it isn't
    pub error: Option<String>,
    /// The QR for an address
    pub qr: Option<(String, Vec<bool>, usize)>,
}

impl PhoneGlue {
    /// Starts the phone page (not for screenshots or self-tests).
    pub fn start(root: &std::path::Path, ctx: &egui::Context, run: bool) -> PhoneGlue {
        let mut g = PhoneGlue { hub: None, rx: None, snap_at: 0.0, error: None, qr: None };
        if run {
            let (tx, rx) = channel();
            let c = ctx.clone();
            match phoneweb::start(root, crate::backend::settings::port_setting(), tx, move || c.request_repaint()) {
                Ok(h) => {
                    g.hub = Some(h);
                    g.rx = Some(rx);
                }
                Err(e) => g.error = Some(e),
            }
        }
        g
    }

    /// The QR modules for `text` (dark is true), and its width.
    pub fn qr_for(&mut self, text: &str) -> Option<(&Vec<bool>, usize)> {
        if self.qr.as_ref().is_none_or(|(t, _, _)| t != text) {
            let code = qrcode::QrCode::with_error_correction_level(text, qrcode::EcLevel::M).ok()?;
            let w = code.width();
            let modules = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
            self.qr = Some((text.to_string(), modules, w));
        }
        self.qr.as_ref().map(|(_, m, w)| (m, *w))
    }
}

fn hex(c: egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// "3 minutes ago", "2 days ago".
fn ago(ms: u64) -> String {
    let secs = crate::backend::util::now_ms().saturating_sub(ms) / 1000;
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

impl App {
    /// The deck as the phone page draws it: the profile in use, its pages and every button's face now.
    pub fn phone_snapshot(&self) -> Value {
        let th = self.theme();
        let on = |k: &str, d: bool| self.st.get(k).map(|v| *v == 1).unwrap_or(d);
        let pages: Vec<Value> = self
            .sample
            .pages
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.folder)
            .map(|(k, p)| {
                let tiles: Vec<Value> = self
                    .sample
                    .tiles
                    .iter()
                    .filter(|t| t.page == k && !t.id.is_empty())
                    .map(|t| {
                        let (label, icon, colour) = t.shown();
                        let ink = if colour == "tile" { th.ink } else { th.ink_on(colour) };
                        let m = t.media_now();
                        let steps = self.lib.steps_of(t);
                        let nav = match t.action.as_str() {
                            "page" => self.page_target(t.value("page")).map(|id| json!({ "page": id })),
                            "back" => Some(json!({ "back": true })),
                            _ => None,
                        };
                        let run = !t.action.is_empty() && (nav.is_none() || steps.len() > 1);
                        json!({
                            "id": t.id, "x": t.col, "y": t.row, "w": t.w, "h": t.h,
                            "label": label, "icon": if icon.is_empty() { "bolt" } else { icon },
                            "colour": hex(th.accent(colour)), "ink": hex(ink),
                            "pic": m.pic, "contain": m.contain, "gif": m.gif, "gifIcon": m.gif_icon, "speed": m.speed,
                            "nav": nav, "run": run, "knob": crate::app_plugins::is_knob(&t.action),
                        })
                    })
                    .collect();
                json!({ "id": p.id, "name": p.name, "cols": p.cols, "rows": p.rows, "tiles": tiles })
            })
            .collect();
        json!({
            "profile": self.sample.profiles.get(self.sample.active).map(|p| p.name.clone()).unwrap_or_default(),
            "page": self.sample.pages.get(self.sample.page).map(|p| p.id.clone()).unwrap_or_default(),
            "follow": on("follow", true),
            "pages": pages,
            "settings": { "names": on("names", true), "vibe": on("vibe", true), "awake": on("awake", true), "dark": th.dark },
            "theme": {
                "ground": hex(th.ground), "ink": hex(th.ink), "text": hex(th.ink), "panel": hex(th.panel), "tile": hex(th.tile),
                "shadow": hex(th.shadow), "slate": hex(th.slate), "mint": hex(th.mint), "coral": hex(th.coral),
            },
        })
    }

    /// The id of the page a Go to page button means (a page by name or id, or a folder's first page).
    fn page_target(&self, want: &str) -> Option<String> {
        let pages = &self.sample.pages;
        let k = pages.iter().position(|p| p.name == want || p.id == want)?;
        if !pages[k].folder {
            return Some(pages[k].id.clone());
        }
        let d = pages[k].depth;
        pages.iter().skip(k + 1).take_while(|p| p.depth > d).find(|p| !p.folder).map(|p| p.id.clone())
    }

    /// Every frame: what phones did, the Phones page's lists, and the deck for connected phones.
    pub fn pump_phones(&mut self, now: f64) {
        let Some(hub) = self.phones.hub.clone() else { return };
        hub.require_match.store(self.st.get("match").map(|v| *v == 1).unwrap_or(true), std::sync::atomic::Ordering::Relaxed);
        let mut events = vec![];
        if let Some(rx) = &self.phones.rx {
            while let Ok(e) = rx.try_recv() {
                events.push(e);
            }
        }
        let mut changed = false;
        for e in events {
            match e {
                PhoneEvent::Changed => changed = true,
                PhoneEvent::Press { device, button, kind } => self.phone_press(&hub, &device, &button, kind, now),
            }
        }
        if changed {
            self.sample.phones = hub
                .phones()
                .into_iter()
                .map(|p| Phone {
                    id: p.id,
                    name: p.name,
                    model: if p.model.is_empty() { "Phone".into() } else { short_model(&p.model) },
                    online: p.online,
                    seen: ago(p.last_seen_ms),
                    tablet: p.tablet,
                })
                .collect();
            crate::backend::tray::set_phones(hub.connected());
        }
        // the deck, for phones that are in
        if hub.connected() > 0 && now - self.phones.snap_at >= SNAPSHOT_EVERY {
            self.phones.snap_at = now;
            hub.set_snapshot(self.phone_snapshot().to_string());
        }
        if !hub.pending().is_empty() || hub.connected() > 0 {
            self.egui.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// A button pressed, held, let go or turned on a phone.
    fn phone_press(&mut self, hub: &Arc<Hub>, device: &str, button: &str, kind: Kind, now: f64) {
        let tell = |hub: &Arc<Hub>, device: &str, r: Result<String, String>| {
            if let Err(message) = r {
                hub.tell(device, &json!({ "t": "result", "ok": false, "message": message }));
            }
        };
        let Some(i) = self.sample.tiles.iter().position(|t| t.id == button) else {
            if kind != Kind::Release {
                tell(hub, device, Err("That button no longer exists.".into()));
            }
            return;
        };
        let t = self.sample.tiles[i].clone();
        let steps: Vec<Step> = self.lib.steps_of(&t);
        let plug = steps.iter().any(|s| s.kind.starts_with("plug:"));
        // what a plugin hears about the button: its look as it is now, before this press moves it on
        let ctx = ButtonCtx { id: t.id.clone(), state: t.look_now() };
        let (hub, device) = (hub.clone(), device.to_string());
        match kind {
            Kind::Release => {
                if plug {
                    std::thread::spawn(move || {
                        let _ = plugins::button_event("release", &ctx, &steps, None);
                    });
                }
            }
            Kind::Turn(by) => {
                if plug && by != 0 {
                    std::thread::spawn(move || {
                        let _ = plugins::button_event("turn", &ctx, &steps, Some(&json!({ "by": by })));
                    });
                }
            }
            Kind::Long if plug => {
                std::thread::spawn(move || {
                    let r = plugins::button_event("long_press", &ctx, &steps, None).map_err(|e| format!("{}: {e}", if t.label.is_empty() { "The button" } else { &t.label }));
                    tell(&hub, &device, r);
                });
            }
            // a long press on anything else is a press, as in the Minniondeck app
            Kind::Press | Kind::Long => {
                if t.action.is_empty() {
                    return tell(&hub, &device, Err("This button does nothing yet.".into()));
                }
                // a phone turns its own pages: Go to page and Back alone have nothing to run
                if (t.action == "page" || t.action == "back") && steps.len() <= 1 {
                    return;
                }
                if self.lib.needs_trust() && crate::backend::actions::risky(&steps) {
                    let name = self.sample.profiles.get(self.sample.active).map(|p| p.name.clone()).unwrap_or_default();
                    if !crate::backend::actions::allow_imported(&name) {
                        return tell(&hub, &device, Err("Buttons from an imported profile need to be allowed on the PC first.".into()));
                    }
                    self.lib.trust();
                }
                // the button moves to its next look, like a press on the PC (a plugin that owns its looks does it itself)
                let n = t.looks();
                if n > 1 && !crate::app_plugins::owns_states(&t.action) {
                    self.sample.tiles[i].cur = (t.cur + 1) % n;
                    self.flip = Some((i, now));
                }
                let label = if t.label.is_empty() { "The button".to_string() } else { t.label.clone() };
                std::thread::spawn(move || {
                    let r = crate::backend::actions::run_steps(&steps, &ctx).map_err(|e| format!("{label}: {e}"));
                    tell(&hub, &device, r);
                });
            }
        }
    }

    /// Takes a phone off the list (Forget): it has to be paired again.
    pub fn forget_phone(&mut self, id: &str) {
        if let Some(h) = &self.phones.hub {
            h.forget(id);
        }
    }
}

/// "Pixel 8 (Android 14)" from what a phone's browser says, short.
fn short_model(ua: &str) -> String {
    let p = ua.split(['(', ')']).nth(1).unwrap_or(ua);
    p.split(';').map(|s| s.trim()).find(|s| !s.is_empty() && *s != "Linux" && !s.starts_with("U")).unwrap_or(p).chars().take(28).collect()
}
