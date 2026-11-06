//! Switch profiles automatically: when the app a profile names comes to the front, that profile
//! is opened (Settings > Profiles and pages, and the switch on the Profiles page).
use std::sync::mpsc::Receiver;

use crate::app::App;
use crate::backend::foreground::{self, Front};
use crate::backend::model::auto_name;

pub struct FrontWatch {
    rx: Option<Receiver<Front>>,
}

impl FrontWatch {
    /// Starts watching (not in screenshot mode).
    pub fn start(ctx: &egui::Context, run: bool) -> FrontWatch {
        if !run {
            return FrontWatch { rx: None };
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let c = ctx.clone();
        foreground::start(tx, move || c.request_repaint());
        FrontWatch { rx: Some(rx) }
    }
}

impl App {
    /// Every frame: opens the profile that names the app now in front.
    pub fn pump_front(&mut self, now: f64) {
        let mut latest = None;
        if let Some(rx) = &self.front.rx {
            while let Ok(f) = rx.try_recv() {
                latest = Some(f);
            }
        }
        let Some(front) = latest else { return };
        let switch_on = self.auto_switch && self.st.get("autoSwitch").map(|v| *v == 1).unwrap_or(true);
        if !switch_on {
            return;
        }
        let want = self.sample.profiles.iter().position(|p| foreground::wants(auto_name(p.auto), &front));
        if let Some(i) = want.filter(|&i| i != self.sample.active) {
            self.sample.active = i;
            self.page_at = now;
            let name = self.sample.profiles[i].name.clone();
            self.say(&format!("Switched to {name}"), now);
        }
    }
}

impl App {
    /// Every frame: the Profiles page's "switch automatically" and Settings' are one setting;
    /// the Settings page is written to settings.json a moment after a change.
    pub fn pump_settings(&mut self, now: f64) {
        if self.auto_switch != self.settings.prev_auto {
            self.st.insert("autoSwitch", self.auto_switch as usize);
        } else if let Some(on) = self.st.get("autoSwitch").map(|v| *v == 1).filter(|on| *on != self.auto_switch) {
            self.auto_switch = on;
        }
        self.settings.prev_auto = self.auto_switch;
        self.lib.auto_backup = self.st.get("backup").map(|v| *v == 1).unwrap_or(true);
        if let Some(e) = self.settings.tick(&self.st, now) {
            self.say(&e, now);
        }
        if self.settings.pending() {
            self.egui.request_repaint_after(std::time::Duration::from_millis(300));
        }
    }
}
