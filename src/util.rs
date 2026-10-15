// 공용 유틸
#![allow(unused_imports, dead_code)]
use crate::*;
use std::path::Path;
use windows_sys::Win32::Foundation::*;

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
pub fn file_ext(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).map(lower).unwrap_or_default()
}

pub fn norm_rect(a: POINT, b: POINT) -> (i32, i32, i32, i32) {
    (a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
}

// 간단한 glob 매칭: `*` 는 임의 문자열, `?` 는 한 글자. 대소문자 무시는 호출 측에서 lower() 처리.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

// "ctrl+shift+F" → (MOD_*, vk). 실패하면 None.
pub fn parse_hotkey(s: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut vk = 0u32;
    for tok in s.split('+').map(|t| t.trim()).filter(|t| !t.is_empty()) {
        match tok.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= 0x0002,
            "shift" => mods |= 0x0004,
            "alt" => mods |= 0x0001,
            "win" => mods |= 0x0008,
            other => {
                let up = other.to_ascii_uppercase();
                if up.len() == 1 {
                    vk = up.as_bytes()[0] as u32;
                } else if let Some(n) = up.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
                    if (1..=24).contains(&n) {
                        vk = 0x70 + n - 1;
                    }
                }
            }
        }
    }
    if vk == 0 {
        None
    } else {
        Some((mods, vk))
    }
}
pub fn hotkey_to_string(hk: (u32, u32)) -> String {
    let mut parts = Vec::new();
    if hk.0 & 0x0002 != 0 {
        parts.push("Ctrl".to_string());
    }
    if hk.0 & 0x0004 != 0 {
        parts.push("Shift".to_string());
    }
    if hk.0 & 0x0001 != 0 {
        parts.push("Alt".to_string());
    }
    if hk.0 & 0x0008 != 0 {
        parts.push("Win".to_string());
    }
    let key = if (0x70..0x88).contains(&hk.1) {
        format!("F{}", hk.1 - 0x70 + 1)
    } else {
        ((hk.1 as u8) as char).to_string()
    };
    parts.push(key);
    parts.join("+")
}
