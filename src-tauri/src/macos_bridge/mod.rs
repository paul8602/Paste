use serde::{Deserialize, Serialize};

use crate::history::ClipKind;

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
    use crate::history::Clip;
    use super::ClipboardItem;

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

pub(super) fn summarize_text(value: &str) -> String {
    let summary = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.chars().count() > 160 {
        format!("{}...", summary.chars().take(157).collect::<String>())
    } else {
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_unchanged() {
        assert_eq!(summarize_text("hello world"), "hello world");
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(summarize_text("hello   world\n\tfoo"), "hello world foo");
    }

    #[test]
    fn truncates_long_text_at_160_chars() {
        let long = "a".repeat(200);
        let result = summarize_text(&long);
        assert!(result.ends_with("..."));
        assert_eq!(result.chars().count(), 160);
    }

    #[test]
    fn exactly_160_chars_not_truncated() {
        let exact = "a".repeat(160);
        let result = summarize_text(&exact);
        assert!(!result.ends_with("..."));
    }
}
