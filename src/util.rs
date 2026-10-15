// 공용 유틸
#![allow(unused_imports, dead_code)]
use crate::*;
use core::ffi::c_void;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
pub fn from_wide(p: &[u16]) -> String {
    let n = p.iter().position(|&c| c == 0).unwrap_or(p.len());
    String::from_utf16_lossy(&p[..n])
}
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}
pub fn darken(c: u32, f: u32) -> u32 {
    let r = (c & 0xFF) * f / 100;
    let g = ((c >> 8) & 0xFF) * f / 100;
    let b = ((c >> 16) & 0xFF) * f / 100;
    r | (g << 8) | (b << 16)
}
pub fn lighten(c: u32, add: u32) -> u32 {
    let r = ((c & 0xFF) + add).min(255);
    let g = (((c >> 8) & 0xFF) + add).min(255);
    let b = (((c >> 16) & 0xFF) + add).min(255);
    r | (g << 8) | (b << 16)
}
pub fn display_name(p: &Path) -> String {
    let ext = p.extension().and_then(|e| e.to_str()).map(lower).unwrap_or_default();
    if ext == "lnk" || ext == "url" {
        p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()
    } else {
        p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string()
    }
}

pub fn norm_rect(a: POINT, b: POINT) -> (i32, i32, i32, i32) {
    (a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
}
