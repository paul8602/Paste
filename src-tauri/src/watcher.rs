use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::history::HistoryStore;
use crate::macos_bridge::ClipboardBridge;

pub fn start_clipboard_watcher(
    app: AppHandle,
    store: Arc<Mutex<HistoryStore>>,
    bridge: Arc<ClipboardBridge>,
    paste_in_progress: &'static AtomicBool,
    shutdown: &'static AtomicBool,
) {
    thread::spawn(move || {
        let mut last_change_count = bridge.change_count().unwrap_or_default();

        loop {
            thread::sleep(Duration::from_millis(350));

            if shutdown.load(Ordering::Relaxed) {
                tracing::info!("clipboard watcher shutting down");
                break;
            }

            let Ok(change_count) = bridge.change_count() else {
                continue;
            };

            if change_count == last_change_count {
                continue;
            }

            last_change_count = change_count;

            let item = match bridge.read_clip() {
                Ok(Some(item)) => item,
                Ok(None) => continue,
                Err(_) => {
                    // Retry with backoff
                    thread::sleep(Duration::from_millis(100));
                    match bridge.read_clip() {
                        Ok(Some(item)) => item,
                        _ => continue,
                    }
                }
            };

            if paste_in_progress.load(Ordering::SeqCst) {
                continue;
            }

            if let Ok(store) = store.lock() {
                match store.insert_clip(item) {
                    Ok(clip_id) => {
                        if !clip_id.is_empty() {
                            // Apply rules on the newly inserted clip
                            if let Ok(clips) = store.search("", 1, 0) {
                                if let Some(latest) = clips.first() {
                                    let kind_str = latest.kind.as_str().to_string();
                                    if let Err(e) = store.apply_rules(&clip_id, &latest.text_preview, &kind_str) {
                                        tracing::warn!("rule execution failed: {e}");
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!("failed to save clipboard item: {error}");
                    }
                }
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("clips-changed", ());
            }
        }
    });
}
