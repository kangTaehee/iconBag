// icondocs(아이콘독스) — Fences 스타일의 가벼운 바탕화면 아이콘 정리 도구 (Win32 API 직접 호출, 런타임 없음)
#![windows_subsystem = "windows"]
#![allow(non_snake_case, static_mut_refs, clippy::missing_safety_doc, unused_imports)]

mod config;
mod desktop;
mod fence;
mod input;
mod peek;
mod portal;
mod snapshot;
mod util;
pub use config::*;
pub use desktop::*;
pub use fence::*;
pub use input::*;
pub use peek::*;
pub use portal::*;
pub use snapshot::*;
pub use util::*;

use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ───────────────────────── 상수 ─────────────────────────
pub const CLASS_FENCE: &str = "IconDocsFence";
pub const CLASS_MAIN: &str = "IconDocsMain";
pub const CLASS_RENAME: &str = "IconDocsRename";
pub const CLASS_BAND: &str = "IconDocsBand";
pub const NEW_ITEMS_TITLE: &str = "새 항목";

pub const TITLE_H: i32 = 30;
pub const GRIP_H: i32 = 8;
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
pub const WM_PEEK_CLICK: u32 = WM_APP + 7;
pub const WM_HOTCORNER: u32 = WM_APP + 8;
pub const WM_KEY: u32 = WM_APP + 9;
pub const TIMER_RESCAN: usize = 1;
pub const TIMER_HOVER: usize = 2;
pub const TIMER_TRAYCLICK: usize = 3;
pub const TIMER_HOTCORNER: usize = 4;
pub const TIMER_ICONS: usize = 5; // Fence 창: 남은 아이콘 이어서 로드
pub const ICON_BUDGET_MS: u64 = 12; // 한 번 그릴 때 아이콘 로드에 쓰는 최대 시간

pub const ID_TRAY_NEW: usize = 101;
pub const ID_TRAY_ICONS: usize = 102;
pub const ID_TRAY_HIDE: usize = 103;
pub const ID_TRAY_AUTORUN: usize = 104;
pub const ID_TRAY_EXIT: usize = 105;
pub const ID_TRAY_RESORT: usize = 106;
pub const ID_TRAY_PORTAL: usize = 107;
pub const ID_TRAY_REAPPLY: usize = 108;
pub const ID_TRAY_HOVER: usize = 109;
pub const ID_TRAY_PEEK: usize = 110;
pub const ID_TRAY_SNAP_SAVE: usize = 111;
pub const ID_TRAY_CORNER0: usize = 120; // 120..124
pub const ID_TRAY_SNAP0: usize = 400; // 복원
pub const ID_TRAY_SNAPDEL0: usize = 500; // 삭제
pub const SNAP_MAX: usize = 50;

pub const ID_F_RENAME: usize = 201;
pub const ID_F_DELETE: usize = 202;
pub const ID_F_ROLL: usize = 203;
pub const ID_F_SORT: usize = 204;
pub const ID_F_RULE_ADD: usize = 205;
pub const ID_F_LOCK: usize = 206;
pub const ID_F_TITLE: usize = 207;
pub const ID_F_PORTAL: usize = 208;
pub const ID_F_BACK: usize = 209;
pub const ID_F_COLOR0: usize = 210; // 210..214
pub const ID_F_PORTAL_OFF: usize = 215;
pub const ID_F_ICON16: usize = 220;
pub const ID_F_ICON32: usize = 221;
pub const ID_F_ICON48: usize = 222;
pub const ID_F_ALPHA0: usize = 230; // 230..233
pub const ID_F_RULE0: usize = 240;
pub const ID_F_RULE_MAX: usize = 40;
pub const ID_F_PSORT0: usize = 290; // 290..294 정렬 기준
pub const ID_F_PASC: usize = 295;
pub const ID_F_PDESC: usize = 296;
pub const ID_F_PDIRS: usize = 297;
pub const ID_F_PHIDDEN: usize = 298;
pub const ID_F_POPEN: usize = 299;
pub const ID_F_AUTOH: usize = 300;
pub const ID_I_OPEN: usize = 301;
pub const ID_I_LOC: usize = 302;
pub const ID_I_REMOVE: usize = 303;
pub const ID_I_RULE: usize = 304;

