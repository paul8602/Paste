use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::error::PasteError;
use crate::history::{
    AppSettings, ClipKind, ClipSummary, DiskUsage, FilePreview, ImportResult, Rule, Tag,
};
use crate::AppState;
use crate::PASTE_IN_PROGRESS;

/// Maximum time to wait for clipboard write to be acknowledged (ms).
const MAX_PASTE_WAIT_MS: u64 = 200;
/// Polling interval when waiting for clipboard changeCount to update (ms).
const PASTE_POLL_STEP_MS: u64 = 15;

#[tauri::command]
pub fn search_clips(
    state: tauri::State<'_, AppState>,
    query: String,
    limit: usize,
    offset: usize,
) -> Result<Vec<ClipSummary>, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.search(&query, limit, offset).map_err(PasteError::Store)
}

/// Wait for the clipboard changeCount to shift, confirming the write completed.
/// Falls back to a minimum sleep if changeCount is unavailable.
fn wait_for_clipboard_write(bridge: &crate::macos_bridge::ClipboardBridge, count_before: i64) {
    let deadline = Instant::now() + Duration::from_millis(MAX_PASTE_WAIT_MS);
    loop {
        let elapsed = Instant::now();
        if elapsed >= deadline {
            break;
        }
        if let Ok(count) = bridge.change_count() {
            if count != count_before {
                return; // write acknowledged
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        let step = Duration::from_millis(PASTE_POLL_STEP_MS).min(remaining);
        thread::sleep(step);
    }
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

    let count_before = state.bridge.change_count().unwrap_or_default();
    state
        .bridge
        .write_clip(&clip)
        .map_err(PasteError::Clipboard)?;
    hide_panel(app.clone())?;

    wait_for_clipboard_write(&state.bridge, count_before);

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
pub fn get_file_preview(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<FilePreview>, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.get_file_preview(&id).map_err(PasteError::Store)
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) -> Result<(), PasteError> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    window.hide().map_err(|error| PasteError::Clipboard(error.to_string()))
}

// ── Export / Import commands ───────────────────────────────────────

#[tauri::command]
pub fn export_to_json(
    state: tauri::State<'_, AppState>,
    ids: Option<Vec<String>>,
    kind: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Result<String, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    let kind = kind.map(|k| ClipKind::from_str(&k));
    store
        .export_to_json(ids, kind, date_from, date_to)
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn export_to_csv(
    state: tauri::State<'_, AppState>,
    ids: Option<Vec<String>>,
    kind: Option<String>,
    date_from: Option<String>,
    date_to: Option<String>,
) -> Result<String, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    let kind = kind.map(|k| ClipKind::from_str(&k));
    store
        .export_to_csv(ids, kind, date_from, date_to)
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn import_from_json(
    state: tauri::State<'_, AppState>,
    json: String,
    mode: String,
) -> Result<ImportResult, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .import_from_json(&json, &mode)
        .map_err(PasteError::Store)
}

// ── Data Management commands ───────────────────────────────────────

#[tauri::command]
pub fn delete_by_date_range(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
) -> Result<usize, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .delete_by_date_range(&from, &to)
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn delete_selected(
    state: tauri::State<'_, AppState>,
    ids: Vec<String>,
) -> Result<usize, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.delete_selected(&ids).map_err(PasteError::Store)
}

#[tauri::command]
pub fn delete_by_type(
    state: tauri::State<'_, AppState>,
    kind: String,
) -> Result<usize, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .delete_by_type(ClipKind::from_str(&kind))
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn auto_prune(
    state: tauri::State<'_, AppState>,
    retention_days: usize,
) -> Result<usize, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.auto_prune(retention_days).map_err(PasteError::Store)
}

