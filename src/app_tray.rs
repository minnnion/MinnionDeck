//! The window's side of the tray: Keep running (close goes to the tray), Quit from the tray menu,
//! and Start with Windows. The tray itself is backend/tray.rs.
use std::sync::mpsc::{Receiver, channel};

use egui::ViewportCommand;

use crate::app::App;
use crate::backend::tray::{self, TrayMsg};
use crate::backend::{autostart, settings};

pub struct TrayGlue {
    rx: Option<Receiver<TrayMsg>>,
    /// Start with Windows as last applied, to notice when Settings changes it
    start_prev: Option<usize>,
    selftest: Option<SelfTest>,
}

/// `--selftest <file> [hide]`: a check that the window keeps working while hidden. Writes a line
/// to the file every frame, hides the window after 10 frames when asked, and quits after 4 seconds.
struct SelfTest {
    file: String,
    started: std::time::Instant,
    hide: bool,
    frames: u32,
}

impl TrayGlue {
    /// Puts the minnion in the tray (not for screenshots or self-tests).
    pub fn start(ctx: &egui::Context, run: bool) -> TrayGlue {
        let args: Vec<String> = std::env::args().collect();
        let selftest = args.iter().position(|a| a == "--selftest").and_then(|i| args.get(i + 1)).map(|f| SelfTest { file: f.clone(), started: std::time::Instant::now(), hide: args.iter().any(|a| a == "hide"), frames: 0 });
        let mut glue = TrayGlue { rx: None, start_prev: None, selftest };
        if run {
            let (tx, rx) = channel();
            let c = ctx.clone();
            tray::start(tx, move || c.request_repaint());
            glue.rx = Some(rx);
        }
        glue
    }

    pub fn on(&self) -> bool {
        self.rx.is_some()
    }
}

/// Whether this launch should start in the tray: the sign-in entry says so (--tray), or "Start
/// minimised to the tray" is on. Never for a screenshot.
pub fn start_hidden() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--shot") {
        return false;
    }
    args.iter().any(|a| a == autostart::TRAY_ARG || a == "--selftest-start-hidden") || settings::start_in_tray_setting()
}

impl App {
    /// "Keep running" (Settings › General): the close button goes to the tray.
    fn keep_running(&self) -> bool {
        self.tray.on() && self.st.get("close").copied().unwrap_or(0) == 0 && !tray::quitting()
    }

    /// The window went to the tray. The first time, Windows is asked to say so, since closing the
    /// window doesn't quit the app (Settings › General › When you close the window).
    fn to_tray(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        tray::hop();
        let told = self.lib.paths.root.join("logs").join("tray-note-shown");
        if !told.exists() {
            tray::notify("MinnionDeck is still running", "Your buttons and phones keep working. To quit, right-click the minnion in the tray and choose Quit. You can change this in Settings › General.");
            let _ = std::fs::create_dir_all(self.lib.paths.root.join("logs"));
            let _ = std::fs::write(&told, b"");
        }
    }

    /// The close button was pressed: to the tray, or really closing.
    pub fn request_close(&mut self, ctx: &egui::Context) {
        if self.keep_running() {
            self.to_tray(ctx);
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    /// Start with Windows shows what Windows really has; while it is on, the entry is rewritten
    /// so it follows the exe if it moved.
    pub fn init_autostart(&mut self) {
        if !self.tray.on() {
            return;
        }
        let on = autostart::is_on();
        self.st.insert("startWin", on as usize);
        self.tray.start_prev = Some(on as usize);
        if on {
            std::thread::spawn(|| {
                let _ = autostart::set(true);
            });
        }
    }

    /// Every frame: what the tray asked, a close request, and Start with Windows.
    pub fn pump_tray(&mut self, ctx: &egui::Context, now: f64) {
        let mut opens = vec![];
        if let Some(rx) = &self.tray.rx {
            while let Ok(m) = rx.try_recv() {
                match m {
                    TrayMsg::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
                    // another launch handed over a file or a link (the window was shown already)
                    TrayMsg::Open(what) => opens.push(what),
                }
            }
        }
        // what the app was started with, once it is up
        if now > 0.8 && !self.pending_open.is_empty() {
            opens.extend(std::mem::take(&mut self.pending_open));
        }
        for what in opens {
            // the installer or uninstaller asking the app to save and close
            if what == crate::backend::single::QUIT {
                tray::QUITTING.store(true, std::sync::atomic::Ordering::SeqCst);
                ctx.send_viewport_cmd(ViewportCommand::Close);
                continue;
            }
            self.open_arg(&what, now);
        }
        // Alt+F4 or the taskbar's Close: the same as the close button
        if ctx.input(|i| i.viewport().close_requested()) && self.keep_running() {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.to_tray(ctx);
        }
        // Settings › Start with Windows changed
        if self.tray.on() {
            let cur = self.st.get("startWin").copied();
            if cur.is_some() && cur != self.tray.start_prev {
                self.tray.start_prev = cur;
                let on = cur == Some(1);
                let tx = self.ran_tx.clone();
                std::thread::spawn(move || {
                    if let Err(e) = autostart::set(on) {
                        let _ = tx.send(Err(e));
                    }
                });
            }
        }
        self.pump_selftest(ctx);
    }

    fn pump_selftest(&mut self, ctx: &egui::Context) {
        let Some(t) = &mut self.tray.selftest else { return };
        t.frames += 1;
        let secs = t.started.elapsed().as_secs_f32();
        if t.hide && t.frames == 10 {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&t.file) {
            use std::io::Write;
            let _ = writeln!(f, "{secs:.2} frame {}", t.frames);
        }
        if secs >= 4.0 {
            std::process::exit(0);
        }
        // like the real pump: look again every 100 ms
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}