// COLORREF 는 0x00BBGGRR
pub const COLORS: [(&str, u32); 5] = [
    ("어두움", 0x2A2A2A),
    ("파랑", 0x6B3F1F),
    ("초록", 0x335522),
    ("보라", 0x622B4E),
    ("주황", 0x14468C),
];
pub const ALPHAS: [(&str, u8); 4] = [("50%", 128), ("70%", 180), ("85%", 215), ("100% (불투명)", 255)];
pub const CORNERS: [&str; 5] = ["사용 안 함", "왼쪽 위", "오른쪽 위", "왼쪽 아래", "오른쪽 아래"];

// ───────────────────────── 앱 상태 ─────────────────────────
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
    pub kbd_hook: HHOOK,
    pub desktop_dirs: Vec<PathBuf>,
    pub last_click: (u32, i32, i32),
    pub band_start: Option<POINT>, // 바탕화면 드래그로 새 Fence 만들기
    pub band_hwnd: HWND,
    pub dpi: i32,
    pub rules: Vec<Rule>,
    pub focused: Option<usize>, // 키보드 대상 Fence
    pub hover_expand: bool,
    pub hot_corner: u8,
    pub corner_armed: bool,
    pub hotkey_toggle: (u32, u32),
    pub hotkey_peek: (u32, u32),
    pub peeking: bool,
    pub peek_was_hidden: bool,
    pub last_res: (i32, i32),
}

pub static mut APP: *mut App = null_mut();

pub fn app() -> &'static mut App {
    unsafe { &mut *APP }
}
pub fn m(v: i32) -> i32 {
    v * app().dpi / 96
}

// ───────────────────────── 트레이 메뉴 ─────────────────────────
static mut SNAP_MENU_LIST: Vec<String> = Vec::new();

unsafe fn tray_menu() {
    let a = app();
    let menu = CreatePopupMenu();
    menu_item(menu, ID_TRAY_NEW, "새 Fence 만들기", false);
    menu_item(menu, ID_TRAY_PORTAL, "폴더 포털 만들기…", false);
    menu_item(menu, ID_TRAY_RESORT, "바탕화면 자동 정리 (폴더/프로그램/문서)", false);
    menu_item(menu, ID_TRAY_REAPPLY, "자동 분류 규칙 다시 적용", false);
    menu_sep(menu);
    menu_item(menu, ID_TRAY_PEEK, &format!("피크: 모든 Fence 맨 위로\t{}", hotkey_to_string(a.hotkey_peek)), a.peeking);
    menu_item(menu, ID_TRAY_HIDE, &format!("모든 Fence 숨기기\t{}", hotkey_to_string(a.hotkey_toggle)), a.fences_hidden);
    let sub = CreatePopupMenu();
    for (i, name) in CORNERS.iter().enumerate() {
        menu_item(sub, ID_TRAY_CORNER0 + i, name, a.hot_corner as usize == i);
    }
    menu_sub(menu, sub, "피크 핫코너");
    menu_item(menu, ID_TRAY_HOVER, "말아 올린 Fence 에 마우스 올리면 펼치기", a.hover_expand);
    menu_sep(menu);
    // 레이아웃 스냅샷
    SNAP_MENU_LIST = list_snapshots();
    let sub = CreatePopupMenu();
    menu_item(sub, ID_TRAY_SNAP_SAVE, "현재 배치 저장…", false);
    if !SNAP_MENU_LIST.is_empty() {
        menu_sep(sub);
        for (i, name) in SNAP_MENU_LIST.iter().enumerate().take(SNAP_MAX) {
            menu_item(sub, ID_TRAY_SNAP0 + i, &snap_label(name), false);
        }
        menu_sep(sub);
        let del = CreatePopupMenu();
        for (i, name) in SNAP_MENU_LIST.iter().enumerate().take(SNAP_MAX) {
            menu_item(del, ID_TRAY_SNAPDEL0 + i, &snap_label(name), false);
        }
        menu_sub(sub, del, "삭제");
    }
    menu_sub(menu, sub, "레이아웃");
    menu_sep(menu);
    menu_item(menu, ID_TRAY_ICONS, "원래 바탕화면 아이콘 숨기기", a.icons_hidden);
    menu_item(menu, ID_TRAY_AUTORUN, "Windows 시작 시 실행", autorun_enabled());
    menu_sep(menu);
    menu_item(menu, ID_TRAY_EXIT, "종료", false);
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    match track_menu(menu, pt.x, pt.y) {
        ID_TRAY_NEW => {
            let idx = new_fence("새 Fence", pt.x.saturating_sub(m(330)).max(0), pt.y.saturating_sub(m(320)).max(0), m(330), m(300), COLORS[0].1);
            invalidate(idx);
            save();
        }
        ID_TRAY_PORTAL => {
            if let Some(dir) = pick_folder("포털로 표시할 폴더를 선택하세요") {
                let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("포털").to_string();
                let idx = new_fence(&name, pt.x.saturating_sub(m(330)).max(0), pt.y.saturating_sub(m(320)).max(0), m(330), m(300), COLORS[1].1);
                set_portal(idx, Some(dir));
            }
        }
        ID_TRAY_RESORT => auto_sort(),
        ID_TRAY_REAPPLY => reapply_rules(),
        ID_TRAY_PEEK => peek_toggle(),
        ID_TRAY_HIDE => toggle_fences(),
        ID_TRAY_HOVER => {
            a.hover_expand = !a.hover_expand;
            save();
        }
        ID_TRAY_SNAP_SAVE => open_prompt(PromptKind::Snapshot, 0, Some(String::new())),
        ID_TRAY_ICONS => {
            a.icons_hidden = !a.icons_hidden;
            apply_icons_hidden();
            save();
        }
        ID_TRAY_AUTORUN => set_autorun(!autorun_enabled()),
        ID_TRAY_EXIT => {
            DestroyWindow(a.main);
        }
        c if (ID_TRAY_CORNER0..ID_TRAY_CORNER0 + CORNERS.len()).contains(&c) => {
            a.hot_corner = (c - ID_TRAY_CORNER0) as u8;
            a.corner_armed = false;
            save();
        }
        c if (ID_TRAY_SNAP0..ID_TRAY_SNAP0 + SNAP_MAX).contains(&c) => {
            if let Some(name) = SNAP_MENU_LIST.get(c - ID_TRAY_SNAP0) {
                restore_snapshot(name);
            }
        }
        c if (ID_TRAY_SNAPDEL0..ID_TRAY_SNAPDEL0 + SNAP_MAX).contains(&c) => {
            if let Some(name) = SNAP_MENU_LIST.get(c - ID_TRAY_SNAPDEL0) {
                delete_snapshot(name);
            }
        }
        _ => {}
    }
}

