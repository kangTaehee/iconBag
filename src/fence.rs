// Fence: 데이터, 창 생성, 그리기, 메뉴, 입력 팝업, 창 프로시저
#![allow(unused_imports, dead_code)]
use crate::*;
use core::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ───────────────────────── 데이터 ─────────────────────────
pub struct Item {
    pub path: String,
    pub name: String,
    pub icon: HICON,
}
impl Item {
    pub fn new(path: &str) -> Item {
        Item { path: path.to_string(), name: display_name(Path::new(path)), icon: null_mut() }
    }
    pub fn clear_icon(&mut self) {
        if !self.icon.is_null() {
            unsafe { DestroyIcon(self.icon) };
            self.icon = null_mut();
        }
    }
}
impl Drop for Item {
    fn drop(&mut self) {
        self.clear_icon();
    }
}

pub struct Fence {
    pub hwnd: HWND,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub color: u32,
    pub rolled: bool,
    pub items: Vec<Item>,
    pub scroll: i32,
    pub sel: Vec<usize>,
    pub anchor: Option<usize>,
    pub drag: Option<(usize, POINT)>,
    pub dragging: bool,
    pub icon_size: i32, // 16 | 32 | 48
    pub alpha: u8,
    pub title_hidden: bool,
    pub locked: bool,
    pub portal: Option<PathBuf>,
    pub nav: Vec<PathBuf>,
    pub hover_expanded: bool,
}

impl Fence {
    pub fn new(title: &str, x: i32, y: i32, w: i32, h: i32, color: u32) -> Fence {
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
            sel: Vec::new(),
            anchor: None,
            drag: None,
            dragging: false,
            icon_size: 32,
            alpha: ALPHA,
            title_hidden: false,
            locked: false,
            portal: None,
            nav: Vec::new(),
            hover_expanded: false,
        }
    }
    // 제목 표시줄 높이 (숨김이면 얇은 그립)
    pub fn th(&self) -> i32 {
        if self.title_hidden {
            m(GRIP_H)
        } else {
            m(TITLE_H)
        }
    }
    // 아이콘 픽셀 크기 (셸 아이콘 크기는 시스템 DPI 를 따름)
    pub fn icon_px(&self) -> i32 {
        unsafe {
            match self.icon_size {
                16 => GetSystemMetrics(SM_CXSMICON).max(16),
                48 => m(48),
                _ => GetSystemMetrics(SM_CXICON).max(32),
            }
        }
    }
    pub fn cell(&self) -> (i32, i32) {
        match self.icon_size {
            16 => (m(76), m(60)),
            48 => (m(104), m(106)),
            _ => (m(CELL_W), m(CELL_H)),
        }
    }
    pub fn cols(&self) -> usize {
        let (cw, _) = self.cell();
        (((self.w - 2 * m(PAD)) / cw).max(1)) as usize
    }
    pub fn content_h(&self) -> i32 {
        let (_, ch) = self.cell();
        let rows = self.items.len().div_ceil(self.cols()) as i32;
        self.th() + m(PAD) + rows * ch + m(PAD)
    }
    pub fn clamp_scroll(&mut self) {
        let max = (self.content_h() - self.h).max(0);
        self.scroll = self.scroll.clamp(0, max);
    }
    pub fn cell_rect(&self, i: usize) -> RECT {
        let cols = self.cols();
        let (cw, ch) = self.cell();
        let (col, row) = ((i % cols) as i32, (i / cols) as i32);
        let cx = m(PAD) + col * cw;
        let cy = self.th() + m(PAD) + row * ch - self.scroll;
        RECT { left: cx, top: cy, right: cx + cw, bottom: cy + ch }
    }
    pub fn hit_item(&self, x: i32, y: i32) -> Option<usize> {
        if y < self.th() {
            return None;
        }
        (0..self.items.len()).find(|&i| {
            let r = self.cell_rect(i);
            x >= r.left && x < r.right && y >= r.top && y < r.bottom
        })
    }
    pub fn has(&self, path: &str) -> bool {
        let l = lower(path);
        self.items.iter().any(|i| lower(&i.path) == l)
    }
    pub fn ensure_visible(&mut self, i: usize) {
        let r = self.cell_rect(i);
        if r.top < self.th() {
            self.scroll -= self.th() - r.top;
        } else if r.bottom > self.h {
            self.scroll += r.bottom - self.h;
        }
        self.clamp_scroll();
    }
    pub fn select_only(&mut self, i: usize) {
        self.sel = vec![i];
        self.anchor = Some(i);
    }
    pub fn is_portal(&self) -> bool {
        self.portal.is_some()
    }
}

pub fn fence_idx(hwnd: HWND) -> Option<usize> {
    app().fences.iter().position(|f| f.hwnd == hwnd)
}

