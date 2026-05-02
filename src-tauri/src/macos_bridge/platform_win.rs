use super::{Clip, ClipboardItem, ClipboardPayload};
use crate::history::ClipKind;
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

#[link(name = "user32")]
extern "system" {
    fn GetClipboardSequenceNumber() -> u32;
}

const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;
const CF_DIB: u32 = 8;

pub struct ClipboardBridge;

impl ClipboardBridge {
    pub fn new() -> Self {
        Self
    }

    pub fn change_count(&self) -> Result<i64, String> {
        unsafe { Ok(GetClipboardSequenceNumber() as i64) }
    }

    pub fn read_clip(&self) -> Result<Option<ClipboardItem>, String> {
        unsafe {
            OpenClipboard(None).map_err(|e| format!("OpenClipboard failed: {e}"))?;
        }

        let result = (|| -> Result<Option<ClipboardItem>, String> {
            // Priority 1: File drop (CF_HDROP)
            if let Ok(handle) = unsafe { GetClipboardData(CF_HDROP) } {
                if !handle.is_invalid() {
                    let drop = HDROP(handle.0);
                    let count = unsafe { DragQueryFileW(drop, 0xFFFFFFFF, None) };
                    if count > 0 {
                        let mut paths = Vec::new();
                        for i in 0..count {
                            let len = unsafe { DragQueryFileW(drop, i, None) } as usize;
                            let mut buf = vec![0u16; len + 1];
                            unsafe { DragQueryFileW(drop, i, Some(&mut buf)); }
                            let path = String::from_utf16_lossy(&buf[..len]);
                            paths.push(path);
                        }
                        let text = paths.join("\n");
                        let preview = summarize_text(&text);
                        return Ok(Some(ClipboardItem {
                            kind: ClipKind::FileUrl,
                            text_preview: preview,
                            payloads: vec![ClipboardPayload {
                                uti: "public.file-url".to_string(),
                                data: text.into_bytes(),
                            }],
                        }));
                    }
                }
            }

            // Priority 2: Image (CF_DIB)
            if let Ok(handle) = unsafe { GetClipboardData(CF_DIB) } {
                if !handle.is_invalid() {
                    let hglobal = HGLOBAL(handle.0);
                    let ptr = unsafe { GlobalLock(hglobal) };
                    if !ptr.is_null() {
                        let size = unsafe { GlobalSize(hglobal) };
                        let data = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();
                        unsafe { let _ = GlobalUnlock(hglobal); }
                        return Ok(Some(ClipboardItem {
                            kind: ClipKind::Image,
                            text_preview: "[Image]".to_string(),
                            payloads: vec![ClipboardPayload {
                                uti: "image/bmp".to_string(),
                                data,
                            }],
                        }));
                    }
                }
            }

            // Priority 3: Unicode text (CF_UNICODETEXT)
            if let Ok(handle) = unsafe { GetClipboardData(CF_UNICODETEXT) } {
                if !handle.is_invalid() {
                    let hglobal = HGLOBAL(handle.0);
                    let ptr = unsafe { GlobalLock(hglobal) };
                    if !ptr.is_null() {
                        let mut end = 0;
                        let p = ptr as *const u16;
                        while unsafe { *p.add(end) } != 0 {
                            end += 1;
                        }
                        let wchars = unsafe { std::slice::from_raw_parts(p, end) };
                        let text = String::from_utf16_lossy(wchars);
                        unsafe { let _ = GlobalUnlock(hglobal); }
                        let preview = summarize_text(&text);
                        return Ok(Some(ClipboardItem {
                            kind: ClipKind::Text,
                            text_preview: preview,
                            payloads: vec![ClipboardPayload {
                                uti: "public.utf8-plain-text".to_string(),
                                data: text.into_bytes(),
                            }],
                        }));
                    }
                }
            }

            Ok(None)
        })();

        unsafe { let _ = CloseClipboard(); }
        result
    }

    pub fn write_clip(&self, clip: &Clip) -> Result<(), String> {
        unsafe {
            OpenClipboard(None).map_err(|e| format!("OpenClipboard failed: {e}"))?;
            EmptyClipboard().map_err(|e| format!("EmptyClipboard failed: {e}"))?;
        }

        for payload in &clip.payloads {
            if payload.uti == "public.utf8-plain-text"
                || payload.uti == "NSStringPboardType"
                || payload.uti == "public.file-url"
            {
                let text = String::from_utf8_lossy(&payload.data);
                let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let size = utf16.len() * 2;
                unsafe {
                    let hmem = GlobalAlloc(GMEM_MOVEABLE, size)
                        .map_err(|e| format!("GlobalAlloc failed: {e}"))?;
                    let ptr = GlobalLock(hmem);
                    if ptr.is_null() {
                        let _ = CloseClipboard();
                        return Err("GlobalLock failed".to_string());
                    }
                    std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr as *mut u16, utf16.len());
                    let _ = GlobalUnlock(hmem);
                    SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hmem.0)))
                        .map_err(|e| format!("SetClipboardData failed: {e}"))?;
                }
            } else {
                let data = &payload.data;
                unsafe {
                    let hmem = GlobalAlloc(GMEM_MOVEABLE, data.len())
                        .map_err(|e| format!("GlobalAlloc failed: {e}"))?;
                    let ptr = GlobalLock(hmem);
                    if ptr.is_null() {
                        let _ = CloseClipboard();
                        return Err("GlobalLock failed".to_string());
                    }
                    std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
                    let _ = GlobalUnlock(hmem);
                    let cf = if payload.uti.contains("image") || payload.uti.contains("bmp") {
                        CF_DIB
                    } else {
                        CF_UNICODETEXT
                    };
                    SetClipboardData(cf, Some(HANDLE(hmem.0)))
                        .map_err(|e| format!("SetClipboardData failed: {e}"))?;
                }
            }
        }

        unsafe { CloseClipboard().map_err(|e| format!("CloseClipboard failed: {e}"))?; }
        Ok(())
    }

    pub fn has_accessibility_permission(&self) -> bool {
        true
    }

    pub fn send_paste_keystroke(&self) -> Result<(), String> {
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_V,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_V,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_CONTROL,
                        dwFlags: KEYEVENTF_KEYUP,
                        ..Default::default()
                    },
                },
            },
        ];

        unsafe {
            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent != inputs.len() as u32 {
                return Err("SendInput failed".to_string());
            }
        }
        Ok(())
    }
}

fn summarize_text(value: &str) -> String {
    let collapsed: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > 160 {
        format!("{}...", &collapsed[..160])
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_short_text() {
        assert_eq!(summarize_text("hello"), "hello");
    }

    #[test]
    fn summarize_collapses_whitespace() {
        assert_eq!(summarize_text("hello   world"), "hello world");
    }

    #[test]
    fn summarize_truncates_long_text() {
        let long = "a".repeat(200);
        let result = summarize_text(&long);
        assert_eq!(result.len(), 163);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn summarize_exact_160() {
        let text = "a".repeat(160);
        assert_eq!(summarize_text(&text), text);
    }
}