#[tauri::command]
pub fn count_by_type(
    state: tauri::State<'_, AppState>,
    kind: String,
) -> Result<usize, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .count_by_type(ClipKind::from_str(&kind))
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn count_by_date_range(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
) -> Result<usize, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .count_by_date_range(&from, &to)
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn count_prunable(
    state: tauri::State<'_, AppState>,
    retention_days: usize,
) -> Result<usize, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.count_prunable(retention_days).map_err(PasteError::Store)
}

#[tauri::command]
pub fn get_disk_usage(
    state: tauri::State<'_, AppState>,
) -> Result<DiskUsage, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.get_disk_usage().map_err(PasteError::Store)
}

#[tauri::command]
pub fn import_from_csv(
    state: tauri::State<'_, AppState>,
    csv: String,
    mode: String,
) -> Result<ImportResult, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .import_from_csv(&csv, &mode)
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn create_tag(
    state: tauri::State<'_, AppState>,
    name: String,
    color: Option<String>,
) -> Result<Tag, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .create_tag(&name, color.as_deref())
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn list_tags(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Tag>, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.list_tags().map_err(PasteError::Store)
}

#[tauri::command]
pub fn delete_tag(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<(), PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.delete_tag(id).map_err(PasteError::Store)
}

#[tauri::command]
pub fn add_tag_to_clip(
    state: tauri::State<'_, AppState>,
    clip_id: String,
    tag_id: i64,
) -> Result<(), PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .add_tag_to_clip(&clip_id, tag_id)
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn remove_tag_from_clip(
    state: tauri::State<'_, AppState>,
    clip_id: String,
    tag_id: i64,
) -> Result<(), PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .remove_tag_from_clip(&clip_id, tag_id)
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn get_clip_tags(
    state: tauri::State<'_, AppState>,
    clip_id: String,
) -> Result<Vec<Tag>, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.get_clip_tags(&clip_id).map_err(PasteError::Store)
}

#[tauri::command]
pub fn create_rule(
    state: tauri::State<'_, AppState>,
    name: String,
    pattern: String,
    pattern_type: String,
    action: String,
    action_value: Option<String>,
) -> Result<Rule, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .create_rule(
            &name,
            &pattern,
            &pattern_type,
            &action,
            action_value.as_deref(),
        )
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn list_rules(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Rule>, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.list_rules().map_err(PasteError::Store)
}

#[tauri::command]
pub fn update_rule(
    state: tauri::State<'_, AppState>,
    id: i64,
    name: String,
    pattern: String,
    pattern_type: String,
    action: String,
    action_value: Option<String>,
    enabled: bool,
    priority: i64,
) -> Result<Rule, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .update_rule(
            id,
            &name,
            &pattern,
            &pattern_type,
            &action,
            action_value.as_deref(),
            enabled,
            priority,
        )
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn delete_rule(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<(), PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store.delete_rule(id).map_err(PasteError::Store)
}

#[tauri::command]
pub fn update_tag(
    state: tauri::State<'_, AppState>,
    id: i64,
    name: String,
    color: Option<String>,
) -> Result<Tag, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    store
        .update_tag(id, &name, color.as_deref())
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn apply_rules_to_clip(
    state: tauri::State<'_, AppState>,
    clip_id: String,
) -> Result<Vec<String>, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    let clip = store.get_clip(&clip_id).map_err(PasteError::Store)?;
    store
        .apply_rules(&clip_id, &clip.text_preview, clip.kind.as_str())
        .map_err(PasteError::Store)
}

#[tauri::command]
pub fn batch_apply_rules(
    state: tauri::State<'_, AppState>,
) -> Result<usize, PasteError> {
    let store = state.store.lock().map_err(|_| PasteError::LockPoisoned)?;
    let clips = store.search("", 10000, 0).map_err(PasteError::Store)?;
    let mut processed = 0;
    for clip in &clips {
        let kind_str = clip.kind.as_str().to_string();
        if let Ok(actions) = store.apply_rules(&clip.id, &clip.text_preview, &kind_str) {
            if !actions.is_empty() {
                processed += 1;
            }
        }
    }
    Ok(processed)
}
