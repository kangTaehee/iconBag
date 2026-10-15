// 설정 저장/불러오기, 시작 프로그램 등록
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

pub fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    base.join("IconDocs").join("layout.txt")
}

// ───────────────────────── 저장/불러오기 ─────────────────────────
pub fn save() {
    let a = app();
    let mut s = String::new();
    s.push_str(&format!("icons_hidden={}\n", a.icons_hidden as u8));
    for f in &a.fences {
        s.push_str("[fence]\n");
        s.push_str(&format!("title={}\n", f.title));
        s.push_str(&format!("rect={},{},{},{}\n", f.x, f.y, f.w, f.h));
        s.push_str(&format!("color={:06X}\n", f.color));
        s.push_str(&format!("rolled={}\n", f.rolled as u8));
        for it in &f.items {
            s.push_str(&format!("item={}\n", it.path));
        }
    }
    let p = config_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, s);
}

pub fn load() -> bool {
    let a = app();
    let Ok(text) = std::fs::read_to_string(config_path()) else { return false };
    let mut cur: Option<Fence> = None;
    for line in text.lines() {
        let line = line.trim();
        if line == "[fence]" {
            if let Some(f) = cur.take() {
                a.fences.push(f);
            }
            cur = Some(Fence::new("Fence", 20, 20, 330, 300, COLORS[0].1));
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match (k, cur.as_mut()) {
            ("icons_hidden", None) => a.icons_hidden = v == "1",
            ("title", Some(f)) => f.title = v.to_string(),
            ("rect", Some(f)) => {
                let n: Vec<i32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                if n.len() == 4 {
                    f.x = n[0];
                    f.y = n[1];
                    f.w = n[2].max(MIN_W);
                    f.h = n[3].max(TITLE_H + CELL_H);
                }
            }
            ("color", Some(f)) => f.color = u32::from_str_radix(v, 16).unwrap_or(COLORS[0].1),
            ("rolled", Some(f)) => f.rolled = v == "1",
            ("item", Some(f)) => f.items.push(Item::new(v)),
            _ => {}
        }
    }
    if let Some(f) = cur.take() {
        a.fences.push(f);
    }
    true
}

// ───────────────────────── 시작 프로그램 등록 ─────────────────────────
pub const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
pub const RUN_VALUE: &str = "IconDocs";

pub unsafe fn autorun_enabled() -> bool {
    let mut hk: HKEY = null_mut();
    if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_QUERY_VALUE, &mut hk) != 0 {
        return false;
    }
    let r = RegQueryValueExW(hk, wide(RUN_VALUE).as_ptr(), null(), null_mut(), null_mut(), null_mut());
    RegCloseKey(hk);
    r == 0
}

pub unsafe fn set_autorun(on: bool) {
    let mut hk: HKEY = null_mut();
    if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_SET_VALUE, &mut hk) != 0 {
        return;
    }
    if on {
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let v = wide(&format!("\"{}\"", exe));
        RegSetValueExW(hk, wide(RUN_VALUE).as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
    } else {
        RegDeleteValueW(hk, wide(RUN_VALUE).as_ptr());
    }
    RegCloseKey(hk);
}
