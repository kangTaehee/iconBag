// 설정 직렬화/복원, 자동 분류 규칙, 시작 프로그램 등록
#![allow(unused_imports, dead_code)]
use crate::*;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn config_dir() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    base.join("IconDocs")
}
pub fn config_path() -> PathBuf {
    config_dir().join("layout.txt")
}

// ───────────────────────── 자동 분류 규칙 ─────────────────────────
pub struct Rule {
    pub pattern: String, // "*.pdf;*.docx" | "<dir>" | "*"
    pub target: String,  // Fence 제목
}

pub fn rule_matches(pattern: &str, path: &Path) -> bool {
    let name = lower(path.file_name().and_then(|s| s.to_str()).unwrap_or(""));
    for pat in pattern.split(';').map(|p| p.trim()).filter(|p| !p.is_empty()) {
        if pat == "<dir>" {
            if path.is_dir() {
                return true;
            }
        } else if !path.is_dir() && glob_match(&lower(pat), &name) {
            return true;
        }
    }
    false
}

// 첫 매칭 규칙의 대상 Fence 제목
pub fn rule_target(path: &Path) -> Option<String> {
    app().rules.iter().find(|r| rule_matches(&r.pattern, path)).map(|r| r.target.clone())
}

// 파일 하나로부터 규칙 패턴 제안: 폴더 → <dir>, 확장자 있으면 *.ext, 없으면 파일명
pub fn suggest_pattern(path: &Path) -> String {
    if path.is_dir() {
        "<dir>".to_string()
    } else {
        let ext = file_ext(path);
        if ext.is_empty() {
            path.file_name().and_then(|s| s.to_str()).unwrap_or("*").to_string()
        } else {
            format!("*.{}", ext)
        }
    }
}

// ───────────────────────── 직렬화 ─────────────────────────
pub fn serialize() -> String {
    let a = app();
    let mut s = String::new();
    s.push_str(&format!("icons_hidden={}\n", a.icons_hidden as u8));
    s.push_str(&format!("fences_hidden={}\n", a.fences_hidden as u8));
    s.push_str(&format!("hover_expand={}\n", a.hover_expand as u8));
    s.push_str(&format!("hot_corner={}\n", a.hot_corner));
    s.push_str(&format!("hotkey_toggle={}\n", hotkey_to_string(a.hotkey_toggle)));
    s.push_str(&format!("hotkey_peek={}\n", hotkey_to_string(a.hotkey_peek)));
    for r in &a.rules {
        s.push_str(&format!("rule={}|{}\n", r.pattern, r.target));
    }
    for f in &a.fences {
        s.push_str("[fence]\n");
        s.push_str(&format!("title={}\n", f.title));
        s.push_str(&format!("rect={},{},{},{}\n", f.x, f.y, f.w, f.h));
        s.push_str(&format!("color={:06X}\n", f.color));
        s.push_str(&format!("rolled={}\n", f.rolled as u8));
        s.push_str(&format!("icon_size={}\n", f.icon_size));
        s.push_str(&format!("alpha={}\n", f.alpha));
        s.push_str(&format!("title_hidden={}\n", f.title_hidden as u8));
        s.push_str(&format!("locked={}\n", f.locked as u8));
        s.push_str(&format!("auto_height={}\n", f.auto_h as u8));
        if let Some(p) = &f.portal {
            s.push_str(&format!("portal={}\n", p.to_string_lossy()));
            s.push_str(&format!("sort={}\n", f.view.sort.as_str()));
            s.push_str(&format!("sort_desc={}\n", f.view.desc as u8));
            s.push_str(&format!("dirs_first={}\n", f.view.dirs_first as u8));
            s.push_str(&format!("show_hidden={}\n", f.view.show_hidden as u8));
        } else {
            for it in &f.items {
                s.push_str(&format!("item={}\n", it.path));
            }
        }
    }
    s
}

pub fn save() {
    let p = config_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, serialize());
}

// 텍스트를 파싱해 App 의 전역 설정과 fences 목록(창 없음)을 채운다.
pub fn parse_into(text: &str) {
    let a = app();
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
            ("fences_hidden", None) => a.fences_hidden = v == "1",
            ("hover_expand", None) => a.hover_expand = v == "1",
            ("hot_corner", None) => a.hot_corner = v.parse().unwrap_or(0),
            ("hotkey_toggle", None) => {
                if let Some(h) = parse_hotkey(v) {
                    a.hotkey_toggle = h;
                }
            }
            ("hotkey_peek", None) => {
                if let Some(h) = parse_hotkey(v) {
                    a.hotkey_peek = h;
                }
            }
            ("rule", None) => {
                if let Some((p, t)) = v.split_once('|') {
                    a.rules.push(Rule { pattern: p.trim().to_string(), target: t.trim().to_string() });
                }
            }
            ("title", Some(f)) => f.title = v.to_string(),
            ("rect", Some(f)) => {
                let n: Vec<i32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
                if n.len() == 4 {
                    f.x = n[0];
                    f.y = n[1];
                    f.w = n[2].max(MIN_W);
                    f.h = n[3].max(TITLE_H + 40);
                }
            }
            ("color", Some(f)) => f.color = u32::from_str_radix(v, 16).unwrap_or(COLORS[0].1),
            ("rolled", Some(f)) => f.rolled = v == "1",
            ("icon_size", Some(f)) => f.icon_size = match v.parse::<i32>().unwrap_or(32) {
                16 => 16,
                48 => 48,
                _ => 32,
            },
            ("alpha", Some(f)) => f.alpha = v.parse::<u8>().unwrap_or(ALPHA).max(40),
            ("title_hidden", Some(f)) => f.title_hidden = v == "1",
            ("locked", Some(f)) => f.locked = v == "1",
            ("auto_height", Some(f)) => f.auto_h = v == "1",
            ("sort", Some(f)) => f.view.sort = SortKey::parse(v),
            ("sort_desc", Some(f)) => f.view.desc = v == "1",
            ("dirs_first", Some(f)) => f.view.dirs_first = v == "1",
            ("show_hidden", Some(f)) => f.view.show_hidden = v == "1",
            ("portal", Some(f)) => f.portal = Some(PathBuf::from(v)),
            ("item", Some(f)) => f.items.push(Item::new(v)),
            _ => {}
        }
    }
    if let Some(f) = cur.take() {
        a.fences.push(f);
    }
}

pub fn load() -> bool {
    let Ok(text) = std::fs::read_to_string(config_path()) else { return false };
    parse_into(&text);
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
