// icondocs(아이콘독스) — Fences 스타일의 가벼운 바탕화면 아이콘 정리 도구 (Win32 API 직접 호출, 런타임 없음)
#![windows_subsystem = "windows"]
#![allow(non_snake_case, static_mut_refs, clippy::missing_safety_doc)]

use core::ffi::c_void;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::core::PWSTR;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ───────────────────────── 상수 ─────────────────────────
const CLASS_FENCE: &str = "IconDocsFence";
const CLASS_MAIN: &str = "IconDocsMain";
const CLASS_RENAME: &str = "IconDocsRename";
const NEW_ITEMS_TITLE: &str = "새 항목";

const TITLE_H: i32 = 30;
const CELL_W: i32 = 84;
const CELL_H: i32 = 88;
const PAD: i32 = 8;
const BORDER: i32 = 6;
const ALPHA: u8 = 215;
const MIN_W: i32 = 120;

const EM_SETSEL: u32 = 0x00B1;
const WM_TRAY: u32 = WM_APP + 1;
const WM_DESKTOP_DBLCLK: u32 = WM_APP + 2;
const WM_BAND_DOWN: u32 = WM_APP + 4;
const WM_BAND_MOVE: u32 = WM_APP + 5;
const WM_BAND_UP: u32 = WM_APP + 6;
const CLASS_BAND: &str = "IconDocsBand";
const TIMER_RESCAN: usize = 1;

const ID_TRAY_NEW: usize = 101;
const ID_TRAY_ICONS: usize = 102;
const ID_TRAY_HIDE: usize = 103;
const ID_TRAY_AUTORUN: usize = 104;
const ID_TRAY_EXIT: usize = 105;
const ID_TRAY_RESORT: usize = 106;
const ID_F_RENAME: usize = 201;
const ID_F_DELETE: usize = 202;
const ID_F_ROLL: usize = 203;
const ID_F_SORT: usize = 204;
const ID_F_COLOR0: usize = 210;
const ID_I_OPEN: usize = 301;
const ID_I_LOC: usize = 302;
const ID_I_REMOVE: usize = 303;

// COLORREF 는 0x00BBGGRR
const COLORS: [(&str, u32); 5] = [
    ("어두움", 0x2A2A2A),
    ("파랑", 0x6B3F1F),
    ("초록", 0x335522),
    ("보라", 0x622B4E),
    ("주황", 0x14468C),
];

// ───────────────────────── 유틸 ─────────────────────────
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn from_wide(p: &[u16]) -> String {
    let n = p.iter().position(|&c| c == 0).unwrap_or(p.len());
    String::from_utf16_lossy(&p[..n])
}
fn lower(s: &str) -> String {
    s.to_lowercase()
}
fn darken(c: u32, f: u32) -> u32 {
    let r = (c & 0xFF) * f / 100;
    let g = ((c >> 8) & 0xFF) * f / 100;
    let b = ((c >> 16) & 0xFF) * f / 100;
    r | (g << 8) | (b << 16)
}
fn lighten(c: u32, add: u32) -> u32 {
    let r = ((c & 0xFF) + add).min(255);
    let g = (((c >> 8) & 0xFF) + add).min(255);
    let b = (((c >> 16) & 0xFF) + add).min(255);
    r | (g << 8) | (b << 16)
}
fn display_name(p: &Path) -> String {
    let ext = p.extension().and_then(|e| e.to_str()).map(lower).unwrap_or_default();
    if ext == "lnk" || ext == "url" {
        p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string()
    } else {
        p.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string()
    }
}

// ───────────────────────── 데이터 ─────────────────────────
struct Item {
    path: String,
    name: String,
    icon: HICON,
}
impl Item {
    fn new(path: &str) -> Item {
        Item { path: path.to_string(), name: display_name(Path::new(path)), icon: null_mut() }
    }
}
impl Drop for Item {
    fn drop(&mut self) {
        if !self.icon.is_null() {
            unsafe { DestroyIcon(self.icon) };
        }
    }
}

struct Fence {
    hwnd: HWND,
    title: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    color: u32,
    rolled: bool,
    items: Vec<Item>,
    scroll: i32,
    sel: Option<usize>,
    drag: Option<(usize, POINT)>,
    dragging: bool,
}

struct App {
    hinst: HINSTANCE,
    main: HWND,
    defview: HWND,
    listview: HWND,
    fences: Vec<Fence>,
    fences_hidden: bool,
    icons_hidden: bool,
    font: HFONT,
    font_b: HFONT,
    hook: HHOOK,
    desktop_dirs: Vec<PathBuf>,
    last_click: (u32, i32, i32),
    band_start: Option<POINT>, // 바탕화면 드래그로 새 Fence 만들기
    band_hwnd: HWND,
    dpi: i32,
    icon_px: i32,
}

static mut APP: *mut App = null_mut();
static mut EDIT_OLD_PROC: isize = 0;

fn app() -> &'static mut App {
    unsafe { &mut *APP }
}
fn m(v: i32) -> i32 {
    v * app().dpi / 96
}

