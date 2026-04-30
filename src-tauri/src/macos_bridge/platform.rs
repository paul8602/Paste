use std::ffi::CStr;
use cocoa::base::{id, nil};
use cocoa::foundation::{NSAutoreleasePool, NSData, NSString};
use objc::{class, msg_send, sel, sel_impl};

use super::{ClipboardItem, ClipboardPayload};
use crate::history::{Clip, ClipKind};

const PLAIN_TEXT: &str = "public.utf8-plain-text";
const LEGACY_TEXT: &str = "NSStringPboardType";
const RTF: &str = "public.rtf";
const HTML: &str = "public.html";
const PNG: &str = "public.png";
const TIFF: &str = "public.tiff";
const FILE_URL: &str = "public.file-url";
const KEY_V: u16 = 9;
const CG_HID_EVENT_TAP: u32 = 0;
const CG_EVENT_FLAG_MASK_COMMAND: u64 = 1 << 20;

type CGEventRef = *mut std::ffi::c_void;
type CGEventSourceRef = *mut std::ffi::c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn CGEventCreateKeyboardEvent(source: CGEventSourceRef, virtual_key: u16, key_down: bool) -> CGEventRef;
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CFRelease(cf: *const std::ffi::c_void);
}

pub struct ClipboardBridge;

impl ClipboardBridge {
    pub fn new() -> Self {
        Self
    }

    pub fn change_count(&self) -> Result<i64, String> {
        unsafe {
            let _pool = NSAutoreleasePool::new(nil);
            let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
            let count: i64 = msg_send![pasteboard, changeCount];
            Ok(count)
        }
    }

    pub fn read_clip(&self) -> Result<Option<ClipboardItem>, String> {
        unsafe {
            let _pool = NSAutoreleasePool::new(nil);
            let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];

            if let Some(payload) = data_for_type(pasteboard, FILE_URL) {
                let preview = String::from_utf8_lossy(&payload.data).replace('\0', "");
                return Ok(Some(ClipboardItem {
                    kind: ClipKind::FileUrl,
                    text_preview: preview,
                    payloads: vec![payload],
                }));
            }

            if let Some(payload) = data_for_type(pasteboard, PNG).or_else(|| data_for_type(pasteboard, TIFF)) {
                return Ok(Some(ClipboardItem {
                    kind: ClipKind::Image,
                    text_preview: "Image".to_string(),
                    payloads: vec![payload],
                }));
            }

            let mut payloads = Vec::new();
            if let Some(payload) = data_for_type(pasteboard, RTF) {
                payloads.push(payload);
            }
            if let Some(payload) = data_for_type(pasteboard, HTML) {
                payloads.push(payload);
            }
            if let Some(text) = string_for_type(pasteboard, PLAIN_TEXT).or_else(|| string_for_type(pasteboard, LEGACY_TEXT)) {
                payloads.push(ClipboardPayload {
                    uti: PLAIN_TEXT.to_string(),
                    data: text.as_bytes().to_vec(),
                });

                let kind = if payloads.iter().any(|payload| payload.uti == RTF) {
                    ClipKind::Rtf
                } else if payloads.iter().any(|payload| payload.uti == HTML) {
                    ClipKind::Html
                } else {
                    ClipKind::Text
                };

                return Ok(Some(ClipboardItem {
                    kind,
                    text_preview: summarize_text(&text),
                    payloads,
                }));
            }

            Ok(None)
        }
    }

    pub fn write_clip(&self, clip: &Clip) -> Result<(), String> {
        unsafe {
            let _pool = NSAutoreleasePool::new(nil);
            let pasteboard: id = msg_send![class!(NSPasteboard), generalPasteboard];
            let _: i64 = msg_send![pasteboard, clearContents];

            for payload in &clip.payloads {
                if payload.uti == PLAIN_TEXT || payload.uti == LEGACY_TEXT || payload.uti == FILE_URL {
                    let string = String::from_utf8_lossy(&payload.data);
                    set_string_for_type(pasteboard, &string, &payload.uti);
                } else {
                    set_data_for_type(pasteboard, &payload.data, &payload.uti);
                }
            }

            Ok(())
        }
    }

    pub fn has_accessibility_permission(&self) -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    pub fn send_paste_keystroke(&self) -> Result<(), String> {
        if !self.has_accessibility_permission() {
            return Err("Accessibility permission is required to paste into the active app".to_string());
        }

        unsafe {
            let key_down = CGEventCreateKeyboardEvent(std::ptr::null_mut(), KEY_V, true);
            let key_up = CGEventCreateKeyboardEvent(std::ptr::null_mut(), KEY_V, false);

            if key_down.is_null() || key_up.is_null() {
                return Err("failed to create paste keyboard event".to_string());
            }

            CGEventSetFlags(key_down, CG_EVENT_FLAG_MASK_COMMAND);
            CGEventSetFlags(key_up, CG_EVENT_FLAG_MASK_COMMAND);
            CGEventPost(CG_HID_EVENT_TAP, key_down);
            CGEventPost(CG_HID_EVENT_TAP, key_up);
            CFRelease(key_down.cast());
            CFRelease(key_up.cast());
        }

        Ok(())
    }
}

unsafe fn data_for_type(pasteboard: id, uti: &str) -> Option<ClipboardPayload> {
    let ns_type = NSString::alloc(nil).init_str(uti);
    let data: id = msg_send![pasteboard, dataForType: ns_type];
    if data == nil {
        return None;
    }

    let length: usize = msg_send![data, length];
    if length == 0 {
        return None;
    }

    let bytes: *const u8 = msg_send![data, bytes];
    if bytes.is_null() {
        return None;
    }

    Some(ClipboardPayload {
        uti: uti.to_string(),
        data: std::slice::from_raw_parts(bytes, length).to_vec(),
    })
}

unsafe fn string_for_type(pasteboard: id, uti: &str) -> Option<String> {
    let ns_type = NSString::alloc(nil).init_str(uti);
    let string: id = msg_send![pasteboard, stringForType: ns_type];
    if string == nil {
        return None;
    }

    let c_string: *const std::os::raw::c_char = msg_send![string, UTF8String];
    if c_string.is_null() {
        return None;
    }

    Some(CStr::from_ptr(c_string).to_string_lossy().into_owned())
}

unsafe fn set_data_for_type(pasteboard: id, bytes: &[u8], uti: &str) {
    let ns_type = NSString::alloc(nil).init_str(uti);
    let data = NSData::dataWithBytes_length_(nil, bytes.as_ptr() as *const _, bytes.len() as u64);
    let _: bool = msg_send![pasteboard, setData: data forType: ns_type];
}

unsafe fn set_string_for_type(pasteboard: id, value: &str, uti: &str) {
    let ns_type = NSString::alloc(nil).init_str(uti);
    let ns_value = NSString::alloc(nil).init_str(value);
    let _: bool = msg_send![pasteboard, setString: ns_value forType: ns_type];
}

fn summarize_text(value: &str) -> String {
    let summary = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.chars().count() > 160 {
        format!("{}...", summary.chars().take(157).collect::<String>())
    } else {
        summary
    }
}