// 현재 표시해야 할 창 높이
pub fn fence_height(f: &Fence) -> i32 {
    if f.rolled && !f.hover_expanded {
        f.th()
    } else {
        f.h
    }
}

// ───────────────────────── 아이콘 로드 ─────────────────────────
#[repr(C)]
struct IImageListVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    add: usize,
    replace_icon: usize,
    set_overlay_image: usize,
    replace: usize,
    add_masked: usize,
    draw: usize,
    remove: usize,
    get_icon: unsafe extern "system" fn(*mut c_void, i32, u32, *mut HICON) -> HRESULT,
}
const IID_IIMAGELIST: GUID = GUID {
    data1: 0x46EB5926,
    data2: 0x582E,
    data3: 0x4017,
    data4: [0x9F, 0xDF, 0xE8, 0x99, 0x8D, 0xAA, 0x09, 0x50],
};
const SHIL_EXTRALARGE: i32 = 2;
static mut XL_LIST: *mut c_void = null_mut();

pub unsafe fn load_icon(path: &str, size: i32) -> HICON {
    let mut sfi: SHFILEINFOW = core::mem::zeroed();
    let p = wide(path);
    let cb = core::mem::size_of::<SHFILEINFOW>() as u32;
    match size {
        16 => {
            SHGetFileInfoW(p.as_ptr(), 0, &mut sfi, cb, SHGFI_ICON | SHGFI_SMALLICON);
            sfi.hIcon
        }
        48 => {
            // 시스템 이미지 목록(SHIL_EXTRALARGE)에서 48px 아이콘을 꺼낸다
            if SHGetFileInfoW(p.as_ptr(), 0, &mut sfi, cb, SHGFI_SYSICONINDEX) == 0 {
                return null_mut();
            }
            if XL_LIST.is_null() && SHGetImageList(SHIL_EXTRALARGE, &IID_IIMAGELIST, &mut XL_LIST) < 0 {
                XL_LIST = null_mut();
            }
            if XL_LIST.is_null() {
                SHGetFileInfoW(p.as_ptr(), 0, &mut sfi, cb, SHGFI_ICON | SHGFI_LARGEICON);
                return sfi.hIcon;
            }
            let vt = &**(XL_LIST as *mut *mut IImageListVtbl);
            let mut h: HICON = null_mut();
            (vt.get_icon)(XL_LIST, sfi.iIcon, 1 /* ILD_TRANSPARENT */, &mut h);
            h
        }
        _ => {
            SHGetFileInfoW(p.as_ptr(), 0, &mut sfi, cb, SHGFI_ICON | SHGFI_LARGEICON);
            sfi.hIcon
        }
    }
}

