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

            let Ok(Some(item)) = bridge.read_clip() else {
                continue;
            };

            if paste_in_progress.load(Ordering::SeqCst) {
                continue;
            }

            if let Ok(store) = store.lock() {
                if let Err(error) = store.insert_clip(item) {
                    tracing::error!("failed to save clipboard item: {error}");
                }
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("clips-changed", ());
            }
        }
    });
}
