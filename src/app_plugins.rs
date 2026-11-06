//! The window's side of plugins: starts them, hears what they say (a button's look, text and
//! picture, toasts), tells them which buttons are on screen, and sends them button events
//! (released, turned). The plugin host itself is backend/plugins.rs.
use std::collections::HashMap;
use std::sync::mpsc;

use crate::app::App;
use crate::backend::actions::{ButtonCtx, Step};
use crate::backend::model::{Live, Tile};
use crate::backend::plugins::{self, Msg, PluginInfo};

/// Pages either side of the one open whose buttons also hear that they are "shown", so a swipe
/// shows live values at once.
const NEAR_PAGES: usize = 5;
const SHOWN_EVERY: f64 = 0.5;
/// Mouse-wheel notches closer together than this go to a knob as one turn
const TURN_BURST: f64 = 0.06;

pub struct PluginGlue {
    rx: Option<mpsc::Receiver<Msg>>,
    /// What plugins show on buttons now, by button id
    live: HashMap<String, Live>,
    /// The plugins Settings lists
    pub rows: Vec<PluginInfo>,
    /// The plugin buttons they were told are on screen: (button, its steps, steps as text)
    shown: HashMap<String, (ButtonCtx, Vec<Step>, String)>,
    shown_at: f64,
    /// The plugin button the pointer is held down on
    pub held: Option<usize>,
    /// Knob turns collected: tile → (clicks, when the first came)
    turns: HashMap<usize, (i32, f64)>,
    /// Developer mode as Settings last had it
    dev_seen: Option<bool>,
}

impl PluginGlue {
    /// Starts plugins (not in screenshot mode, so a picture never runs your buttons' plugins).
    /// Returns a message for the user when plugins were copied.
    pub fn start(root: &std::path::Path, ctx: &egui::Context, run: bool) -> (PluginGlue, Option<String>) {
        let mut glue = PluginGlue { rx: None, live: HashMap::new(), rows: vec![], shown: HashMap::new(), shown_at: 0.0, held: None, turns: HashMap::new(), dev_seen: None };
        let note = None;
        if run {
            // makes <data>market.json (where the marketplace is) the first time, so it can be edited
            let _ = crate::backend::market::Source::load(root);
            let (tx, rx) = mpsc::channel();
            let c = ctx.clone();
            plugins::init(root, tx, move || c.request_repaint());
            glue.rx = Some(rx);
        }
        (glue, note)
    }

    pub fn running(&self) -> bool {
        self.rx.is_some()
    }

    pub fn turn(&mut self, tile: usize, by: i32, now: f64) {
        let e = self.turns.entry(tile).or_insert((0, now));
        e.0 += by;
    }
}

/// A plugin action with several states switches the button between them itself
pub fn owns_states(action: &str) -> bool {
    crate::backend::catalog::actions().iter().find(|a| a.id == action).is_some_and(|a| a.states.len() >= 2)
}

pub fn is_knob(action: &str) -> bool {
    crate::backend::catalog::actions().iter().find(|a| a.id == action).is_some_and(|a| a.knob)
}

fn has_plugin(steps: &[Step]) -> bool {
    steps.iter().any(|s| s.kind.starts_with("plug:"))
}

impl App {
    /// The button's state as a plugin should hear it: before this press moved the look on
    pub fn plugin_ctx(&self, t: &Tile) -> ButtonCtx {
        let n = t.looks();
        let state = if owns_states(&t.action) || n < 2 { t.look_now() } else { (t.cur + n - 1) % n };
        ButtonCtx { id: t.id.clone(), state }
    }

