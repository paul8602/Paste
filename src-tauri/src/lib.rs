mod commands;
mod error;
mod history;
mod macos_bridge;
mod search;
mod tray;
mod watcher;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use history::HistoryStore;
use macos_bridge::ClipboardBridge;
use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub(crate) struct AppState {
    pub store: Arc<Mutex<HistoryStore>>,
    pub bridge: Arc<ClipboardBridge>,
}

pub(crate) static PASTE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
pub(crate) static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn show_panel(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    window.center().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::setup_tray(app)?;

            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| error::PasteError::AppDataDir(error.to_string()))?;

            let store = Arc::new(Mutex::new(
                history::HistoryStore::new(app_data_dir)
                    .map_err(|error| error::PasteError::HistoryStore(error.to_string()))?,
            ));
            let bridge = Arc::new(ClipboardBridge::new());
            let state = AppState {
                store: store.clone(),
                bridge: bridge.clone(),
            };

            app.manage(state);

            #[cfg(target_os = "macos")]
            let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyV);
            #[cfg(not(target_os = "macos"))]
            let shortcut = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyV);
            let handle = app.handle().clone();
            app.global_shortcut()
                .on_shortcut(shortcut, move |_app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Some(window) = handle.get_webview_window("main") {
                            match window.is_visible() {
                                Ok(true) => {
                                    let _ = window.hide();
                                }
                                _ => {
                                    let _ = show_panel(&handle);
                                }
                            }
                        }
                    }
                })
                .map_err(|error| error::PasteError::GlobalShortcut(error.to_string()))?;

            watcher::start_clipboard_watcher(
                app.handle().clone(),
                store,
                bridge,
                &PASTE_IN_PROGRESS,
                &SHUTDOWN,
            );

            tracing::info!("Paste started");
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                let _ = show_panel(app);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Focused(false)) {
                let w = window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    if !w.is_focused().unwrap_or(true) {
                        let _ = w.hide();
                    }
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::search_clips,
            commands::paste_clip,
            commands::delete_clip,
            commands::pin_clip,
            commands::get_settings,
            commands::save_settings,
            commands::has_accessibility_permission,
            commands::open_accessibility_settings,
            commands::get_clip_thumbnail,
            commands::get_file_preview,
            commands::hide_panel
        ])
        .build(tauri::generate_context!())
        .expect("error building Paste")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                SHUTDOWN.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = app;
            }
        });
}
