mod actions;
mod autostart;
mod applog;
mod backups;
mod cli;
mod foreground;
mod maptiles;
mod market;
mod opener;
mod package;
mod phones;
mod pluginhost;
mod plugin_settings;
mod plugins;
mod profiles;
mod signing;
mod speech;
mod storage;
mod tray;

use std::time::Duration;

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // The same exe also runs plugins, in a helper process the app starts (see plugins.rs)
    if std::env::args().any(|a| a == pluginhost::ARG) {
        pluginhost::main();
        return;
    }
    // Minniondeck.exe plugin new / pack / install, for plugin authors (see cli.rs)
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = cli::run(&args) {
        std::process::exit(code);
    }
    tauri::Builder::default()
        // Must be first: a second launch (e.g. double-clicking a .mdprofile) hands its
        // arguments to the copy that is already running, then exits
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // `plugin install` asks the running copy to load the plugin, without popping the window up
            if let Some(i) = argv.iter().position(|a| a == plugins::RELOAD_ARG) {
                if let Some(id) = argv.get(i + 1) {
                    plugins::installed(id.clone());
                }
                return;
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
            for path in opener::open_args(&argv) {
                let _ = app.emit("open-profile-file", path);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(tray::on_window_event)
        .setup(|app| {
            // `plugin install` sends --reload-plugin to tell a running Minniondeck. A running one
            // takes it above (single instance) and this copy never gets here; getting here means
            // none is running, and it isn't asked to start: the plugin loads when it next does.
            if std::env::args().any(|a| a == plugins::RELOAD_ARG) {
                app.handle().exit(0);
                return Ok(());
            }
            app.manage(storage::Store::new(app.handle())?);
            applog::init(&app.state::<storage::Store>().paths.root);
            applog::line(&format!("Minniondeck {} started", app.package_info().version));
            // The minnion in the notification area, there as long as Minniondeck runs
            tray::create(app.handle())?;
            // Starts in the tray when the setting says so, or when the sign-in entry started it
            let store = app.state::<storage::Store>();
            let from_sign_in = std::env::args().any(|a| a == autostart::TRAY_ARG);
            let start_in_tray = from_sign_in || storage::setting_value(&store, "tray").and_then(|v| v.as_bool()).unwrap_or(false);
            app.manage(autostart::StartMode { in_tray: std::sync::atomic::AtomicBool::new(start_in_tray) });
            // Keep the sign-in entry pointing at this copy of the app (release builds only)
            #[cfg(windows)]
            if !cfg!(debug_assertions) && storage::setting_value(&store, "startWin").and_then(|v| v.as_bool()).unwrap_or(false) {
                if let Err(e) = autostart::set(true) {
                    applog::line(&format!("Start with Windows: {e}"));
                }
            }
            // Which app is in front, for switching profiles automatically
            foreground::start(app.handle().clone());
            // Phones can connect whenever Minniondeck is running
            let (hub, phone_server) = phones::start(app.handle(), &app.state::<storage::Store>());
            app.manage(hub);
            app.manage(phone_server);
            // Plugins start after the phone hub, so their live button changes can reach phones
            plugins::init(app.handle(), &app.state::<storage::Store>().paths.root);
            // Speech, for the Testing page and for plugins (the model comes with minnion_speech)
            speech::init(app.handle(), &app.state::<storage::Store>().paths.root);
            let args: Vec<String> = std::env::args().collect();
            app.manage(opener::PendingOpen(std::sync::Mutex::new(opener::open_args(&args))));

            // Only the installed (release) app registers itself for .mdprofile files and minniondeck:// links
            #[cfg(windows)]
            if !cfg!(debug_assertions) {
                if let Err(e) = opener::register_file_type() {
                    applog::line(&format!("Could not register .mdprofile files: {e}"));
                }
            }

            // Start-up backup, off the main thread so the window opens straight away
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let store = handle.state::<storage::Store>();
                if backups::auto_backup_on(&store) {
                    if let Err(e) = backups::create(&store, backups::Reason::Startup) {
                        applog::line(&format!("Start-up backup failed: {e}"));
                    }
                }
            });

            // The UI shows the window once saved data is loaded (unless it starts in the tray).
            // If that ever fails, show it anyway rather than leave an invisible app running.
            if !start_in_tray {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_secs(3));
                    if let Some(w) = handle.get_webview_window("main") {
                        let _ = w.show();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            storage::load_state,
            storage::save_state,
            storage::save_media,
            maptiles::map_tile,
            backups::list_backups,
            backups::backup_now,
            backups::restore_backup,
            profiles::export_profile,
            profiles::export_all_profiles,
            profiles::pick_profile_file,
            profiles::import_profile,
            opener::take_pending_open,
            autostart::set_start_with_windows,
            autostart::started_in_tray,
            tray::tray_hop,
            tray::quit_app,
            phones::get_phones,
            phones::phone_server_status,
            phones::start_pairing,
            phones::cancel_pairing,
            phones::forget_phone,
            phones::update_phone,
            phones::set_phone_port,
            phones::set_pc_page,
            phones::phone_press_result,
            phones::notify_phone_update,
            actions::run_button,
            actions::confirm_imported,
            plugins::list_plugins,
            plugins::reload_plugins,
            plugins::plugin_event,
            plugins::plugin_options,
            plugins::plugin_config,
            plugins::set_dev_mode,
            plugins::set_plugin_enabled,
            plugins::remove_plugin,
            plugins::inspect_plugin,
            plugins::install_plugin,
            plugins::add_author,
            plugins::remove_author,
            plugins::pick_plugin_file,
            market::market_lookup,
            market::market_download,
            market::market_profile_download,
            market::market_cancel,
            market::market_discard,
            market::open_site,
            plugins::plugin_settings,
            plugins::save_plugin_settings,
            plugins::plugin_settings_button,
            speech::speech_status,
            speech::speech_say,
            speech::speech_stop,
            applog::app_info,
            applog::ui_log,
            applog::open_app_folder,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_, event| {
            if let tauri::RunEvent::Exit = event {
                plugins::shutdown();
            }
        });
}