    /// Every frame: what plugins said, what they should hear, and developer mode.
    pub fn pump_plugins(&mut self, ctx: &egui::Context, now: f64) {
        if !self.plug.running() {
            return;
        }
        // what plugins said
        let mut msgs = vec![];
        if let Some(rx) = &self.plug.rx {
            while let Ok(m) = rx.try_recv() {
                msgs.push(m);
            }
        }
        for m in msgs {
            match m {
                Msg::Changed => {
                    crate::backend::catalog::rebuild(&self.lib.paths.root);
                    self.plug.rows = plugins::list();
                    self.market_installed = self
                        .plug
                        .rows
                        .iter()
                        .filter_map(|p| crate::backend::catalog::plugins().iter().find(|c| c.id == p.id).map(|c| (c.id, p.version.clone())))
                        .collect();
                    if self.plug.dev_seen.is_none() {
                        let dev = plugins::dev_mode();
                        self.st.insert("dev", dev as _);
                        self.plug.dev_seen = Some(dev);
                    }
                    // plugins that (re)started hear about the buttons on screen again
                    self.plug.shown.clear();
                }
                Msg::Button { button, state, title, icon } => {
                    let live = self.plug.live.entry(button.clone()).or_default();
                    if let Some(s) = state {
                        live.state = Some(s);
                    }
                    if let Some(t) = title {
                        live.title = t;
                    }
                    if let Some(i) = icon {
                        live.icon = i.map(|(name, bytes)| {
                            crate::ui::images::register_live(&name, bytes);
                            format!("png:{name}")
                        });
                    }
                    if *live == Live::default() {
                        self.plug.live.remove(&button);
                    }
                }
                Msg::Toast { plugin, text } => {
                    let name = self.plug.rows.iter().find(|p| p.id == plugin).map(|p| p.name.clone()).unwrap_or(plugin);
                    self.say(&format!("{name}: {text}"), now);
                }
                Msg::OptionsChanged => {}
            }
        }
        // show it on the buttons (never saved: Live doesn't count as a change)
        for t in self.sample.tiles.iter_mut() {
            let want = self.plug.live.get(&t.id).cloned().unwrap_or_default();
            if t.live != want {
                t.live = want;
            }
        }
        // the pointer let go of a plugin button: it hears "release" (a moment after the press)
        if let Some(i) = self.plug.held {
            if !ctx.input(|i| i.pointer.primary_down()) {
                self.plug.held = None;
                if let Some(t) = self.sample.tiles.get(i).cloned() {
                    let steps = self.lib.steps_of(&t);
                    if has_plugin(&steps) {
                        let b = self.plugin_ctx(&t);
                        std::thread::spawn(move || {
                            std::thread::sleep(std::time::Duration::from_millis(40));
                            let _ = plugins::button_event("release", &b, &steps, None);
                        });
                    }
                }
            }
        }
        // a knob turned with the wheel: quick notches go as one turn
        let due: Vec<usize> = self.plug.turns.iter().filter(|(_, (_, t0))| now - t0 >= TURN_BURST).map(|(i, _)| *i).collect();
        for i in due {
            let Some((by, _)) = self.plug.turns.remove(&i) else { continue };
            if by == 0 {
                continue;
            }
            if let Some(t) = self.sample.tiles.get(i).cloned() {
                let steps = self.lib.steps_of(&t);
                let b = self.plugin_ctx(&t);
                std::thread::spawn(move || {
                    let _ = plugins::button_event("turn", &b, &steps, Some(&serde_json::json!({ "by": by.clamp(-50, 50) })));
                });
            }
        }
        if !self.plug.turns.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
        // developer mode flipped in Settings
        let dev = self.st.get("dev").map(|v| *v == 1).unwrap_or(false);
        if self.plug.dev_seen.is_some_and(|d| d != dev) {
            self.plug.dev_seen = Some(dev);
            std::thread::spawn(move || {
                let _ = plugins::set_dev_mode(dev);
            });
        }
        // which plugin buttons are on screen: the open page and 5 either side
        if now - self.plug.shown_at >= SHOWN_EVERY {
            self.plug.shown_at = now;
            self.update_shown();
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn update_shown(&mut self) {
        let pages: Vec<usize> = (0..self.sample.pages.len()).filter(|&k| !self.sample.pages[k].folder).collect();
        let Some(at) = pages.iter().position(|&k| k == self.sample.page) else { return };
        // the page open here, and the pages phones are showing
        let mut centres = vec![at];
        if let Some(h) = &self.phones.hub {
            for id in h.pages_shown() {
                if let Some(p) = pages.iter().position(|&k| self.sample.pages[k].id == id) {
                    centres.push(p);
                }
            }
        }
        let mut near: Vec<usize> = vec![];
        for c in centres {
            near.extend(pages[c.saturating_sub(NEAR_PAGES)..(c + NEAR_PAGES + 1).min(pages.len())].iter().copied());
        }
        near.sort_unstable();
        near.dedup();
        let mut now_shown: HashMap<String, (ButtonCtx, Vec<Step>, String)> = HashMap::new();
        for t in self.sample.tiles.iter().filter(|t| near.contains(&t.page) && !t.id.is_empty()) {
            let steps = self.lib.steps_of(t);
            if has_plugin(&steps) {
                let key = format!("{steps:?}");
                now_shown.insert(t.id.clone(), (ButtonCtx { id: t.id.clone(), state: t.look_now() }, steps, key));
            }
        }
        let mut events: Vec<(&'static str, ButtonCtx, Vec<Step>)> = vec![];
        for (id, (b, steps, key)) in &self.plug.shown {
            if now_shown.get(id).is_none_or(|n| n.2 != *key) {
                events.push(("hidden", b.clone(), steps.clone()));
            }
        }
        for (id, (b, steps, key)) in &now_shown {
            if self.plug.shown.get(id).is_none_or(|o| o.2 != *key) {
                events.push(("shown", b.clone(), steps.clone()));
            }
        }
        self.plug.shown = now_shown;
        if !events.is_empty() {
            std::thread::spawn(move || {
                for (ev, b, steps) in events {
                    let _ = plugins::button_event(ev, &b, &steps, None);
                }
            });
        }
    }
}