fn snap_label(name: &str) -> String {
    match name.strip_prefix("auto_") {
        Some(res) => format!("자동 ({})", res),
        None => name.to_string(),
    }
}

pub unsafe fn toggle_fences() {
    let a = app();
    if a.peeking {
        peek_end();
    }
    a.fences_hidden = !a.fences_hidden;
    for f in &a.fences {
        ShowWindow(f.hwnd, if a.fences_hidden { SW_HIDE } else { SW_SHOWNOACTIVATE });
    }
    save();
}

// ───────────────────────── 메인(숨김) 창 프로시저 ─────────────────────────
unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            let ev = lp as u32;
            if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                tray_menu();
            } else if ev == WM_LBUTTONUP {
                // 단일 클릭 = 피크 (더블클릭과 구분하기 위해 지연)
                SetTimer(hwnd, TIMER_TRAYCLICK, GetDoubleClickTime(), None);
            } else if ev == WM_LBUTTONDBLCLK {
                KillTimer(hwnd, TIMER_TRAYCLICK);
                toggle_fences();
            }
            return 0;
        }
        WM_TIMER => {
            match wp {
                TIMER_RESCAN => {
                    rescan();
                    apply_icons_hidden();
                }
                TIMER_TRAYCLICK => {
                    KillTimer(hwnd, TIMER_TRAYCLICK);
                    peek_toggle();
                }
                TIMER_HOTCORNER => {
                    KillTimer(hwnd, TIMER_HOTCORNER);
                    let mut pt = POINT { x: 0, y: 0 };
                    GetCursorPos(&mut pt);
                    if in_hot_corner(pt) && !app().peeking {
                        peek_start();
                    }
                }
                _ => {}
            }
            return 0;
        }
        WM_HOTKEY => {
            match wp as i32 {
                HK_TOGGLE => toggle_fences(),
                HK_PEEK => peek_toggle(),
                _ => {}
            }
            return 0;
        }
        WM_KEY => {
            handle_key(wp as u32, lp != 0);
            return 0;
        }
        WM_HOTCORNER => {
            if wp != 0 {
                SetTimer(hwnd, TIMER_HOTCORNER, 300, None);
            } else {
                KillTimer(hwnd, TIMER_HOTCORNER);
            }
            return 0;
        }
        WM_PEEK_CLICK => {
            // 피크 중 Fence 바깥 클릭 → 종료
            let pt = POINT { x: wp as i32, y: lp as i32 };
            let target = WindowFromPoint(pt);
            if fence_idx(target).is_none() && class_name(target) != CLASS_RENAME {
                peek_end();
            }
            return 0;
        }
        WM_DISPLAYCHANGE => {
            on_display_change((lp & 0xFFFF) as i32, ((lp >> 16) & 0xFFFF) as i32);
            return 0;
        }
        WM_DESKTOP_DBLCLK => {
            let pt = POINT { x: wp as i32, y: lp as i32 };
            if is_empty_desktop(pt) {
                toggle_fences();
            }
            return 0;
        }
        WM_BAND_DOWN => {
            let pt = POINT { x: wp as i32, y: lp as i32 };
            if is_empty_desktop(pt) {
                let a = app();
                a.band_start = Some(pt);
                // 바탕화면 빈 곳 클릭: 선택 해제
                a.focused = None;
                for i in 0..a.fences.len() {
                    if !a.fences[i].sel.is_empty() {
                        a.fences[i].sel.clear();
                        InvalidateRect(a.fences[i].hwnd, null(), 1);
                    }
                }
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
                    s.x,
                    s.y,
                    1,
                    1,
                    null_mut(),
                    null_mut(),
                    a.hinst,
                    null(),
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
            if had_band && w >= m(MIN_W) && h >= m(TITLE_H + 40) {
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
            if a.peeking {
                peek_end();
            }
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
            if !a.kbd_hook.is_null() {
                UnhookWindowsHookEx(a.kbd_hook);
            }
            UnregisterHotKey(hwnd, HK_TOGGLE);
            UnregisterHotKey(hwnd, HK_PEEK);
            PostQuitMessage(0);
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

// 외부 의존성 없이 현재 시각을 ISO 비슷하게 기록
fn chrono_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("unix={}", secs)
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
        CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
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
            kbd_hook: null_mut(),
            desktop_dirs: dirs,
            last_click: (0, 0, 0),
            band_start: None,
            band_hwnd: null_mut(),
            dpi,
            rules: Vec::new(),
            focused: None,
            hover_expand: true,
            hot_corner: 2,
            corner_armed: false,
            hotkey_toggle: (0x0002 | 0x0004, b'F' as u32),
            hotkey_peek: (0x0002 | 0x0004, b'P' as u32),
            peeking: false,
            peek_was_hidden: false,
            last_res: (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)),
        });
        APP = Box::into_raw(app_box);

        // 패닉 시(panic=abort 라도 훅은 먼저 실행됨) 바탕화면 아이콘을 복원하고 원인을 기록한다
        std::panic::set_hook(Box::new(|info| {
            let a = app();
            if !a.listview.is_null() {
                ShowWindowAsync(a.listview, SW_SHOW);
            }
            let msg = format!("{}
{}

", chrono_now(), info);
            let p = config_dir().join("crash.log");
            let _ = std::fs::create_dir_all(config_dir());
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                use std::io::Write;
                let _ = f.write_all(msg.as_bytes());
            }
        }));

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
        let tip = wide("아이콘독스 — 클릭: 피크 / 더블클릭: 숨기기·보이기 / 우클릭: 메뉴");
        nid.szTip[..tip.len().min(128)].copy_from_slice(&tip[..tip.len().min(128)]);
        Shell_NotifyIconW(NIM_ADD, &nid);

        // 레이아웃 불러오기 또는 최초 자동 정리
        if load() {
            if app().rules.is_empty() {
                seed_default_rules();
            }
            for i in 0..app().fences.len() {
                create_fence_window(i);
                if app().fences[i].portal.is_some() {
                    refresh_portal(i);
                }
                invalidate(i);
            }
        } else {
            app().icons_hidden = true;
            auto_sort();
        }
        apply_icons_hidden();
        rescan();

        app().hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), hinst, 0);
        app().kbd_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), hinst, 0);
        register_hotkeys();
        SetTimer(main, TIMER_RESCAN, 3000, None);

        let mut msg: MSG = core::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        DeleteObject(app().font as HGDIOBJ);
        DeleteObject(app().font_b as HGDIOBJ);
    }
}