// ───────────────────────── 창 생성/관리 ─────────────────────────
pub unsafe fn create_fence_window(idx: usize) {
    let a = app();
    let (title, x, y, w, h, alpha, locked) = {
        let f = &a.fences[idx];
        (f.title.clone(), f.x, f.y, f.w, fence_height(f), f.alpha, f.locked)
    };
    // 다른 프로세스(explorer)의 창을 부모로 직접 지정해 만들면 실패하므로
    // 먼저 최상위 팝업으로 만든 뒤 SetParent 로 바탕화면 레이어에 붙인다.
    let exstyle = WS_EX_LAYERED | WS_EX_ACCEPTFILES | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    let style = WS_POPUP | WS_CLIPSIBLINGS | if locked { 0 } else { WS_THICKFRAME };
    let hwnd = CreateWindowExW(
        exstyle,
        wide(CLASS_FENCE).as_ptr(),
        wide(&title).as_ptr(),
        style,
        x,
        y,
        w,
        h,
        null_mut(),
        null_mut(),
        a.hinst,
        null(),
    );
    a.fences[idx].hwnd = hwnd;
    let attached = attach_to_desktop(hwnd, x, y, w, h);
    SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
    DragAcceptFiles(hwnd, 1);
    apply_region(hwnd);
    if !a.fences_hidden {
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    if !attached {
        SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

pub unsafe fn apply_region(hwnd: HWND) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(hwnd, &mut r);
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let rad = m(12);
    SetWindowRgn(hwnd, CreateRoundRectRgn(0, 0, w + 1, h + 1, rad, rad), 1);
}

pub unsafe fn new_fence(title: &str, x: i32, y: i32, w: i32, h: i32, color: u32) -> usize {
    app().fences.push(Fence::new(title, x, y, w, h, color));
    let idx = app().fences.len() - 1;
    create_fence_window(idx);
    idx
}

pub unsafe fn delete_fence(idx: usize) {
    let a = app();
    let f = a.fences.remove(idx);
    if !f.hwnd.is_null() {
        DestroyWindow(f.hwnd);
    }
    if a.focused == Some(idx) {
        a.focused = None;
    } else if let Some(fi) = a.focused {
        if fi > idx {
            a.focused = Some(fi - 1);
        }
    }
    save();
}

// 롤업/펼침 상태에 맞춰 창 높이 적용
pub unsafe fn apply_height(idx: usize) {
    let f = &mut app().fences[idx];
    let h = fence_height(f);
    SetWindowPos(f.hwnd, null_mut(), 0, 0, f.w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    apply_region(f.hwnd);
    InvalidateRect(f.hwnd, null(), 1);
}

pub unsafe fn set_rolled(idx: usize, rolled: bool) {
    let f = &mut app().fences[idx];
    f.rolled = rolled;
    f.hover_expanded = false;
    KillTimer(f.hwnd, TIMER_HOVER);
    apply_height(idx);
    save();
}

pub unsafe fn set_locked(idx: usize, locked: bool) {
    let f = &mut app().fences[idx];
    f.locked = locked;
    let st = GetWindowLongPtrW(f.hwnd, GWL_STYLE) as u32;
    let st = if locked { st & !WS_THICKFRAME } else { st | WS_THICKFRAME };
    SetWindowLongPtrW(f.hwnd, GWL_STYLE, st as isize);
    SetWindowPos(f.hwnd, null_mut(), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    apply_region(f.hwnd);
    invalidate(idx);
    save();
}

pub unsafe fn set_icon_size(idx: usize, size: i32) {
    let f = &mut app().fences[idx];
    f.icon_size = size;
    for it in f.items.iter_mut() {
        it.clear_icon();
    }
    f.scroll = 0;
    invalidate(idx);
    save();
}

pub unsafe fn set_alpha(idx: usize, alpha: u8) {
    let f = &mut app().fences[idx];
    f.alpha = alpha;
    SetLayeredWindowAttributes(f.hwnd, 0, alpha, LWA_ALPHA);
    save();
}

pub unsafe fn set_title_hidden(idx: usize, hidden: bool) {
    let f = &mut app().fences[idx];
    f.title_hidden = hidden;
    apply_height(idx);
    save();
}

pub unsafe fn invalidate(idx: usize) {
    let f = &mut app().fences[idx];
    f.clamp_scroll();
    InvalidateRect(f.hwnd, null(), 1);
}

// 다른 Fence 에 들어 있는 같은 경로를 제거한 뒤 대상 Fence 에 추가
pub unsafe fn add_item_to(idx: usize, path: &str) {
    let a = app();
    let l = lower(path);
    for (i, f) in a.fences.iter_mut().enumerate() {
        if i != idx && f.portal.is_none() {
            let before = f.items.len();
            f.items.retain(|it| lower(&it.path) != l);
            if f.items.len() != before {
                f.sel.clear();
                InvalidateRect(f.hwnd, null(), 1);
            }
        }
    }
    if !a.fences[idx].has(path) {
        a.fences[idx].items.push(Item::new(path));
    }
    invalidate(idx);
}

pub unsafe fn open_path(path: &str) {
    ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(path).as_ptr(), null(), null(), SW_SHOWNORMAL);
}

// 항목 실행: 포털의 폴더는 안으로 들어가고, 그 외는 셸로 연다. 피크 중이면 피크 종료.
pub unsafe fn open_item(idx: usize, i: usize) {
    let f = &app().fences[idx];
    if i >= f.items.len() {
        return;
    }
    let p = PathBuf::from(&f.items[i].path);
    if f.is_portal() && p.is_dir() {
        portal_enter(idx, p);
        return;
    }
    open_path(&p.to_string_lossy());
    if app().peeking {
        peek_end();
    }
}

// ───────────────────────── 그리기 ─────────────────────────
pub unsafe fn paint(hwnd: HWND, idx: usize) {
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
    let th = f.th();

    // 배경
    let br = CreateSolidBrush(f.color);
    FillRect(mem, &rc, br);
    DeleteObject(br as HGDIOBJ);
    // 제목 표시줄 (또는 그립)
    let trc = RECT { left: 0, top: 0, right: w, bottom: th };
    let br = CreateSolidBrush(darken(f.color, 70));
    FillRect(mem, &trc, br);
    DeleteObject(br as HGDIOBJ);

    SetBkMode(mem, TRANSPARENT as i32);
    SetTextColor(mem, 0xF0F0F0);
    let old_font = SelectObject(mem, a.font_b as HGDIOBJ);
    if !f.title_hidden {
        let mut title = String::new();
        if f.is_portal() && !f.nav.is_empty() {
            let sub = f.nav.last().and_then(|p| p.file_name()).and_then(|s| s.to_str()).unwrap_or("");
            title.push_str(&format!("‹  {} › {}", f.title, sub));
        } else {
            title.push_str(&f.title);
        }
        title.push_str(&format!("  ({})", f.items.len()));
        if f.locked {
            title.push_str("  🔒");
        }
        let mut tr = RECT { left: m(PAD) + 4, top: 0, right: w - m(PAD), bottom: th };
        let tw = wide(&title);
        DrawTextW(mem, tw.as_ptr(), -1, &mut tr, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX);
    }
    SelectObject(mem, a.font as HGDIOBJ);

    if !(f.rolled && !f.hover_expanded) {
        let old_pen = SelectObject(mem, GetStockObject(NULL_PEN));
        let f = &mut a.fences[idx];
        let icon_px = f.icon_px();
        let (cw, _) = f.cell();
        let size = f.icon_size;
        for i in 0..f.items.len() {
            let r = f.cell_rect(i);
            if r.bottom < th || r.top > h {
                continue;
            }
            if f.sel.contains(&i) {
                let sb = CreateSolidBrush(lighten(f.color, 45));
                let old = SelectObject(mem, sb as HGDIOBJ);
                RoundRect(mem, r.left, r.top, r.right, r.bottom, m(8), m(8));
                SelectObject(mem, old);
                DeleteObject(sb as HGDIOBJ);
            }
            let it = &mut f.items[i];
            if it.icon.is_null() {
                it.icon = load_icon(&it.path, size);
            }
            let ix = r.left + (cw - icon_px) / 2;
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
pub unsafe fn track_menu(menu: HMENU, x: i32, y: i32) -> usize {
    let a = app();
    SetForegroundWindow(a.main);
    let r = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, x, y, 0, a.main, null());
    PostMessageW(a.main, WM_NULL, 0, 0);
    DestroyMenu(menu);
    r as usize
}

pub unsafe fn menu_item(menu: HMENU, id: usize, text: &str, checked: bool) {
    let fl = MF_STRING | if checked { MF_CHECKED } else { 0 };
    AppendMenuW(menu, fl, id, wide(text).as_ptr());
}
pub unsafe fn menu_sep(menu: HMENU) {
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
}
pub unsafe fn menu_sub(menu: HMENU, sub: HMENU, text: &str) {
    AppendMenuW(menu, MF_POPUP, sub as usize, wide(text).as_ptr());
}

pub unsafe fn fence_menu(idx: usize, x: i32, y: i32) {
    let a = app();
    let menu = CreatePopupMenu();
    let (is_portal, nav_depth, rolled, locked, title_hidden, color, icon_size, alpha) = {
        let f = &a.fences[idx];
        (f.is_portal(), f.nav.len(), f.rolled, f.locked, f.title_hidden, f.color, f.icon_size, f.alpha)
    };
    menu_item(menu, ID_F_RENAME, "이름 바꾸기\tF2", false);
    menu_item(menu, ID_F_ROLL, "말아 올리기 (제목만 표시)", rolled);
    menu_item(menu, ID_F_SORT, "이름순 정렬", false);
    if is_portal && nav_depth > 0 {
        menu_item(menu, ID_F_BACK, "상위 폴더로\tBackspace", false);
    }
    menu_sep(menu);

    // 보기 옵션
    let sub = CreatePopupMenu();
    menu_item(sub, ID_F_ICON16, "작게 (16px)", icon_size == 16);
    menu_item(sub, ID_F_ICON32, "보통 (32px)", icon_size == 32);
    menu_item(sub, ID_F_ICON48, "크게 (48px)", icon_size == 48);
    menu_sub(menu, sub, "아이콘 크기");
    let sub = CreatePopupMenu();
    for (i, (name, v)) in ALPHAS.iter().enumerate() {
        menu_item(sub, ID_F_ALPHA0 + i, name, alpha == *v);
    }
    menu_sub(menu, sub, "투명도");
    let sub = CreatePopupMenu();
    for (i, (name, c)) in COLORS.iter().enumerate() {
        menu_item(sub, ID_F_COLOR0 + i, name, color == *c);
    }
    menu_sub(menu, sub, "색상");
    menu_item(menu, ID_F_TITLE, "제목 표시줄 숨기기", title_hidden);
    menu_item(menu, ID_F_LOCK, "잠금 (이동·크기 변경 방지)", locked);
    menu_sep(menu);

    // 자동 분류 규칙 / 포털
    if is_portal {
        menu_item(menu, ID_F_PORTAL_OFF, "폴더 포털 해제", false);
    } else {
        menu_item(menu, ID_F_RULE_ADD, "자동 분류 규칙 추가…", false);
        let mine: Vec<(usize, String)> = a
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| r.target == a.fences[idx].title)
            .map(|(i, r)| (i, r.pattern.clone()))
            .collect();
        if !mine.is_empty() {
            let sub = CreatePopupMenu();
            for (k, (_, pat)) in mine.iter().enumerate().take(ID_F_RULE_MAX) {
                menu_item(sub, ID_F_RULE0 + k, &format!("{}   (클릭하면 삭제)", pat), false);
            }
            menu_sub(menu, sub, &format!("이 Fence 의 규칙 ({})", mine.len()));
        }
        menu_item(menu, ID_F_PORTAL, "폴더 포털로 전환…", false);
        // 규칙 삭제 인덱스 매핑을 위해 보관
        RULE_MENU_MAP = mine.iter().map(|(i, _)| *i).collect();
    }
    menu_sep(menu);
    menu_item(menu, ID_F_DELETE, "Fence 삭제", false);

    match track_menu(menu, x, y) {
        ID_F_RENAME => open_prompt(PromptKind::Rename, idx, None),
        ID_F_ROLL => set_rolled(idx, !rolled),
        ID_F_SORT => {
            let f = &mut app().fences[idx];
            f.items.sort_by_key(|i| (!Path::new(&i.path).is_dir(), lower(&i.name)));
            f.sel.clear();
            invalidate(idx);
            save();
        }
        ID_F_BACK => portal_back(idx),
        ID_F_ICON16 => set_icon_size(idx, 16),
        ID_F_ICON32 => set_icon_size(idx, 32),
        ID_F_ICON48 => set_icon_size(idx, 48),
        ID_F_TITLE => set_title_hidden(idx, !title_hidden),
        ID_F_LOCK => set_locked(idx, !locked),
        ID_F_RULE_ADD => open_prompt(PromptKind::Rule, idx, Some("*.".to_string())),
        ID_F_PORTAL => {
            if let Some(dir) = pick_folder("포털로 표시할 폴더를 선택하세요") {
                set_portal(idx, Some(dir));
            }
        }
        ID_F_PORTAL_OFF => set_portal(idx, None),
        ID_F_DELETE => delete_fence(idx),
        c if (ID_F_COLOR0..ID_F_COLOR0 + COLORS.len()).contains(&c) => {
            app().fences[idx].color = COLORS[c - ID_F_COLOR0].1;
            invalidate(idx);
            save();
        }
        c if (ID_F_ALPHA0..ID_F_ALPHA0 + ALPHAS.len()).contains(&c) => set_alpha(idx, ALPHAS[c - ID_F_ALPHA0].1),
        c if (ID_F_RULE0..ID_F_RULE0 + ID_F_RULE_MAX).contains(&c) => {
            let k = c - ID_F_RULE0;
            if let Some(&ri) = RULE_MENU_MAP.get(k) {
                if ri < app().rules.len() {
                    app().rules.remove(ri);
                    save();
                }
            }
        }
        _ => {}
    }
}
static mut RULE_MENU_MAP: Vec<usize> = Vec::new();

pub unsafe fn item_menu(idx: usize, item: usize, x: i32, y: i32) {
    let menu = CreatePopupMenu();
    let (path, is_portal, title) = {
        let f = &app().fences[idx];
        (f.items[item].path.clone(), f.is_portal(), f.title.clone())
    };
    let pat = suggest_pattern(Path::new(&path));
    menu_item(menu, ID_I_OPEN, "열기\tEnter", false);
    menu_item(menu, ID_I_LOC, "파일 위치 열기", false);
    if !is_portal {
        menu_sep(menu);
        menu_item(menu, ID_I_RULE, &format!("이 종류를 '{}' 로 자동 분류 ({})", title, pat), false);
        menu_item(menu, ID_I_REMOVE, "이 Fence 에서 제거\tDelete", false);
    }
    match track_menu(menu, x, y) {
        ID_I_OPEN => open_item(idx, item),
        ID_I_LOC => {
            let args = wide(&format!("/select,\"{}\"", path));
            ShellExecuteW(null_mut(), wide("open").as_ptr(), wide("explorer.exe").as_ptr(), args.as_ptr(), null(), SW_SHOWNORMAL);
        }
        ID_I_RULE => {
            app().rules.retain(|r| r.pattern != pat);
            app().rules.insert(0, Rule { pattern: pat, target: title });
            save();
            reapply_rules();
        }
        ID_I_REMOVE => {
            let f = &mut app().fences[idx];
            let mut sel = if f.sel.contains(&item) { f.sel.clone() } else { vec![item] };
            sel.sort_unstable_by(|a, b| b.cmp(a));
            for i in sel {
                if i < f.items.len() {
                    f.items.remove(i);
                }
            }
            f.sel.clear();
            invalidate(idx);
            save();
            rescan(); // 규칙 또는 '새 항목' 으로 이동
        }
        _ => {}
    }
}

// ───────────────────────── 텍스트 입력 팝업 ─────────────────────────
#[derive(Clone, Copy, PartialEq)]
pub enum PromptKind {
    Rename,
    Rule,
    Snapshot,
}
static mut PROMPT_KIND: PromptKind = PromptKind::Rename;
static mut EDIT_OLD_PROC: isize = 0;

// idx: 대상 Fence (Snapshot 은 무시). default: 초기 문자열
pub unsafe fn open_prompt(kind: PromptKind, idx: usize, default: Option<String>) {
    let a = app();
    let (x, y, w, text, target) = if kind == PromptKind::Snapshot {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        (pt.x, pt.y, m(260), default.unwrap_or_default(), null_mut())
    } else {
        let f = &a.fences[idx];
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(f.hwnd, &mut r);
        let text = match kind {
            PromptKind::Rename => f.title.clone(),
            _ => default.unwrap_or_default(),
        };
        (r.left, r.top, (r.right - r.left).max(m(200)), text, f.hwnd)
    };
    let hint = match kind {
        PromptKind::Rename => "",
        PromptKind::Rule => "규칙 패턴 (예: *.pdf;*.docx  또는 <dir>)",
        PromptKind::Snapshot => "레이아웃 이름",
    };
    let ph = m(TITLE_H) + if hint.is_empty() { 0 } else { m(18) };
    PROMPT_KIND = kind;
    let popup = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        wide(CLASS_RENAME).as_ptr(),
        wide(hint).as_ptr(),
        WS_POPUP | WS_BORDER,
        x,
        y,
        w,
        ph,
        a.main,
        null_mut(),
        a.hinst,
        null(),
    );
    SetWindowLongPtrW(popup, GWLP_USERDATA, target as isize);
    let ey = if hint.is_empty() { 2 } else { m(18) };
    let edit = CreateWindowExW(
        0,
        wide("EDIT").as_ptr(),
        wide(&text).as_ptr(),
        WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
        2,
        ey,
        w - 6,
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

unsafe fn commit_prompt(edit: HWND, apply: bool) {
    let popup = GetParent(edit);
    if popup.is_null() {
        return;
    }
    let target = GetWindowLongPtrW(popup, GWLP_USERDATA) as HWND;
    SetWindowLongPtrW(popup, GWLP_USERDATA, 0);
    let mut buf = [0u16; 512];
    let n = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32);
    let text = from_wide(&buf[..n as usize]).trim().to_string();
    let kind = PROMPT_KIND;
    DestroyWindow(popup);
    if !apply || text.is_empty() {
        return;
    }
    match kind {
        PromptKind::Rename => {
            if let Some(idx) = fence_idx(target) {
                let old = app().fences[idx].title.clone();
                app().fences[idx].title = text.clone();
                // 규칙의 대상 제목도 함께 바꾼다
                for r in app().rules.iter_mut() {
                    if r.target == old {
                        r.target = text.clone();
                    }
                }
                invalidate(idx);
                save();
            }
        }
        PromptKind::Rule => {
            if let Some(idx) = fence_idx(target) {
                let title = app().fences[idx].title.clone();
                app().rules.insert(0, Rule { pattern: text, target: title });
                save();
                reapply_rules();
            }
        }
        PromptKind::Snapshot => save_snapshot(&text),
    }
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN if wp as u16 == VK_RETURN => {
            commit_prompt(hwnd, true);
            return 0;
        }
        WM_KEYDOWN if wp as u16 == VK_ESCAPE => {
            commit_prompt(hwnd, false);
            return 0;
        }
        WM_KILLFOCUS => {
            commit_prompt(hwnd, true);
            return 0;
        }
        WM_CHAR if wp as u16 == VK_RETURN || wp as u16 == VK_ESCAPE => return 0,
        _ => {}
    }
    let old: WNDPROC = core::mem::transmute(EDIT_OLD_PROC);
    CallWindowProcW(old, hwnd, msg, wp, lp)
}

pub unsafe extern "system" fn rename_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        // 힌트 문구
        let mut ps: PAINTSTRUCT = core::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut buf = [0u16; 128];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n > 0 {
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetClientRect(hwnd, &mut rc);
            let br = CreateSolidBrush(0xF4F4F4);
            FillRect(hdc, &rc, br);
            DeleteObject(br as HGDIOBJ);
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, 0x505050);
            let old = SelectObject(hdc, app().font as HGDIOBJ);
            let mut tr = RECT { left: 4, top: 1, right: rc.right - 4, bottom: m(18) };
            DrawTextW(hdc, buf.as_ptr(), n, &mut tr, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS);
            SelectObject(hdc, old);
        }
        EndPaint(hwnd, &ps);
        return 0;
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

// ───────────────────────── 드래그 앤 드롭(항목 이동) ─────────────────────────
unsafe fn finish_item_drag(idx: usize, i: usize, mx: i32, my: i32) {
    let hwnd = app().fences[idx].hwnd;
    let mut spt = POINT { x: mx, y: my };
    ClientToScreen(hwnd, &mut spt);
    let target = WindowFromPoint(spt);
    let Some(tidx) = fence_idx(target) else { return };
    let mut tpt = spt;
    ScreenToClient(target, &mut tpt);
    // 이동할 항목 집합 (선택에 포함되면 선택 전체)
    let moving: Vec<usize> = {
        let f = &app().fences[idx];
        let mut v = if f.sel.contains(&i) { f.sel.clone() } else { vec![i] };
        v.sort_unstable();
        v.dedup();
        v.retain(|&k| k < f.items.len());
        v
    };
    if moving.is_empty() {
        return;
    }
    if tidx == idx {
        let f = &mut app().fences[idx];
        let hit = f.hit_item(tpt.x, tpt.y).unwrap_or(f.items.len());
        // 꺼낸 뒤의 삽입 위치: hit 앞에 있던 이동 항목 수만큼 당긴다
        let to = hit - moving.iter().filter(|&&k| k < hit).count();
        let mut taken: Vec<Item> = Vec::new();
        for &k in moving.iter().rev() {
            taken.push(f.items.remove(k));
        }
        taken.reverse();
        let to = to.min(f.items.len());
        let n = taken.len();
        for (j, it) in taken.into_iter().enumerate() {
            f.items.insert(to + j, it);
        }
        f.sel = (to..to + n).collect();
        f.anchor = Some(to);
        invalidate(idx);
    } else {
        if app().fences[idx].is_portal() || app().fences[tidx].is_portal() {
            return; // 포털 항목은 파일 시스템 이동이 필요하므로 탐색기 드롭만 지원
        }
        let mut taken: Vec<Item> = Vec::new();
        {
            let f = &mut app().fences[idx];
            for &k in moving.iter().rev() {
                taken.push(f.items.remove(k));
            }
            taken.reverse();
            f.sel.clear();
        }
        invalidate(idx);
        let tf = &mut app().fences[tidx];
        let mut to = tf.hit_item(tpt.x, tpt.y).unwrap_or(tf.items.len()).min(tf.items.len());
        tf.sel.clear();
        for it in taken {
            if !tf.has(&it.path) {
                tf.items.insert(to, it);
                tf.sel.push(to);
                to += 1;
            }
        }
        invalidate(tidx);
    }
    save();
}

// ───────────────────────── Fence 창 프로시저 ─────────────────────────
pub unsafe extern "system" fn fence_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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
            let f = &app().fences[idx];
            if f.locked {
                return HTCLIENT as LRESULT;
            }
            let b = m(BORDER);
            let collapsed = f.rolled && !f.hover_expanded;
            let l = pt.x < b;
            let r = pt.x >= rc.right - b;
            let t = !collapsed && pt.y < b;
            let bt = !collapsed && pt.y >= rc.bottom - b;
            let th = f.th();
            // 포털 뒤로 가기 버튼 영역은 클라이언트로 넘긴다
            let back_btn = f.is_portal() && !f.nav.is_empty() && !f.title_hidden && pt.y < th && pt.x < m(28);
            let ht = match (l, r, t, bt) {
                (true, _, true, _) => HTTOPLEFT,
                (_, true, true, _) => HTTOPRIGHT,
                (true, _, _, true) => HTBOTTOMLEFT,
                (_, true, _, true) => HTBOTTOMRIGHT,
                (true, _, _, _) => HTLEFT,
                (_, true, _, _) => HTRIGHT,
                (_, _, true, _) => HTTOP,
                (_, _, _, true) => HTBOTTOM,
                _ if back_btn => HTCLIENT,
                _ if pt.y < th => HTCAPTION,
                // 제목을 숨긴 경우 빈 배경 드래그로 이동
                _ if f.title_hidden && f.hit_item(pt.x, pt.y).is_none() => HTCAPTION,
                _ => HTCLIENT,
            };
            return ht as LRESULT;
        }
        WM_GETMINMAXINFO => {
            let mmi = &mut *(lp as *mut MINMAXINFO);
            let f = &app().fences[idx];
            mmi.ptMinTrackSize.x = m(MIN_W);
            mmi.ptMinTrackSize.y = if f.rolled && !f.hover_expanded { f.th() } else { f.th() + m(40) };
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
        WM_NCLBUTTONDOWN | WM_NCRBUTTONDOWN => {
            app().focused = Some(idx);
            // 키 입력을 받기 위해 바탕화면을 전경으로
            if !app().peeking {
                SetForegroundWindow(GetAncestor(hwnd, GA_ROOT));
            }
        }
        WM_NCMOUSEMOVE | WM_MOUSEMOVE => {
            // 말아 올린 Fence 에 마우스를 올리면 임시로 펼침
            let a = app();
            let f = &mut a.fences[idx];
            if f.rolled && !f.hover_expanded && a.hover_expand {
                f.hover_expanded = true;
                apply_height(idx);
                SetTimer(hwnd, TIMER_HOVER, 300, None);
            }
            if msg == WM_MOUSEMOVE {
                let f = &mut app().fences[idx];
                if let Some((_, p)) = f.drag {
                    if !f.dragging && ((mx - p.x).abs() > 4 || (my - p.y).abs() > 4) {
                        f.dragging = true;
                        SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL));
                    }
                }
                return 0;
            }
        }
        WM_TIMER if wp == TIMER_HOVER => {
            // 커서가 창 밖으로 나가면 다시 접는다
            let f = &mut app().fences[idx];
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut r);
            let inside = pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom;
            if !inside && f.hover_expanded && f.drag.is_none() {
                KillTimer(hwnd, TIMER_HOVER);
                f.hover_expanded = false;
                apply_height(idx);
            }
            return 0;
        }
        WM_SIZE | WM_MOVE => {
            let a = app();
            let f = &mut a.fences[idx];
            let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut r);
            if !a.peeking {
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
            let a = app();
            a.focused = Some(idx);
            if !a.peeking {
                SetForegroundWindow(GetAncestor(hwnd, GA_ROOT));
            }
            let f = &mut a.fences[idx];
            // 포털 뒤로 가기 버튼
            if f.is_portal() && !f.nav.is_empty() && my < f.th() && mx < m(28) {
                portal_back(idx);
                return 0;
            }
            if f.locked && my < f.th() {
                return 0;
            }
            let hit = f.hit_item(mx, my);
            let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
            let shift = GetKeyState(VK_SHIFT as i32) < 0;
            match hit {
                Some(i) => {
                    if ctrl {
                        if let Some(p) = f.sel.iter().position(|&k| k == i) {
                            f.sel.remove(p);
                        } else {
                            f.sel.push(i);
                        }
                        f.anchor = Some(i);
                    } else if shift {
                        let an = f.anchor.unwrap_or(i);
                        let (lo, hi) = (an.min(i), an.max(i));
                        f.sel = (lo..=hi).collect();
                    } else if !f.sel.contains(&i) {
                        f.select_only(i);
                    } else {
                        f.anchor = Some(i);
                    }
                    f.drag = Some((i, POINT { x: mx, y: my }));
                    f.dragging = false;
                    SetCapture(hwnd);
                }
                None => {
                    if !ctrl && !shift {
                        f.sel.clear();
                    }
                }
            }
            InvalidateRect(hwnd, null(), 1);
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
            if !was_dragging {
                // 드래그 없이 클릭만 한 경우: 선택 집합 안의 항목을 클릭했으면 단일 선택으로
                let ctrl = GetKeyState(VK_CONTROL as i32) < 0;
                let shift = GetKeyState(VK_SHIFT as i32) < 0;
                if !ctrl && !shift && f.sel.len() > 1 && f.sel.contains(&i) {
                    f.select_only(i);
                    InvalidateRect(hwnd, null(), 1);
                }
                return 0;
            }
            if i < f.items.len() {
                finish_item_drag(idx, i, mx, my);
            }
            return 0;
        }
        WM_CAPTURECHANGED => {
            let f = &mut app().fences[idx];
            f.drag = None;
            f.dragging = false;
            return 0;
        }
        WM_LBUTTONDBLCLK => {
            let f = &app().fences[idx];
            if my < f.th() {
                if f.locked {
                    set_rolled(idx, !f.rolled);
                }
                return 0;
            }
            if let Some(i) = f.hit_item(mx, my) {
                open_item(idx, i);
            }
            return 0;
        }
        WM_RBUTTONUP => {
            app().focused = Some(idx);
            let mut spt = POINT { x: mx, y: my };
            ClientToScreen(hwnd, &mut spt);
            match app().fences[idx].hit_item(mx, my) {
                Some(i) => {
                    let f = &mut app().fences[idx];
                    if !f.sel.contains(&i) {
                        f.select_only(i);
                    }
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
            let mut paths = Vec::new();
            for i in 0..n {
                let mut buf = [0u16; 1024];
                let len = DragQueryFileW(hdrop, i, buf.as_mut_ptr(), buf.len() as u32);
                if len > 0 {
                    paths.push(from_wide(&buf[..len as usize]));
                }
            }
            DragFinish(hdrop);
            if app().fences[idx].is_portal() {
                portal_move_in(idx, &paths);
                rescan();
            } else {
                for p in &paths {
                    add_item_to(idx, p);
                }
                save();
            }
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

// 드래그 미리보기(반투명 사각형) 창
pub unsafe extern "system" fn band_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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
