//! The window's side of app updates (Settings › Updates): checking for a newer version, downloading
//! its installer and starting it. The download and the checks are backend/updates.rs, which gets
//! its GitHub address from <data>\update.json.
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use egui::ViewportCommand;
use serde_json::json;

use crate::app::App;
use crate::backend::updates::{self, UpdateInfo};
use crate::backend::util::now_ms;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Clone)]
pub enum UpdState {
    Idle,
    Checking(f64),
    /// This is the newest version
    Latest,
    Available(UpdateInfo),
    Downloading,
    /// The installer is downloaded and checked
    Ready,
    Error(String),
}

enum Msg {
    Checked(Result<UpdateInfo, String>, bool),
    Downloaded(Result<String, String>),
}

pub struct UpdateGlue {
    pub state: UpdState,
    /// How far the download is, 0 to 1 (as f32 bits)
    pub frac: Arc<AtomicU32>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    daily_done: bool,
    /// The newest release found, kept for the download button
    info: Option<UpdateInfo>,
}

impl UpdateGlue {
    pub fn new() -> UpdateGlue {
        let (tx, rx) = channel();
        UpdateGlue { state: UpdState::Idle, frac: Arc::new(AtomicU32::new(0)), tx, rx, daily_done: false, info: None }
    }

    pub fn progress(&self) -> f32 {
        f32::from_bits(self.frac.load(Ordering::Relaxed))
    }
}

impl App {
    /// Check for Minniondeck updates. `auto` is the daily check: it says nothing when there is no news.
    pub fn start_update_check(&mut self, auto: bool, now: f64) {
        if !self.tray.on() || matches!(self.upd.state, UpdState::Checking(_) | UpdState::Downloading) {
            return;
        }
        self.upd.state = UpdState::Checking(now);
        let (tx, root, ctx) = (self.upd.tx.clone(), self.lib.paths.root.clone(), self.egui.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Checked(updates::check(&root), auto));
            ctx.request_repaint();
        });
    }

    pub fn start_update_download(&mut self) {
        if !matches!(self.upd.state, UpdState::Available(_)) {
            return;
        }
        self.upd.state = UpdState::Downloading;
        self.upd.frac.store(0f32.to_bits(), Ordering::Relaxed);
        let (tx, root, ctx, frac) = (self.upd.tx.clone(), self.lib.paths.root.clone(), self.egui.clone(), self.upd.frac.clone());
        std::thread::spawn(move || {
            let r = updates::download(&root, |got, total| {
                frac.store((got as f32 / total.max(1) as f32).to_bits(), Ordering::Relaxed);
                ctx.request_repaint();
            });
            let _ = tx.send(Msg::Downloaded(r));
            ctx.request_repaint();
        });
    }

    pub fn cancel_update_download(&mut self) {
        updates::cancel();
        self.upd.state = match self.upd.info.clone() {
            Some(i) => UpdState::Available(i),
            None => UpdState::Idle,
        };
    }

    /// Start the checked installer and close, so it can replace the app.
    pub fn install_update(&mut self, ctx: &egui::Context, now: f64) {
        match updates::install() {
            Ok(()) => {
                // really close, not to the tray
                crate::backend::tray::QUITTING.store(true, Ordering::SeqCst);
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Err(e) => {
                self.upd.state = UpdState::Error(e.clone());
                self.say(&e, now);
            }
        }
    }

    /// Every frame: what the update threads found, and the daily check.
    pub fn pump_updates(&mut self, ctx: &egui::Context, now: f64) {
        if !self.tray.on() {
            return;
        }
        while let Ok(m) = self.upd.rx.try_recv() {
            match m {
                Msg::Checked(r, auto) => {
                    self.settings.set_extra("updChecked", json!(now_ms()));
                    match r {
                        Ok(info) if info.newer => {
                            if auto {
                                self.say(&format!("Minniondeck {} is ready. See Settings › Updates.", info.latest), now);
                            }
                            self.upd.info = Some(info.clone());
                            self.upd.state = UpdState::Available(info);
                        }
                        Ok(_) => self.upd.state = UpdState::Latest,
                        Err(e) => self.upd.state = if auto { UpdState::Idle } else { UpdState::Error(e) },
                    }
                }
                Msg::Downloaded(Ok(_)) => self.upd.state = UpdState::Ready,
                Msg::Downloaded(Err(e)) => {
                    self.upd.state = match self.upd.info.clone() {
                        Some(i) if e.contains("cancelled") => UpdState::Available(i),
                        _ => UpdState::Error(e),
                    }
                }
            }
        }
        // once a day, when "Check for Minniondeck updates every day" is on
        if !self.upd.daily_done && now > 3.0 {
            self.upd.daily_done = true;
            let on = self.st.get("updDaily").map(|v| *v == 1).unwrap_or(true);
            let last = self.settings.get_u64("updChecked");
            if on && now_ms().saturating_sub(last) > DAY_MS {
                self.start_update_check(true, now);
            }
        }
        if matches!(self.upd.state, UpdState::Checking(_) | UpdState::Downloading) {
            ctx.request_repaint();
        }
    }
}
