use serde::{Deserialize, Serialize};

use crate::history::{Clip, ClipKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardPayload {
    pub uti: String,
    pub data: Vec<u8>,
}

impl ClipboardPayload {
    pub fn is_blob_candidate(&self) -> bool {
        self.uti.contains("image")
            || self.uti.contains("png")
            || self.uti.contains("tiff")
            || self.uti.contains("rtf")
            || self.uti.contains("html")
    }
}

#[derive(Debug, Clone)]
pub struct ClipboardItem {
    pub kind: ClipKind,
    pub text_preview: String,
    pub payloads: Vec<ClipboardPayload>,
}

#[cfg(target_os = "macos")]
mod platform;

#[cfg(target_os = "windows")]
mod platform_win;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::{Clip, ClipboardItem};

    pub struct ClipboardBridge;

    impl ClipboardBridge {
        pub fn new() -> Self {
            Self
        }

        pub fn change_count(&self) -> Result<i64, String> {
            Ok(0)
        }

        pub fn read_clip(&self) -> Result<Option<ClipboardItem>, String> {
            Ok(None)
        }

        pub fn write_clip(&self, _clip: &Clip) -> Result<(), String> {
            Err("clipboard bridge is only implemented on macOS and Windows".to_string())
        }

        pub fn has_accessibility_permission(&self) -> bool {
            false
        }

        pub fn send_paste_keystroke(&self) -> Result<(), String> {
            Err("paste keystroke is only implemented on macOS and Windows".to_string())
        }
    }
}

#[cfg(target_os = "windows")]
pub use platform_win::ClipboardBridge;

#[cfg(not(target_os = "windows"))]
pub use platform::ClipboardBridge;
