// 바탕화면(explorer) 연동: 창 탐색, 파일 스캔, 재검색(규칙 분류), 아이콘 숨김, 자동 정리
#![allow(unused_imports, dead_code)]
use crate::*;
use core::ffi::c_void;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

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

const ATTR_HIDDEN: u32 = 0x2;
const ATTR_SYSTEM: u32 = 0x4;

// 폴더 내용과 메타데이터(desktop.ini 제외). Windows 에서 DirEntry::metadata 는 열거 결과를 재사용하므로 추가 I/O 가 없다.
// show_hidden 이 false 면 숨김 파일을 빼고, true 여도 숨김+시스템(보호된 OS 파일)은 뺀다.
pub fn scan_entries(d: &Path, show_hidden: bool) -> Vec<(PathBuf, std::fs::Metadata)> {
    use std::os::windows::fs::MetadataExt;
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(d) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }
        let Ok(md) = e.metadata() else { continue };
        let attr = md.file_attributes();
        let hidden = attr & ATTR_HIDDEN != 0;
        if hidden && (!show_hidden || attr & ATTR_SYSTEM != 0) {
            continue;
        }
        out.push((p, md));
    }
    out
}

// 폴더 내용(숨김 파일·desktop.ini 제외)
pub fn scan_dir(d: &Path) -> Vec<PathBuf> {
    scan_entries(d, false).into_iter().map(|(p, _)| p).collect()
}

pub fn scan_desktop(dirs: &[PathBuf]) -> Vec<PathBuf> {
    dirs.iter().flat_map(|d| scan_dir(d)).collect()
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    from_wide(&buf[..n as usize])
}

pub fn is_desktop_root(hwnd: HWND) -> bool {
    if hwnd.is_null() {
        return false;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    let cls = class_name(root);
    cls == "Progman" || cls == "WorkerW"
}

// 화면 좌표가 Fence 가 아닌 바탕화면(Progman/WorkerW) 위인지
pub unsafe fn is_empty_desktop(pt: POINT) -> bool {
    let target = WindowFromPoint(pt);
    if target.is_null() || fence_idx(target).is_some() {
        return false;
    }
    is_desktop_root(target)
}

pub fn desktop_is_foreground() -> bool {
    let fg = unsafe { GetForegroundWindow() };
    is_desktop_root(fg)
}

// 제목으로 Fence 찾기, 없으면 화면 우측 상단에 생성
pub unsafe fn fence_by_title_or_create(title: &str) -> usize {
    let a = app();
    if let Some(i) = a.fences.iter().position(|f| f.title == title && f.portal.is_none()) {
        return i;
    }
    let sw = GetSystemMetrics(SM_CXSCREEN);
    let sh = GetSystemMetrics(SM_CYSCREEN);
    let n = a.fences.len() as i32;
    let (fw, fh) = (m(330), m(300));
    let y = (m(20) + (n % 3) * (fh + m(16))).min(sh - fh);
    let x = (sw - m(350) - (n / 3) * (fw + m(16))).max(0);
    let color = COLORS[(n as usize) % COLORS.len()].1;
    new_fence(title, x, y, fw, fh, color)
}

// ───────────────────────── 바탕화면 재검색 ─────────────────────────
pub unsafe fn rescan() {
    let a = app();
    // 탐색기 재시작 등으로 부모가 사라졌으면 다시 부착
    if !a.defview.is_null() && IsWindow(a.defview) == 0 {
        reattach();
    }
    // 포털은 자기 폴더만 갱신
    for i in 0..a.fences.len() {
        if a.fences[i].portal.is_some() {
            refresh_portal(i);
        }
    }
    let current = scan_desktop(&a.desktop_dirs);
    let set: HashSet<String> = current.iter().map(|p| lower(&p.to_string_lossy())).collect();
    let mut changed = false;
    for f in a.fences.iter_mut().filter(|f| f.portal.is_none()) {
        let before = f.items.len();
        f.items.retain(|it| set.contains(&lower(&it.path)));
        if f.items.len() != before {
            f.sel.clear();
            f.clamp_scroll();
            InvalidateRect(f.hwnd, null(), 1);
            changed = true;
        }
    }
    let assigned: HashSet<String> = a
        .fences
        .iter()
        .filter(|f| f.portal.is_none())
        .flat_map(|f| f.items.iter().map(|i| lower(&i.path)))
        .collect();
    let new: Vec<PathBuf> = current.into_iter().filter(|p| !assigned.contains(&lower(&p.to_string_lossy()))).collect();
    for p in new {
        let title = rule_target(&p).unwrap_or_else(|| NEW_ITEMS_TITLE.to_string());
        let idx = fence_by_title_or_create(&title);
        app().fences[idx].items.push(Item::new(&p.to_string_lossy()));
        invalidate(idx);
        changed = true;
    }
    if changed {
        for i in 0..app().fences.len() {
            apply_auto_height(i);
        }
        save();
    }
}

// 모든(포털 제외) 항목을 규칙대로 다시 배치
pub unsafe fn reapply_rules() {
    let a = app();
    let mut all: Vec<String> = Vec::new();
    for f in a.fences.iter_mut().filter(|f| f.portal.is_none()) {
        all.extend(f.items.drain(..).map(|i| i.path.clone()));
        f.sel.clear();
    }
    for p in all {
        let path = PathBuf::from(&p);
        let title = rule_target(&path).unwrap_or_else(|| NEW_ITEMS_TITLE.to_string());
        let idx = fence_by_title_or_create(&title);
        app().fences[idx].items.push(Item::new(&p));
    }
    for i in 0..app().fences.len() {
        invalidate(i);
    }
    save();
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

pub fn seed_default_rules() {
    app().rules = vec![
        Rule { pattern: "<dir>".into(), target: "폴더".into() },
        Rule { pattern: "*.lnk;*.exe;*.url;*.appref-ms".into(), target: "프로그램".into() },
        Rule { pattern: "*".into(), target: "문서 및 파일".into() },
    ];
}

// 최초 실행/트레이 메뉴: 폴더·프로그램·문서로 나누고 기본 규칙 등록
pub unsafe fn auto_sort() {
    let a = app();
    // 포털은 유지, 일반 Fence 만 제거
    let mut i = 0;
    while i < a.fences.len() {
        if a.fences[i].portal.is_none() {
            delete_fence(i);
        } else {
            i += 1;
        }
    }
    seed_default_rules();
    let files = scan_desktop(&a.desktop_dirs);
    let titles = ["폴더", "프로그램", "문서 및 파일"];
    let mut groups: Vec<Vec<String>> = vec![Vec::new(); 3];
    for p in files {
        let t = rule_target(&p).unwrap_or_default();
        let g = titles.iter().position(|x| *x == t).unwrap_or(2);
        groups[g].push(p.to_string_lossy().to_string());
    }
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

// 화면 밖으로 나간 Fence 를 가상 화면 안으로 당긴다 (해상도 변경 대응)
pub unsafe fn clamp_fences_to_screen() {
    let a = app();
    let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
    let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
    for i in 0..a.fences.len() {
        let f = &mut a.fences[i];
        let nx = f.x.clamp(vx, (vx + vw - f.w).max(vx));
        let ny = f.y.clamp(vy, (vy + vh - m(TITLE_H)).max(vy));
        if nx != f.x || ny != f.y {
            f.x = nx;
            f.y = ny;
            SetWindowPos(f.hwnd, null_mut(), nx, ny, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}
