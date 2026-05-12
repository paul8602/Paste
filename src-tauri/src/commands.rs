use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::error::PasteError;
use crate::history::{AppSettings, ClipSummary};
use crate::AppState;
use crate::PASTE_IN_PROGRESS;

#[tauri::command]
pub fn search_clips(
    state: tauri::State<'_, AppState>,
    query: String,
    limit: usize,
) -> Result<Vec<ClipSummary>, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.search(&query, limit).map_err(PasteError::Store)
}

#[tauri::command]
pub fn paste_clip(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<(), PasteError> {
    let clip = {
        let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
        store.get_clip(&id).map_err(PasteError::Store)?
    };

    PASTE_IN_PROGRESS.store(true, Ordering::SeqCst);
    state
        .bridge
        .write_clip(&clip)
        .map_err(PasteError::Clipboard)?;
    hide_panel(app.clone())?;
    thread::sleep(Duration::from_millis(80));
    let result = state
        .bridge
        .send_paste_keystroke()
        .map_err(PasteError::Clipboard);
    PASTE_IN_PROGRESS.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub fn delete_clip(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.delete_clip(&id).map_err(PasteError::Store)
}

#[tauri::command]
pub fn pin_clip(
    state: tauri::State<'_, AppState>,
    id: String,
    pinned: bool,
) -> Result<(), PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.pin_clip(&id, pinned).map_err(PasteError::Store)
}

#[tauri::command]
pub fn get_settings(
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.get_settings().map_err(PasteError::Store)
}

#[tauri::command]
pub fn save_settings(
    state: tauri::State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.save_settings(settings).map_err(PasteError::Store)
}

#[tauri::command]
pub fn has_accessibility_permission(state: tauri::State<'_, AppState>) -> bool {
    state.bridge.has_accessibility_permission()
}

#[tauri::command]
pub fn open_accessibility_settings() -> Result<(), PasteError> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .status()
            .map_err(|error| PasteError::OpenSettings(error.to_string()))?;
    }
    #[cfg(target_os = "windows")]
    {
        // Windows does not require accessibility permission for paste keystrokes
    }
    Ok(())
}

#[tauri::command]
pub fn get_clip_thumbnail(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<String>, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.get_clip_thumbnail(&id).map_err(PasteError::Store)
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) -> Result<(), PasteError> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window.hide().map_err(|error| PasteError::Clipboard(error.to_string()))
}
