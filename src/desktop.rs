// 바탕화면(explorer) 연동: 창 탐색, 파일 스캔, 재검색, 아이콘 숨김
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

// ───────────────────────── 바탕화면 탐색 ─────────────────────────
pub unsafe fn find_desktop() -> (HWND, HWND) {
    let progman = FindWindowW(wide("Progman").as_ptr(), null());
    let mut dv = if progman.is_null() {
        null_mut()
    } else {
        FindWindowExW(progman, null_mut(), wide("SHELLDLL_DefView").as_ptr(), null())
    };
    if dv.is_null() {
        let mut ww: HWND = null_mut();
        loop {
            ww = FindWindowExW(GetDesktopWindow(), ww, wide("WorkerW").as_ptr(), null());
            if ww.is_null() {
                break;
            }
            dv = FindWindowExW(ww, null_mut(), wide("SHELLDLL_DefView").as_ptr(), null());
            if !dv.is_null() {
                break;
            }
        }
    }
    let lv = if dv.is_null() {
        null_mut()
    } else {
        FindWindowExW(dv, null_mut(), wide("SysListView32").as_ptr(), null())
    };
    (dv, lv)
}

pub unsafe fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    let mut p: PWSTR = null_mut();
    if SHGetKnownFolderPath(id, 0, null_mut(), &mut p) < 0 || p.is_null() {
        return None;
    }
    let len = (0..).take_while(|&i| *p.add(i) != 0).count();
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
    CoTaskMemFree(p as *const c_void);
    Some(PathBuf::from(s))
}

pub fn scan_desktop(dirs: &[PathBuf]) -> Vec<PathBuf> {
    use std::os::windows::fs::MetadataExt;
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.eq_ignore_ascii_case("desktop.ini") {
                continue;
            }
            if let Ok(md) = e.metadata() {
                if md.file_attributes() & 0x2 != 0 {
                    continue; // 숨김 파일
                }
            }
            out.push(p);
        }
    }
    out
}

// ───────────────────────── 바탕화면 재검색 ─────────────────────────
pub unsafe fn rescan() {
    let a = app();
    // 탐색기 재시작 등으로 부모가 사라졌으면 다시 부착
    if !a.defview.is_null() && IsWindow(a.defview) == 0 {
        reattach();
    }
    let current = scan_desktop(&a.desktop_dirs);
    let set: HashSet<String> = current.iter().map(|p| lower(&p.to_string_lossy())).collect();
    let mut changed = false;
    for f in a.fences.iter_mut() {
        let before = f.items.len();
        f.items.retain(|it| set.contains(&lower(&it.path)));
        if f.items.len() != before {
            f.sel = None;
            f.clamp_scroll();
            InvalidateRect(f.hwnd, null(), 1);
            changed = true;
        }
    }
    let assigned: HashSet<String> =
        a.fences.iter().flat_map(|f| f.items.iter().map(|i| lower(&i.path))).collect();
    let new: Vec<String> = current
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !assigned.contains(&lower(p)))
        .collect();
    if !new.is_empty() {
        let idx = match a.fences.iter().position(|f| f.title == NEW_ITEMS_TITLE) {
            Some(i) => i,
            None => {
                let sw = GetSystemMetrics(SM_CXSCREEN);
                new_fence(NEW_ITEMS_TITLE, sw - m(350), m(20), m(330), m(300), COLORS[0].1)
            }
        };
        for p in new {
            app().fences[idx].items.push(Item::new(&p));
        }
        invalidate(idx);
        changed = true;
    }
    if changed {
        save();
    }
}

pub unsafe fn reattach() {
    let a = app();
    let (dv, lv) = find_desktop();
    a.defview = dv;
    a.listview = lv;
    for i in 0..a.fences.len() {
        let old = a.fences[i].hwnd;
        if !old.is_null() {
            DestroyWindow(old);
        }
        create_fence_window(i);
    }
    apply_icons_hidden();
}

// 바탕화면 아이콘 표시/숨김: Windows 11 에서는 동기 ShowWindow 와 WM_COMMAND 0x7402 토글이
// 모두 무시되고, ShowWindowAsync 만 explorer 의 SysListView32 에 적용된다.
pub unsafe fn apply_icons_hidden() {
    let a = app();
    let (dv, lv) = find_desktop();
    if !dv.is_null() {
        a.listview = lv;
    }
    if a.listview.is_null() || IsWindow(a.listview) == 0 {
        return;
    }
    let visible = IsWindowVisible(a.listview) != 0;
    if visible == a.icons_hidden {
        ShowWindowAsync(a.listview, if a.icons_hidden { SW_HIDE } else { SW_SHOW });
    }
}

pub unsafe fn auto_sort() {
    let a = app();
    while !a.fences.is_empty() {
        delete_fence(0);
    }
    let files = scan_desktop(&a.desktop_dirs);
    let mut groups: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for p in files {
        let ext = p.extension().and_then(|e| e.to_str()).map(lower).unwrap_or_default();
        let g = if p.is_dir() {
            0
        } else if matches!(ext.as_str(), "lnk" | "exe" | "url" | "appref-ms") {
            1
        } else {
            2
        };
        groups[g].push(p.to_string_lossy().to_string());
    }
    let titles = ["폴더", "프로그램", "문서 및 파일"];
    let (fw, fh, gap) = (m(330), m(300), m(16));
    let sh = GetSystemMetrics(SM_CYSCREEN);
    let (mut x, mut y) = (m(20), m(20));
    for (gi, g) in groups.iter().enumerate() {
        if g.is_empty() {
            continue;
        }
        if y + fh > sh {
            x += fw + gap;
            y = m(20);
        }
        let idx = new_fence(titles[gi], x, y, fw, fh, COLORS[(gi + 1) % COLORS.len()].1);
        for p in g {
            app().fences[idx].items.push(Item::new(p));
        }
        invalidate(idx);
        y += fh + gap;
    }
    save();
}

// 화면 좌표가 Fence 가 아닌 바탕화면(Progman/WorkerW) 위인지
pub unsafe fn is_empty_desktop(pt: POINT) -> bool {
    let target = WindowFromPoint(pt);
    if target.is_null() || fence_idx(target).is_some() {
        return false;
    }
    let root = GetAncestor(target, GA_ROOT);
    let mut buf = [0u16; 64];
    let n = GetClassNameW(root, buf.as_mut_ptr(), buf.len() as i32);
    let cls = from_wide(&buf[..n as usize]);
    cls == "Progman" || cls == "WorkerW"
}
