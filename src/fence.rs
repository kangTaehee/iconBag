// Fence: 데이터, 창 생성, 그리기, 메뉴, 창 프로시저
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
}
impl Drop for Item {
    fn drop(&mut self) {
        if !self.icon.is_null() {
            unsafe { DestroyIcon(self.icon) };
        }
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
    pub sel: Option<usize>,
    pub drag: Option<(usize, POINT)>,
    pub dragging: bool,
}

// ───────────────────────── Fence ─────────────────────────
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
            sel: None,
            drag: None,
            dragging: false,
        }
    }
    pub fn cols(&self) -> usize {
        (((self.w - 2 * m(PAD)) / m(CELL_W)).max(1)) as usize
    }
    pub fn content_h(&self) -> i32 {
        let rows = self.items.len().div_ceil(self.cols()) as i32;
        m(TITLE_H + PAD) + rows * m(CELL_H) + m(PAD)
    }
    pub fn clamp_scroll(&mut self) {
        let max = (self.content_h() - self.h).max(0);
        self.scroll = self.scroll.clamp(0, max);
    }
    pub fn cell_rect(&self, i: usize) -> RECT {
        let cols = self.cols();
        let (col, row) = ((i % cols) as i32, (i / cols) as i32);
        let cx = m(PAD) + col * m(CELL_W);
        let cy = m(TITLE_H + PAD) + row * m(CELL_H) - self.scroll;
        RECT { left: cx, top: cy, right: cx + m(CELL_W), bottom: cy + m(CELL_H) }
    }
    pub fn hit_item(&self, x: i32, y: i32) -> Option<usize> {
        if y < m(TITLE_H) {
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
}

pub fn fence_idx(hwnd: HWND) -> Option<usize> {
    app().fences.iter().position(|f| f.hwnd == hwnd)
}

pub unsafe fn create_fence_window(idx: usize) {
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
    let f = app().fences.remove(idx);
    if !f.hwnd.is_null() {
        DestroyWindow(f.hwnd);
    }
    save();
}

pub unsafe fn set_rolled(idx: usize, rolled: bool) {
    let f = &mut app().fences[idx];
    f.rolled = rolled;
    let h = if rolled { m(TITLE_H) } else { f.h };
    SetWindowPos(f.hwnd, null_mut(), 0, 0, f.w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    apply_region(f.hwnd);
    InvalidateRect(f.hwnd, null(), 1);
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

pub unsafe fn fence_menu(idx: usize, x: i32, y: i32) {
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

pub unsafe fn item_menu(idx: usize, item: usize, x: i32, y: i32) {
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

pub unsafe fn open_path(path: &str) {
    ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(path).as_ptr(), null(), null(), SW_SHOWNORMAL);
}

// ───────────────────────── 이름 바꾸기 팝업 ─────────────────────────
pub unsafe fn open_rename(idx: usize) {
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

pub unsafe fn commit_rename(edit: HWND, apply: bool) {
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

pub unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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

pub unsafe extern "system" fn rename_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wp, lp)
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
