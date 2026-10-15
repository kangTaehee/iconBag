// icondocs(아이콘독스) — Fences 스타일의 가벼운 바탕화면 아이콘 정리 도구 (Win32 API 직접 호출, 런타임 없음)
#![windows_subsystem = "windows"]
#![allow(non_snake_case, static_mut_refs, clippy::missing_safety_doc, unused_imports)]

mod config;
mod desktop;
mod fence;
mod input;
mod util;
pub use config::*;
pub use desktop::*;
pub use fence::*;
pub use input::*;
pub use util::*;

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
pub const CLASS_FENCE: &str = "IconDocsFence";
pub const CLASS_MAIN: &str = "IconDocsMain";
pub const CLASS_RENAME: &str = "IconDocsRename";
pub const NEW_ITEMS_TITLE: &str = "새 항목";

pub const TITLE_H: i32 = 30;
pub const CELL_W: i32 = 84;
pub const CELL_H: i32 = 88;
pub const PAD: i32 = 8;
pub const BORDER: i32 = 6;
pub const ALPHA: u8 = 215;
pub const MIN_W: i32 = 120;

pub const EM_SETSEL: u32 = 0x00B1;
pub const WM_TRAY: u32 = WM_APP + 1;
pub const WM_DESKTOP_DBLCLK: u32 = WM_APP + 2;
pub const WM_BAND_DOWN: u32 = WM_APP + 4;
pub const WM_BAND_MOVE: u32 = WM_APP + 5;
pub const WM_BAND_UP: u32 = WM_APP + 6;
pub const CLASS_BAND: &str = "IconDocsBand";
pub const TIMER_RESCAN: usize = 1;

pub const ID_TRAY_NEW: usize = 101;
pub const ID_TRAY_ICONS: usize = 102;
pub const ID_TRAY_HIDE: usize = 103;
pub const ID_TRAY_AUTORUN: usize = 104;
pub const ID_TRAY_EXIT: usize = 105;
pub const ID_TRAY_RESORT: usize = 106;
pub const ID_F_RENAME: usize = 201;
pub const ID_F_DELETE: usize = 202;
pub const ID_F_ROLL: usize = 203;
pub const ID_F_SORT: usize = 204;
pub const ID_F_COLOR0: usize = 210;
pub const ID_I_OPEN: usize = 301;
pub const ID_I_LOC: usize = 302;
pub const ID_I_REMOVE: usize = 303;

// COLORREF 는 0x00BBGGRR
pub const COLORS: [(&str, u32); 5] = [
    ("어두움", 0x2A2A2A),
    ("파랑", 0x6B3F1F),
    ("초록", 0x335522),
    ("보라", 0x622B4E),
    ("주황", 0x14468C),
];

pub struct App {
    pub hinst: HINSTANCE,
    pub main: HWND,
    pub defview: HWND,
    pub listview: HWND,
    pub fences: Vec<Fence>,
    pub fences_hidden: bool,
    pub icons_hidden: bool,
    pub font: HFONT,
    pub font_b: HFONT,
    pub hook: HHOOK,
    pub desktop_dirs: Vec<PathBuf>,
    pub last_click: (u32, i32, i32),
    pub band_start: Option<POINT>, // 바탕화면 드래그로 새 Fence 만들기
    pub band_hwnd: HWND,
    pub dpi: i32,
    pub icon_px: i32,
}

pub static mut APP: *mut App = null_mut();
pub static mut EDIT_OLD_PROC: isize = 0;

pub fn app() -> &'static mut App {
    unsafe { &mut *APP }
}
pub fn m(v: i32) -> i32 {
    v * app().dpi / 96
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

pub unsafe fn tray_menu() {
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

pub unsafe fn toggle_fences() {
    let a = app();
    a.fences_hidden = !a.fences_hidden;
    for f in &a.fences {
        ShowWindow(f.hwnd, if a.fences_hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
    }
}

// ───────────────────────── 메인(숨김) 창 프로시저 ─────────────────────────
pub unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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

// ───────────────────────── 진입점 ─────────────────────────
pub unsafe fn register(class: &str, proc: WNDPROC, style: u32, hinst: HINSTANCE) {
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

pub fn main() {
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