// ───────────────────────── 바탕화면 탐색 ─────────────────────────
unsafe fn find_desktop() -> (HWND, HWND) {
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

unsafe fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    let mut p: PWSTR = null_mut();
    if SHGetKnownFolderPath(id, 0, null_mut(), &mut p) < 0 || p.is_null() {
        return None;
    }
    let len = (0..).take_while(|&i| *p.add(i) != 0).count();
    let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
    CoTaskMemFree(p as *const c_void);
    Some(PathBuf::from(s))
}

fn scan_desktop(dirs: &[PathBuf]) -> Vec<PathBuf> {
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

fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    base.join("IconDocs").join("layout.txt")
}

// ───────────────────────── 저장/불러오기 ─────────────────────────
fn save() {
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

fn load() -> bool {
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

// ───────────────────────── Fence ─────────────────────────
impl Fence {
    fn new(title: &str, x: i32, y: i32, w: i32, h: i32, color: u32) -> Fence {
        Fence {
            hwnd: null_mut(),
            title: title.to_string(),
            x,
            y,
            w,
            h,
            color,
            rolled: false,
            items: Vec::new(),
            scroll: 0,
            sel: None,
            drag: None,
            dragging: false,
        }
    }
    fn cols(&self) -> usize {
        (((self.w - 2 * m(PAD)) / m(CELL_W)).max(1)) as usize
    }
    fn content_h(&self) -> i32 {
        let rows = self.items.len().div_ceil(self.cols()) as i32;
        m(TITLE_H + PAD) + rows * m(CELL_H) + m(PAD)
    }
    fn clamp_scroll(&mut self) {
        let max = (self.content_h() - self.h).max(0);
        self.scroll = self.scroll.clamp(0, max);
    }
    fn cell_rect(&self, i: usize) -> RECT {
        let cols = self.cols();
        let (col, row) = ((i % cols) as i32, (i / cols) as i32);
        let cx = m(PAD) + col * m(CELL_W);
        let cy = m(TITLE_H + PAD) + row * m(CELL_H) - self.scroll;
        RECT { left: cx, top: cy, right: cx + m(CELL_W), bottom: cy + m(CELL_H) }
    }
    fn hit_item(&self, x: i32, y: i32) -> Option<usize> {
        if y < m(TITLE_H) {
            return None;
        }
        (0..self.items.len()).find(|&i| {
            let r = self.cell_rect(i);
            x >= r.left && x < r.right && y >= r.top && y < r.bottom
        })
    }
    fn has(&self, path: &str) -> bool {
        let l = lower(path);
        self.items.iter().any(|i| lower(&i.path) == l)
    }
}

fn fence_idx(hwnd: HWND) -> Option<usize> {
    app().fences.iter().position(|f| f.hwnd == hwnd)
}

unsafe fn create_fence_window(idx: usize) {
    let a = app();
    let child = !a.defview.is_null() && IsWindow(a.defview) != 0;
    let f = &a.fences[idx];
    // 다른 프로세스(explorer)의 창을 부모로 직접 지정해 만들면 실패하므로
    // 먼저 최상위 팝업으로 만든 뒤 SetParent 로 바탕화면 레이어에 붙인다.
    let exstyle = WS_EX_LAYERED | WS_EX_ACCEPTFILES | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    let h = if f.rolled { m(TITLE_H) } else { f.h };
    let hwnd = CreateWindowExW(
        exstyle,
        wide(CLASS_FENCE).as_ptr(),
        wide(&f.title).as_ptr(),
        WS_POPUP | WS_THICKFRAME | WS_CLIPSIBLINGS,
        f.x,
        f.y,
        f.w,
        h,
        null_mut(),
        null_mut(),
        a.hinst,
        null(),
    );
    if child && !hwnd.is_null() {
        let st = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        SetWindowLongPtrW(hwnd, GWL_STYLE, ((st & !WS_POPUP) | WS_CHILD) as isize);
        SetParent(hwnd, a.defview);
        SetWindowPos(hwnd, null_mut(), f.x, f.y, f.w, h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    }
    a.fences[idx].hwnd = hwnd;
    SetLayeredWindowAttributes(hwnd, 0, ALPHA, LWA_ALPHA);
    DragAcceptFiles(hwnd, 1);
    apply_region(hwnd);
    if !a.fences_hidden {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    if !child {
        SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

unsafe fn apply_region(hwnd: HWND) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(hwnd, &mut r);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let rad = m(12);
    SetWindowRgn(hwnd, CreateRoundRectRgn(0, 0, w + 1, h + 1, rad, rad), 1);
}

unsafe fn new_fence(title: &str, x: i32, y: i32, w: i32, h: i32, color: u32) -> usize {
    app().fences.push(Fence::new(title, x, y, w, h, color));
    let idx = app().fences.len() - 1;
    create_fence_window(idx);
    idx
}

unsafe fn delete_fence(idx: usize) {
    let f = app().fences.remove(idx);
    if !f.hwnd.is_null() {
        DestroyWindow(f.hwnd);
    }
    save();
}

unsafe fn set_rolled(idx: usize, rolled: bool) {
    let f = &mut app().fences[idx];
    f.rolled = rolled;
    let h = if rolled { m(TITLE_H) } else { f.h };
    SetWindowPos(f.hwnd, null_mut(), 0, 0, f.w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    apply_region(f.hwnd);
    InvalidateRect(f.hwnd, null(), 1);
    save();
}

unsafe fn invalidate(idx: usize) {
    let f = &mut app().fences[idx];
    f.clamp_scroll();
    InvalidateRect(f.hwnd, null(), 1);
}

// 다른 Fence 에 들어 있는 같은 경로를 제거한 뒤 대상 Fence 에 추가
unsafe fn add_item_to(idx: usize, path: &str) {
    let a = app();
    let l = lower(path);
    for (i, f) in a.fences.iter_mut().enumerate() {
        if i != idx {
            let before = f.items.len();
            f.items.retain(|it| lower(&it.path) != l);
            if f.items.len() != before {
                InvalidateRect(f.hwnd, null(), 1);
            }
        }
    }
    if !a.fences[idx].has(path) {
        a.fences[idx].items.push(Item::new(path));
    }
    invalidate(idx);
}

// ───────────────────────── 바탕화면 재검색 ─────────────────────────
unsafe fn rescan() {
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

unsafe fn reattach() {
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
unsafe fn apply_icons_hidden() {
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

unsafe fn auto_sort() {
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

// ───────────────────────── 그리기 ─────────────────────────
unsafe fn paint(hwnd: HWND, idx: usize) {
    let a = app();
    let mut ps: PAINTSTRUCT = core::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(hwnd, &mut rc);
    let (w, h) = (rc.right, rc.bottom);
    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w.max(1), h.max(1));
    let old_bmp = SelectObject(mem, bmp as HGDIOBJ);
    let f = &a.fences[idx];

    // 배경
    let br = CreateSolidBrush(f.color);
    FillRect(mem, &rc, br);
    DeleteObject(br as HGDIOBJ);
    // 제목 표시줄
    let th = m(TITLE_H);
    let trc = RECT { left: 0, top: 0, right: w, bottom: th };
    let br = CreateSolidBrush(darken(f.color, 70));
    FillRect(mem, &trc, br);
    DeleteObject(br as HGDIOBJ);

    SetBkMode(mem, TRANSPARENT as i32);
    SetTextColor(mem, 0xF0F0F0);
    let old_font = SelectObject(mem, a.font_b as HGDIOBJ);
    let mut tr = RECT { left: m(PAD) + 4, top: 0, right: w - m(PAD), bottom: th };
    let title = wide(&format!("{}  ({})", f.title, f.items.len()));
    DrawTextW(mem, title.as_ptr(), -1, &mut tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
    SelectObject(mem, a.font as HGDIOBJ);

    if !f.rolled {
        let old_pen = SelectObject(mem, GetStockObject(NULL_PEN));
        let icon_px = a.icon_px;
        let f = &mut a.fences[idx];
        for i in 0..f.items.len() {
            let r = f.cell_rect(i);
            if r.bottom < th || r.top > h {
                continue;
            }
            if f.sel == Some(i) {
                let sb = CreateSolidBrush(lighten(f.color, 45));
                let old = SelectObject(mem, sb as HGDIOBJ);
                RoundRect(mem, r.left, r.top, r.right, r.bottom, m(8), m(8));
                SelectObject(mem, old);
                DeleteObject(sb as HGDIOBJ);
            }
            let it = &mut f.items[i];
            if it.icon.is_null() {
                let mut sfi: SHFILEINFOW = core::mem::zeroed();
                SHGetFileInfoW(
                    wide(&it.path).as_ptr(),
                    0,
                    &mut sfi,
                    core::mem::size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON,
                );
                it.icon = sfi.hIcon;
            }
            let ix = r.left + (m(CELL_W) - icon_px) / 2;
            let iy = r.top + m(6);
            if !it.icon.is_null() {
                DrawIconEx(mem, ix, iy, it.icon, icon_px, icon_px, 0, null_mut(), DI_NORMAL);
            }
            let mut txt = RECT { left: r.left + 2, top: iy + icon_px + m(4), right: r.right - 2, bottom: r.bottom - 2 };
            let name = wide(&it.name);
            DrawTextW(mem, name.as_ptr(), -1, &mut txt, DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS | DT_NOPREFIX | DT_EDITCONTROL);
        }
        // 스크롤 표시
        let content = f.content_h();
        if content > h {
            let track = h - th - 4;
            let bar_h = ((h as i64 * track as i64) / content as i64).max(m(16) as i64) as i32;
            let bar_y = th + 2 + ((f.scroll as i64 * (track - bar_h) as i64) / (content - h).max(1) as i64) as i32;
            let sb = CreateSolidBrush(lighten(f.color, 90));
            let old = SelectObject(mem, sb as HGDIOBJ);
            RoundRect(mem, w - 6, bar_y, w - 2, bar_y + bar_h, 3, 3);
            SelectObject(mem, old);
            DeleteObject(sb as HGDIOBJ);
        }
        SelectObject(mem, old_pen);
    }

    SelectObject(mem, old_font);
    BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old_bmp);
    DeleteObject(bmp as HGDIOBJ);
    DeleteDC(mem);
    EndPaint(hwnd, &ps);
}

// ───────────────────────── 메뉴 ─────────────────────────
unsafe fn track_menu(menu: HMENU, x: i32, y: i32) -> usize {
    let a = app();
    SetForegroundWindow(a.main);
    let r = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, x, y, 0, a.main, null());
    PostMessageW(a.main, WM_NULL, 0, 0);
    DestroyMenu(menu);
    r as usize
}

unsafe fn menu_item(menu: HMENU, id: usize, text: &str, checked: bool) {
    let fl = MF_STRING | if checked { MF_CHECKED } else { 0 };
    AppendMenuW(menu, fl, id, wide(text).as_ptr());
}

unsafe fn fence_menu(idx: usize, x: i32, y: i32) {
    let menu = CreatePopupMenu();
    menu_item(menu, ID_F_RENAME, "이름 바꾸기", false);
    menu_item(menu, ID_F_ROLL, "제목만 표시 (말아 올리기)", app().fences[idx].rolled);
    menu_item(menu, ID_F_SORT, "이름순 정렬", false);
    let sub = CreatePopupMenu();
    for (i, (name, c)) in COLORS.iter().enumerate() {
        menu_item(sub, ID_F_COLOR0 + i, name, app().fences[idx].color == *c);
    }
    AppendMenuW(menu, MF_POPUP, sub as usize, wide("색상").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    menu_item(menu, ID_F_DELETE, "Fence 삭제", false);
    match track_menu(menu, x, y) {
        ID_F_RENAME => open_rename(idx),
        ID_F_ROLL => set_rolled(idx, !app().fences[idx].rolled),
        ID_F_SORT => {
            let f = &mut app().fences[idx];
            f.items.sort_by_key(|i| lower(&i.name));
            f.sel = None;
            invalidate(idx);
            save();
        }
        ID_F_DELETE => delete_fence(idx),
        c if (ID_F_COLOR0..ID_F_COLOR0 + COLORS.len()).contains(&c) => {
            app().fences[idx].color = COLORS[c - ID_F_COLOR0].1;
            invalidate(idx);
            save();
        }
        _ => {}
    }
}

unsafe fn item_menu(idx: usize, item: usize, x: i32, y: i32) {
    let menu = CreatePopupMenu();
    menu_item(menu, ID_I_OPEN, "열기", false);
    menu_item(menu, ID_I_LOC, "파일 위치 열기", false);
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    menu_item(menu, ID_I_REMOVE, "이 Fence 에서 제거", false);
    let path = app().fences[idx].items[item].path.clone();
    match track_menu(menu, x, y) {
        ID_I_OPEN => open_path(&path),
        ID_I_LOC => {
            let args = wide(&format!("/select,\"{}\"", path));
            ShellExecuteW(null_mut(), wide("open").as_ptr(), wide("explorer.exe").as_ptr(), args.as_ptr(), null(), SW_SHOWNORMAL);
        }
        ID_I_REMOVE => {
            let f = &mut app().fences[idx];
            if item < f.items.len() {
                f.items.remove(item);
            }
            f.sel = None;
            invalidate(idx);
            save();
            rescan(); // '새 항목' 으로 이동
        }
        _ => {}
    }
}

unsafe fn open_path(path: &str) {
    ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(path).as_ptr(), null(), null(), SW_SHOWNORMAL);
}

unsafe fn tray_menu() {
    let a = app();
    let menu = CreatePopupMenu();
    menu_item(menu, ID_TRAY_NEW, "새 Fence 만들기", false);
    menu_item(menu, ID_TRAY_RESORT, "바탕화면 자동 정리 (폴더/프로그램/문서)", false);
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    menu_item(menu, ID_TRAY_HIDE, "모든 Fence 숨기기", a.fences_hidden);
    menu_item(menu, ID_TRAY_ICONS, "원래 바탕화면 아이콘 숨기기", a.icons_hidden);
    menu_item(menu, ID_TRAY_AUTORUN, "Windows 시작 시 실행", autorun_enabled());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    menu_item(menu, ID_TRAY_EXIT, "종료", false);
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    match track_menu(menu, pt.x, pt.y) {
        ID_TRAY_NEW => {
            let idx = new_fence("새 Fence", pt.x.saturating_sub(m(330)).max(0), pt.y.saturating_sub(m(320)).max(0), m(330), m(300), COLORS[0].1);
            invalidate(idx);
            save();
        }
        ID_TRAY_RESORT => auto_sort(),
        ID_TRAY_HIDE => toggle_fences(),
        ID_TRAY_ICONS => {
            a.icons_hidden = !a.icons_hidden;
            apply_icons_hidden();
            save();
        }
        ID_TRAY_AUTORUN => set_autorun(!autorun_enabled()),
        ID_TRAY_EXIT => {
            DestroyWindow(a.main);
        }
        _ => {}
    }
}

unsafe fn toggle_fences() {
    let a = app();
    a.fences_hidden = !a.fences_hidden;
    for f in &a.fences {
        ShowWindow(f.hwnd, if a.fences_hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
    }
}

// ───────────────────────── 시작 프로그램 등록 ─────────────────────────
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "IconDocs";

unsafe fn autorun_enabled() -> bool {
    let mut hk: HKEY = null_mut();
    if RegOpenKeyExW(HKEY_CURRENT_USER, wide(RUN_KEY).as_ptr(), 0, KEY_QUERY_VALUE, &mut hk) != 0 {
        return false;
    }
    let r = RegQueryValueExW(hk, wide(RUN_VALUE).as_ptr(), null(), null_mut(), null_mut(), null_mut());
    RegCloseKey(hk);
    r == 0
}

unsafe fn set_autorun(on: bool) {
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

// ───────────────────────── 이름 바꾸기 팝업 ─────────────────────────
unsafe fn open_rename(idx: usize) {
    let a = app();
    let f = &a.fences[idx];
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(f.hwnd, &mut r);
    let popup = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        wide(CLASS_RENAME).as_ptr(),
        null(),
        WS_POPUP | WS_BORDER,
        r.left,
        r.top,
        r.right - r.left,
        m(TITLE_H),
        a.main,
        null_mut(),
        a.hinst,
        null(),
    );
    SetWindowLongPtrW(popup, GWLP_USERDATA, f.hwnd as isize);
    let edit = CreateWindowExW(
        0,
        wide("EDIT").as_ptr(),
        wide(&f.title).as_ptr(),
        WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
        2,
        2,
        r.right - r.left - 6,
        m(TITLE_H) - 6,
        popup,
        null_mut(),
        a.hinst,
        null(),
    );
    SendMessageW(edit, WM_SETFONT, a.font_b as usize, 1);
    EDIT_OLD_PROC = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT as usize as isize);
    ShowWindow(popup, SW_SHOW);
    SetForegroundWindow(popup);
    SetFocus(edit);
    SendMessageW(edit, EM_SETSEL, 0, -1);
}

unsafe fn commit_rename(edit: HWND, apply: bool) {
    let popup = GetParent(edit);
    if popup.is_null() {
        return;
    }
    let target = GetWindowLongPtrW(popup, GWLP_USERDATA) as HWND;
    if apply {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32);
        let title = from_wide(&buf[..n as usize]).trim().to_string();
        if !title.is_empty() {
            if let Some(idx) = fence_idx(target) {
                app().fences[idx].title = title;
                invalidate(idx);
                save();
            }
        }
    }
    SetWindowLongPtrW(popup, GWLP_USERDATA, 0);
    DestroyWindow(popup);
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN if wp as u16 == VK_RETURN => {
            commit_rename(hwnd, true);
            return 0;
        }
        WM_KEYDOWN if wp as u16 == VK_ESCAPE => {
            commit_rename(hwnd, false);
            return 0;
        }
        WM_KILLFOCUS => {
            commit_rename(hwnd, true);
            return 0;
        }
        WM_CHAR if wp as u16 == VK_RETURN || wp as u16 == VK_ESCAPE => return 0,
        _ => {}
    }
    let old: WNDPROC = core::mem::transmute(EDIT_OLD_PROC);
    CallWindowProcW(old, hwnd, msg, wp, lp)
}

unsafe extern "system" fn rename_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wp, lp)
}

// ───────────────────────── Fence 창 프로시저 ─────────────────────────
unsafe extern "system" fn fence_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let idx = match fence_idx(hwnd) {
        Some(i) => i,
        None => return DefWindowProcW(hwnd, msg, wp, lp),
    };
    let (mx, my) = (lp as i32 as i16 as i32, (lp >> 16) as i32 as i16 as i32);
    match msg {
        WM_NCCALCSIZE => return 0,
        WM_ERASEBKGND => return 1,
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_PAINT => {
            paint(hwnd, idx);
            return 0;
        }
        WM_NCHITTEST => {
            let mut pt = POINT { x: mx, y: my };
            ScreenToClient(hwnd, &mut pt);
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetClientRect(hwnd, &mut rc);
            let b = m(BORDER);
            let rolled = app().fences[idx].rolled;
            let l = pt.x < b;
            let r = pt.x >= rc.right - b;
            let t = !rolled && pt.y < b;
            let bt = !rolled && pt.y >= rc.bottom - b;
            let ht = match (l, r, t, bt) {
                (true, _, true, _) => HTTOPLEFT,
                (_, true, true, _) => HTTOPRIGHT,
                (true, _, _, true) => HTBOTTOMLEFT,
                (_, true, _, true) => HTBOTTOMRIGHT,
                (true, _, _, _) => HTLEFT,
                (_, true, _, _) => HTRIGHT,
                (_, _, true, _) => HTTOP,
                (_, _, _, true) => HTBOTTOM,
                _ if pt.y < m(TITLE_H) => HTCAPTION,
                _ => HTCLIENT,
            };
            return ht as LRESULT;
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lp as *mut MINMAXINFO);
            mmi.ptMinTrackSize.x = m(MIN_W);
            mmi.ptMinTrackSize.y = if app().fences[idx].rolled { m(TITLE_H) } else { m(TITLE_H + CELL_H / 2) };
            return 0;
        }
        WM_NCLBUTTONDBLCLK if wp == HTCAPTION as usize => {
            set_rolled(idx, !app().fences[idx].rolled);
            return 0;
        }
        WM_NCRBUTTONUP if wp == HTCAPTION as usize => {
            fence_menu(idx, mx, my);
            return 0;
        }
        WM_SIZE | WM_MOVE => {
            let f = &mut app().fences[idx];
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut r);
            let parent = GetParent(hwnd);
            if !parent.is_null() {
                let mut p = POINT { x: r.left, y: r.top };
                ScreenToClient(parent, &mut p);
                f.x = p.x;
                f.y = p.y;
            } else {
                f.x = r.left;
                f.y = r.top;
            }
            f.w = r.right - r.left;
            if !f.rolled {
                f.h = r.bottom - r.top;
            }
            if msg == WM_SIZE {
                apply_region(hwnd);
                f.clamp_scroll();
                InvalidateRect(hwnd, null(), 1);
            }
            return 0;
        }
        WM_EXITSIZEMOVE => {
            save();
            return 0;
        }
        WM_MOUSEWHEEL => {
            let delta = (wp >> 16) as u16 as i16 as i32;
            let f = &mut app().fences[idx];
            f.scroll -= delta / 120 * m(40);
            f.clamp_scroll();
            InvalidateRect(hwnd, null(), 1);
            return 0;
        }
        WM_LBUTTONDOWN => {
            let f = &mut app().fences[idx];
            let hit = f.hit_item(mx, my);
            f.sel = hit;
            if let Some(i) = hit {
                f.drag = Some((i, POINT { x: mx, y: my }));
                f.dragging = false;
                SetCapture(hwnd);
            }
            InvalidateRect(hwnd, null(), 1);
            return 0;
        }
        WM_MOUSEMOVE => {
            let f = &mut app().fences[idx];
            if let Some((_, p)) = f.drag {
                if !f.dragging && ((mx - p.x).abs() > 4 || (my - p.y).abs() > 4) {
                    f.dragging = true;
                    SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
                }
            }
            return 0;
        }
        WM_SETCURSOR => {
            if app().fences[idx].dragging {
                SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
                return 1;
            }
        }
        WM_LBUTTONUP => {
            let f = &mut app().fences[idx];
            let Some((i, _)) = f.drag.take() else { return 0 };
            let was_dragging = f.dragging;
            f.dragging = false;
            ReleaseCapture();
            if !was_dragging || i >= f.items.len() {
                return 0;
            }
            let mut spt = POINT { x: mx, y: my };
            ClientToScreen(hwnd, &mut spt);
            let target = WindowFromPoint(spt);
            let Some(tidx) = fence_idx(target) else { return 0 };
            let mut tpt = spt;
            ScreenToClient(target, &mut tpt);
            if tidx == idx {
                let f = &mut app().fences[idx];
                let to = f.hit_item(tpt.x, tpt.y).unwrap_or(f.items.len() - 1);
                let it = f.items.remove(i);
                f.items.insert(to.min(f.items.len()), it);
                f.sel = Some(to.min(f.items.len() - 1));
                invalidate(idx);
            } else {
                let it = app().fences[idx].items.remove(i);
                app().fences[idx].sel = None;
                invalidate(idx);
                let tf = &mut app().fences[tidx];
                let to = tf.hit_item(tpt.x, tpt.y).unwrap_or(tf.items.len());
                if !tf.has(&it.path) {
                    tf.items.insert(to.min(tf.items.len()), it);
                }
                invalidate(tidx);
            }
            save();
            return 0;
        }
        WM_CAPTURECHANGED => {
            let f = &mut app().fences[idx];
            f.drag = None;
            f.dragging = false;
            return 0;
        }
        WM_LBUTTONDBLCLK => {
            if let Some(i) = app().fences[idx].hit_item(mx, my) {
                let p = app().fences[idx].items[i].path.clone();
                open_path(&p);
            }
            return 0;
        }
        WM_RBUTTONUP => {
            let mut spt = POINT { x: mx, y: my };
            ClientToScreen(hwnd, &mut spt);
            match app().fences[idx].hit_item(mx, my) {
                Some(i) => {
                    app().fences[idx].sel = Some(i);
                    InvalidateRect(hwnd, null(), 1);
                    item_menu(idx, i, spt.x, spt.y);
                }
                None => fence_menu(idx, spt.x, spt.y),
            }
            return 0;
        }
        WM_DROPFILES => {
            let hdrop = wp as HDROP;
            let n = DragQueryFileW(hdrop, 0xFFFF_FFFF, null_mut(), 0);
            for i in 0..n {
                let mut buf = [0u16; 1024];
                let len = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), buf.len() as u32);
                if len > 0 {
                    let p = from_wide(&buf[..len as usize]);
                    add_item_to(idx, &p);
                }
            }
            DragFinish(hdrop);
            save();
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

// ───────────────────────── 메인(숨김) 창 프로시저 ─────────────────────────
unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let ev = lp as u32;
            if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                tray_menu();
            } else if ev == WM_LBUTTONDBLCLK {
                toggle_fences();
            }
            return 0;
        }
        WM_TIMER if wp == TIMER_RESCAN => {
            rescan();
            apply_icons_hidden();
            return 0;
        }
        WM_DESKTOP_DBLCLK => {
            let pt = POINT { x: wp as i32, y: lp as i32 };
            let target = WindowFromPoint(pt);
            if fence_idx(target).is_some() {
                return 0;
            }
            let root = GetAncestor(target, GA_ROOT);
            let mut buf = [0u16; 64];
            let n = GetClassNameW(root, buf.as_mut_ptr(), buf.len() as i32);
            let cls = from_wide(&buf[..n as usize]);
            if cls == "Progman" || cls == "WorkerW" {
                toggle_fences();
            }
            return 0;
        }
        WM_BAND_DOWN => {
            let pt = POINT { x: wp as i32, y: lp as i32 };
            if is_empty_desktop(pt) {
                app().band_start = Some(pt);
            }
            return 0;
        }
        WM_BAND_MOVE => {
            let a = app();
            let Some(s) = a.band_start else { return 0 };
            let pt = POINT { x: wp as i32, y: lp as i32 };
            if a.band_hwnd.is_null() {
                if (pt.x - s.x).abs() < 12 && (pt.y - s.y).abs() < 12 {
                    return 0;
                }
                a.band_hwnd = CreateWindowExW(
                    WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT,
                    wide(CLASS_BAND).as_ptr(),
                    null(),
                    WS_POPUP,
                    s.x, s.y, 1, 1,
                    null_mut(), null_mut(), a.hinst, null(),
                );
                SetLayeredWindowAttributes(a.band_hwnd, 0, 110, LWA_ALPHA);
                ShowWindow(a.band_hwnd, SW_SHOWNOACTIVATE);
            }
            let (x, y, w, h) = norm_rect(s, pt);
            SetWindowPos(a.band_hwnd, HWND_TOPMOST, x, y, w.max(1), h.max(1), SWP_NOACTIVATE);
            return 0;
        }
        WM_BAND_UP => {
            let a = app();
            let Some(s) = a.band_start.take() else { return 0 };
            let had_band = !a.band_hwnd.is_null();
            if had_band {
                DestroyWindow(a.band_hwnd);
                a.band_hwnd = null_mut();
            }
            let pt = POINT { x: wp as i32, y: lp as i32 };
            let (x, y, w, h) = norm_rect(s, pt);
            if had_band && w >= m(MIN_W) && h >= m(TITLE_H + CELL_H / 2) {
                // DefView 클라이언트 좌표로 변환
                let mut p = POINT { x, y };
                if !a.defview.is_null() && IsWindow(a.defview) != 0 {
                    ScreenToClient(a.defview, &mut p);
                }
                let idx = new_fence("새 Fence", p.x, p.y, w, h, COLORS[0].1);
                invalidate(idx);
                save();
            }
            return 0;
        }
        WM_DESTROY => {
            let a = app();
            let mut nid: NOTIFYICONDATAW = core::mem::zeroed();
            nid.cbSize = core::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &nid);
            a.icons_hidden = false;
            apply_icons_hidden();
            for f in &a.fences {
                if !f.hwnd.is_null() {
                    DestroyWindow(f.hwnd);
                }
            }
            if !a.hook.is_null() {
                UnhookWindowsHookEx(a.hook);
            }
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

