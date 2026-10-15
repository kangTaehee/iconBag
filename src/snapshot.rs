// 레이아웃 스냅샷: 이름으로 저장·복원, 해상도별 자동 전환
#![allow(unused_imports, dead_code)]
use crate::*;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn snapshot_dir() -> PathBuf {
    config_dir().join("snapshots")
}
pub fn snapshot_path(name: &str) -> PathBuf {
    snapshot_dir().join(format!("{}.txt", name))
}
pub fn auto_name(w: i32, h: i32) -> String {
    format!("auto_{}x{}", w, h)
}

// 파일명에 쓸 수 없는 문자 제거
pub fn sanitize(name: &str) -> String {
    name.chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect::<String>().trim().to_string()
}

pub fn list_snapshots() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(snapshot_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().and_then(|x| x.to_str()) == Some("txt") {
                        p.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn save_snapshot(name: &str) {
    let name = sanitize(name);
    if name.is_empty() {
        return;
    }
    let _ = std::fs::create_dir_all(snapshot_dir());
    let _ = std::fs::write(snapshot_path(&name), serialize());
}

pub fn delete_snapshot(name: &str) {
    let _ = std::fs::remove_file(snapshot_path(name));
}

// 현재 Fence 를 모두 없애고 텍스트의 배치로 교체
pub unsafe fn apply_layout(text: &str) {
    let a = app();
    if a.peeking {
        peek_end();
    }
    for f in a.fences.drain(..) {
        if !f.hwnd.is_null() {
            DestroyWindow(f.hwnd);
        }
    }
    a.rules.clear();
    parse_into(text);
    for i in 0..app().fences.len() {
        create_fence_window(i);
        if app().fences[i].portal.is_some() {
            refresh_portal(i);
        }
        invalidate(i);
    }
    apply_icons_hidden();
    rescan();
    save();
}

pub unsafe fn restore_snapshot(name: &str) {
    if let Ok(text) = std::fs::read_to_string(snapshot_path(name)) {
        apply_layout(&text);
    }
}

// WM_DISPLAYCHANGE: 이전 해상도용으로 현재 배치를 저장하고, 새 해상도용 배치가 있으면 복원
pub unsafe fn on_display_change(new_w: i32, new_h: i32) {
    let a = app();
    let (ow, oh) = a.last_res;
    if (ow, oh) == (new_w, new_h) || new_w <= 0 || new_h <= 0 {
        return;
    }
    save_snapshot(&auto_name(ow, oh));
    a.last_res = (new_w, new_h);
    let p = snapshot_path(&auto_name(new_w, new_h));
    if p.exists() {
        restore_snapshot(&auto_name(new_w, new_h));
    } else {
        clamp_fences_to_screen();
        save();
    }
}
