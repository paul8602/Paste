mod history;
mod macos_bridge;
mod search;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

static PASTE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

use history::{AppSettings, ClipSummary, HistoryStore};
use macos_bridge::ClipboardBridge;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

struct AppState {
    store: Arc<Mutex<HistoryStore>>,
    bridge: Arc<ClipboardBridge>,
}

#[tauri::command]
fn search_clips(state: tauri::State<'_, AppState>, query: String, limit: usize) -> Result<Vec<ClipSummary>, String> {
    let store = state.store.lock().map_err(|_| "history store lock poisoned")?;
    store.search(&query, limit).map_err(|error| error.to_string())
}

#[tauri::command]
fn paste_clip(state: tauri::State<'_, AppState>, app: AppHandle, id: String) -> Result<(), String> {
    let clip = {
        let store = state.store.lock().map_err(|_| "history store lock poisoned")?;
        store.get_clip(&id).map_err(|error| error.to_string())?
    };

    PASTE_IN_PROGRESS.store(true, Ordering::SeqCst);
    state.bridge.write_clip(&clip).map_err(|error| error.to_string())?;
    hide_panel(app)?;
    thread::sleep(Duration::from_millis(80));
    let result = state.bridge.send_paste_keystroke().map_err(|error| error.to_string());
    PASTE_IN_PROGRESS.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
fn delete_clip(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "history store lock poisoned")?;
    store.delete_clip(&id).map_err(|error| error.to_string())
}

#[tauri::command]
fn pin_clip(state: tauri::State<'_, AppState>, id: String, pinned: bool) -> Result<(), String> {
    let store = state.store.lock().map_err(|_| "history store lock poisoned")?;
    store.pin_clip(&id, pinned).map_err(|error| error.to_string())
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> Result<AppSettings, String> {
    let store = state.store.lock().map_err(|_| "history store lock poisoned")?;
    store.get_settings().map_err(|error| error.to_string())
}

#[tauri::command]
fn save_settings(state: tauri::State<'_, AppState>, settings: AppSettings) -> Result<AppSettings, String> {
    let store = state.store.lock().map_err(|_| "history store lock poisoned")?;
    store.save_settings(settings).map_err(|error| error.to_string())
}

#[tauri::command]
fn has_accessibility_permission(state: tauri::State<'_, AppState>) -> bool {
    state.bridge.has_accessibility_permission()
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map_err(|error| format!("failed to open Accessibility settings: {error}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        // Windows does not require accessibility permission for paste keystrokes
    }
    Ok(())
}

#[tauri::command]
fn hide_panel(app: AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window.hide().map_err(|error| error.to_string())
}

fn show_panel(app: &AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };

    window.center().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

fn start_clipboard_watcher(app: AppHandle, store: Arc<Mutex<HistoryStore>>, bridge: Arc<ClipboardBridge>) {
    thread::spawn(move || {
        let mut last_change_count = bridge.change_count().unwrap_or_default();

        loop {
            thread::sleep(Duration::from_millis(350));

            let Ok(change_count) = bridge.change_count() else {
                continue;
            };

            if change_count == last_change_count {
                continue;
            }

            last_change_count = change_count;

            let Ok(Some(item)) = bridge.read_clip() else {
                continue;
            };

            if PASTE_IN_PROGRESS.load(Ordering::SeqCst) {
                continue;
            }

            if let Ok(store) = store.lock() {
                if let Err(error) = store.insert_clip(item) {
                    eprintln!("failed to save clipboard item: {error}");
                }
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("clips-changed", ());
            }
        }
    });
}

fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show Paste", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let icon = app
        .default_window_icon()
        .ok_or("missing default app icon")?
        .clone();
    let handle = app.handle().clone();

    TrayIconBuilder::with_id("main-tray")
        .tooltip("Paste")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(move |_tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_panel(&handle);
            }
        })
        .build(app)?;

    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            setup_tray(app)?;

            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("failed to resolve app data dir: {error}"))?;

            let store = Arc::new(Mutex::new(
                HistoryStore::new(app_data_dir).map_err(|error| format!("failed to open history store: {error}"))?,
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
                .map_err(|error| format!("failed to register global shortcut: {error}"))?;

            start_clipboard_watcher(app.handle().clone(), store, bridge);
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
            if matches!(event, WindowEvent::Focused(false)) {
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            search_clips,
            paste_clip,
            delete_clip,
            pin_clip,
            get_settings,
            save_settings,
            has_accessibility_permission,
            open_accessibility_settings,
            hide_panel
        ])
        .run(tauri::generate_context!())
        .expect("error while running Paste");
}