fn norm_rect(a: POINT, b: POINT) -> (i32, i32, i32, i32) {
    (a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
}

// 화면 좌표가 Fence 가 아닌 바탕화면(Progman/WorkerW) 위인지
unsafe fn is_empty_desktop(pt: POINT) -> bool {
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

// 드래그 미리보기(반투명 사각형) 창
unsafe extern "system" fn band_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let mut ps: PAINTSTRUCT = core::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(hwnd, &mut rc);
        let br = CreateSolidBrush(lighten(COLORS[0].1, 120));
        FillRect(hdc, &rc, br);
        DeleteObject(br as HGDIOBJ);
        let fr = CreateSolidBrush(0xFFFFFF);
        FrameRect(hdc, &rc, fr);
        DeleteObject(fr as HGDIOBJ);
        EndPaint(hwnd, &ps);
        return 0;
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

// 바탕화면 빈 공간 더블클릭 감지용 저수준 마우스 훅
unsafe extern "system" fn mouse_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && wp as u32 == WM_LBUTTONDOWN {
        let ms = &*(lp as *const MSLLHOOKSTRUCT);
        let a = app();
        let (t0, x0, y0) = a.last_click;
        if t0 != 0
            && ms.time.wrapping_sub(t0) <= GetDoubleClickTime()
            && (ms.pt.x - x0).abs() <= 4
            && (ms.pt.y - y0).abs() <= 4
        {
            a.last_click = (0, 0, 0);
            PostMessageW(a.main, WM_DESKTOP_DBLCLK, ms.pt.x as usize, ms.pt.y as isize);
        } else {
            a.last_click = (ms.time, ms.pt.x, ms.pt.y);
        }
        PostMessageW(a.main, WM_BAND_DOWN, ms.pt.x as usize, ms.pt.y as isize);
    } else if code >= 0 && wp as u32 == WM_MOUSEMOVE {
        let a = app();
        if a.band_start.is_some() {
            let ms = &*(lp as *const MSLLHOOKSTRUCT);
            PostMessageW(a.main, WM_BAND_MOVE, ms.pt.x as usize, ms.pt.y as isize);
        }
    } else if code >= 0 && wp as u32 == WM_LBUTTONUP {
        let a = app();
        if a.band_start.is_some() {
            let ms = &*(lp as *const MSLLHOOKSTRUCT);
            PostMessageW(a.main, WM_BAND_UP, ms.pt.x as usize, ms.pt.y as isize);
        }
    }
    CallNextHookEx(null_mut(), code, wp, lp)
}

// ───────────────────────── 진입점 ─────────────────────────
unsafe fn register(class: &str, proc: WNDPROC, style: u32, hinst: HINSTANCE) {
    let name = wide(class);
    let wc = WNDCLASSW {
        style,
        lpfnWndProc: proc,
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: null_mut(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
    };
    RegisterClassW(&wc);
}

fn main() {
    unsafe {
        // 중복 실행 방지
        if !FindWindowW(wide(CLASS_MAIN).as_ptr(), null()).is_null() {
            return;
        }
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hinst = GetModuleHandleW(null());

        let mut dirs = Vec::new();
        if let Some(p) = known_folder(&FOLDERID_Desktop) {
            dirs.push(p);
        }
        if let Some(p) = known_folder(&FOLDERID_PublicDesktop) {
            dirs.push(p);
        }

        let (defview, listview) = find_desktop();
        let dpi = if defview.is_null() { 96 } else { GetDpiForWindow(defview) as i32 }.max(96);
        let mkfont = |size: i32, weight: i32| {
            CreateFontW(
                -(size * dpi / 96),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                wide("Malgun Gothic").as_ptr(),
            )
        };
        let app_box = Box::new(App {
            hinst,
            main: null_mut(),
            defview,
            listview,
            fences: Vec::new(),
            fences_hidden: false,
            icons_hidden: false,
            font: mkfont(11, FW_NORMAL as i32),
            font_b: mkfont(12, FW_SEMIBOLD as i32),
            hook: null_mut(),
            desktop_dirs: dirs,
            last_click: (0, 0, 0),
            band_start: None,
            band_hwnd: null_mut(),
            dpi,
            icon_px: GetSystemMetrics(SM_CXICON).max(32),
        });
        APP = Box::into_raw(app_box);

        register(CLASS_MAIN, Some(main_proc), 0, hinst);
        register(CLASS_FENCE, Some(fence_proc), CS_DBLCLKS | CS_HREDRAW | CS_VREDRAW, hinst);
        register(CLASS_RENAME, Some(rename_proc), 0, hinst);
        register(CLASS_BAND, Some(band_proc), CS_HREDRAW | CS_VREDRAW, hinst);

        let main = CreateWindowExW(
            0,
            wide(CLASS_MAIN).as_ptr(),
            wide("IconDocs").as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        app().main = main;

        // 트레이 아이콘
        let mut nid: NOTIFYICONDATAW = core::mem::zeroed();
        nid.cbSize = core::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = main;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = LoadIconW(null_mut(), IDI_APPLICATION);
        let tip = wide("아이콘독스 — 우클릭: 메뉴 / 더블클릭: Fence 숨기기·보이기");
        nid.szTip[..tip.len().min(128)].copy_from_slice(&tip[..tip.len().min(128)]);
        Shell_NotifyIconW(NIM_ADD, &nid);

        // 레이아웃 불러오기 또는 최초 자동 정리
        if load() {
            for i in 0..app().fences.len() {
                create_fence_window(i);
                invalidate(i);
            }
        } else {
            app().icons_hidden = true;
            auto_sort();
        }
        apply_icons_hidden();
        rescan();

        app().hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), hinst, 0);
        SetTimer(main, TIMER_RESCAN, 3000, None);

        let mut msg: MSG = core::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        for f in &app().fences {
            let _ = f;
        }
        DeleteObject(app().font as HGDIOBJ);
        DeleteObject(app().font_b as HGDIOBJ);
    }
}
