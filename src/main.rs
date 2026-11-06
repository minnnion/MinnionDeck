#![allow(deprecated)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Minniondeck's desktop app, drawn natively in Rust. Its data lives next to its exe
//! (backend/paths.rs) in the same format as the Minniondeck app's.

mod app;
mod app_io;
mod app_open;
mod app_phones;
mod app_plugins;
mod app_switch;
mod app_tray;
mod app_updates;
mod backend;
mod generated;
mod ui;
mod views;

fn main() -> eframe::Result {
    // Plugins run in a helper copy of this exe: nothing else may start before this
    if std::env::args().any(|a| a == backend::pluginhost::ARG) {
        backend::pluginhost::main();
    }
    let args: Vec<String> = std::env::args().collect();
    // The installer's helpers: nothing else starts. --quit asks the running copy to save and close;
    // --install-plugin <file> installs a plugin without opening the app.
    if args.iter().any(|a| a == backend::single::QUIT) {
        let sent = backend::single::ask_to_quit();
        std::process::exit(if sent { 0 } else { 1 });
    }
    if let Some(i) = args.iter().position(|a| a == "--install-plugin") {
        let Some(file) = args.get(i + 1) else { eprintln!("Name the .mdplugin file."); std::process::exit(2) };
        let root = backend::paths::Paths::find().root;
        match backend::plugins::install_file(&root, std::path::Path::new(file)) {
            Ok((m, _, trust)) => {
                println!("{} {} installed{}", m.name, m.version, if trust.trusted() { "" } else { " (turn on Developer mode to run it)" });
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--add-profile" || a == "--add-profile-file") {
        let Some(id) = args.get(i + 1) else { eprintln!("Name the plugin."); std::process::exit(2) };
        let root = backend::paths::Paths::find().root;
        // --add-profile-file <file.mdprofile> adds a profile file from disk instead
        let r = if args.iter().any(|a| a == "--add-profile-file") {
            backend::library::Library::add_profile_file(std::path::Path::new(id))
        } else {
            backend::market::download_profile(&root, id).and_then(|path| {
                let out = backend::library::Library::add_profile_file(&path);
                let _ = std::fs::remove_file(&path);
                out
            })
        };
        match r {
            Ok(name) => {
                println!("{name} added");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    // One copy at a time: a second launch hands what it was asked to open to the first and quits.
    // (Screenshots and self-tests are not the app, so they don't count.)
    if !args.iter().any(|a| a == "--shot" || a == "--selftest") {
        let to_open: Vec<String> = args.iter().skip(1).filter(|a| !a.starts_with("--")).cloned().collect();
        let silent = to_open.is_empty() && args.iter().any(|a| a == backend::autostart::TRAY_ARG);
        if backend::single::claim(&to_open, silent) == backend::single::Claim::Handed {
            return Ok(());
        }
    }
    // This install's data key, before anything reads or writes saved data
    if let Err(e) = backend::package::init_data_key(&backend::paths::Paths::find().root) {
        use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        unsafe { MessageBoxW(None, &windows::core::HSTRING::from(e.as_str()), &windows::core::HSTRING::from("MinnionDeck"), MB_OK | MB_ICONERROR) };
        std::process::exit(1);
    }
    let light = args.iter().any(|a| a == "--light");
    let view = args.iter().position(|a| a == "--view").and_then(|i| args.get(i + 1)).cloned();
    let shot = args.iter().position(|a| a == "--shot").and_then(|i| args.get(i + 1)).cloned();

    let viewport = egui::ViewportBuilder::default()
        .with_title("MinnionDeck")
        .with_icon(app_icon())
        .with_inner_size([1200.0, 760.0])
        .with_min_inner_size([900.0, 600.0])
        // opened by the sign-in entry (or "Start minimised to the tray"): the window waits in the tray
        .with_visible(!app_tray::start_hidden())
        .with_decorations(false)
        .with_transparent(false);
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Minniondeck",
        options,
        Box::new(move |cc| {
            ui::fonts::install(&cc.egui_ctx);
            Ok(Box::new(app::App::new(light, view, shot, cc.egui_ctx.clone())))
        }),
    )
}

/// The window's and taskbar's icon: the app logo, kept pixel-sharp.
fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/brand/app-icon.png")).map(|i| i.resize_exact(256, 256, image::imageops::FilterType::Nearest).to_rgba8());
    match img {
        Ok(i) => egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() },
        Err(_) => egui::IconData::default(),
    }
}
