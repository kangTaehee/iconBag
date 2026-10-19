// 저수준 마우스/키보드 훅, 전역 핫키, 핫코너
#![allow(unused_imports, dead_code)]
use crate::*;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU8, Ordering::Relaxed};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub const HK_TOGGLE: i32 = 1;
pub const HK_PEEK: i32 = 2;
pub const MOD_NOREPEAT_FLAG: u32 = 0x4000;

// ───────────────────────── 훅 스레드와 공유하는 상태 ─────────────────────────
// 저수준 훅은 설치한 스레드에서 실행되고, 그 스레드가 바쁘면 시스템 전체의 마우스·키 입력이 함께 밀린다.
// 그래서 훅은 메시지 루프만 도는 전용 스레드에 두고, UI 스레드의 App 대신 아래 원자 변수만 읽는다.
// UI 스레드는 값이 바뀔 때 set_* 로 갱신한다.
static HOOK_MAIN: AtomicIsize = AtomicIsize::new(0);
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
static HOOK_PEEKING: AtomicBool = AtomicBool::new(false);
static HOOK_FOCUSED: AtomicBool = AtomicBool::new(false);
static HOOK_BAND: AtomicBool = AtomicBool::new(false);
static HOOK_CORNER: AtomicU8 = AtomicU8::new(0);
static HOOK_CORNER_ARMED: AtomicBool = AtomicBool::new(false);
// 더블클릭 판정용 직전 클릭 (훅 스레드 전용)
static mut LAST_CLICK: (u32, i32, i32) = (0, 0, 0);

pub fn set_peeking(on: bool) {
    app().peeking = on;
    HOOK_PEEKING.store(on, Relaxed);
}
pub fn set_focused(f: Option<usize>) {
    app().focused = f;
    HOOK_FOCUSED.store(f.is_some(), Relaxed);
}
pub fn set_band_start(p: Option<POINT>) {
    app().band_start = p;
    HOOK_BAND.store(p.is_some(), Relaxed);
}
pub fn set_hot_corner(c: u8) {
    app().hot_corner = c;
    HOOK_CORNER.store(c, Relaxed);
    HOOK_CORNER_ARMED.store(false, Relaxed);
}

fn hook_main() -> HWND {
    HOOK_MAIN.load(Relaxed) as HWND
}

// 훅 스레드 시작: 훅을 설치하고 WM_QUIT 이 올 때까지 메시지 루프를 돈다
pub unsafe fn start_hook_thread() {
    let a = app();
    HOOK_MAIN.store(a.main as isize, Relaxed);
    HOOK_CORNER.store(a.hot_corner, Relaxed);
    let hinst = a.hinst as isize;
    std::thread::spawn(move || unsafe {
        HOOK_THREAD.store(GetCurrentThreadId(), Relaxed);
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), hinst as HINSTANCE, 0);
        let kbd = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), hinst as HINSTANCE, 0);
        let mut msg: MSG = core::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(mouse);
        UnhookWindowsHookEx(kbd);
    });
}

pub unsafe fn stop_hook_thread() {
    let tid = HOOK_THREAD.load(Relaxed);
    if tid != 0 {
        PostThreadMessageW(tid, WM_QUIT, 0, 0);
    }
}

pub unsafe fn register_hotkeys() {
    let a = app();
    UnregisterHotKey(a.main, HK_TOGGLE);
    UnregisterHotKey(a.main, HK_PEEK);
    RegisterHotKey(a.main, HK_TOGGLE, a.hotkey_toggle.0 | MOD_NOREPEAT_FLAG, a.hotkey_toggle.1);
    RegisterHotKey(a.main, HK_PEEK, a.hotkey_peek.0 | MOD_NOREPEAT_FLAG, a.hotkey_peek.1);
}

// 커서가 설정된 핫코너(가상 화면 모서리 3px 이내)에 있는지
pub unsafe fn in_hot_corner(pt: POINT) -> bool {
    in_corner(pt, app().hot_corner)
}

unsafe fn in_corner(pt: POINT, c: u8) -> bool {
    if c == 0 {
        return false;
    }
    let (vx, vy) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
    let (vw, vh) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
    let (l, t, r, b) = (vx, vy, vx + vw - 1, vy + vh - 1);
    let near = |a: i32, b: i32| (a - b).abs() <= 3;
    match c {
        1 => near(pt.x, l) && near(pt.y, t),
        2 => near(pt.x, r) && near(pt.y, t),
        3 => near(pt.x, l) && near(pt.y, b),
        4 => near(pt.x, r) && near(pt.y, b),
        _ => false,
    }
}

// 바탕화면 더블클릭 / 드래그로 Fence 생성 / 피크 종료 / 핫코너 감지
pub unsafe extern "system" fn mouse_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let ms = &*(lp as *const MSLLHOOKSTRUCT);
        let main = hook_main();
        match wp as u32 {
            WM_LBUTTONDOWN => {
                let (t0, x0, y0) = LAST_CLICK;
                if t0 != 0
                    && ms.time.wrapping_sub(t0) <= GetDoubleClickTime()
                    && (ms.pt.x - x0).abs() <= 4
                    && (ms.pt.y - y0).abs() <= 4
                {
                    LAST_CLICK = (0, 0, 0);
                    PostMessageW(main, WM_DESKTOP_DBLCLK, ms.pt.x as usize, ms.pt.y as isize);
                } else {
                    LAST_CLICK = (ms.time, ms.pt.x, ms.pt.y);
                }
                if HOOK_PEEKING.load(Relaxed) {
                    PostMessageW(main, WM_PEEK_CLICK, ms.pt.x as usize, ms.pt.y as isize);
                }
                PostMessageW(main, WM_BAND_DOWN, ms.pt.x as usize, ms.pt.y as isize);
            }
            WM_MOUSEMOVE => {
                if HOOK_BAND.load(Relaxed) {
                    PostMessageW(main, WM_BAND_MOVE, ms.pt.x as usize, ms.pt.y as isize);
                }
                let corner = HOOK_CORNER.load(Relaxed);
                if corner != 0 {
                    let inside = in_corner(ms.pt, corner);
                    if inside != HOOK_CORNER_ARMED.load(Relaxed) {
                        HOOK_CORNER_ARMED.store(inside, Relaxed);
                        PostMessageW(main, WM_HOTCORNER, inside as usize, 0);
                    }
                }
            }
            WM_LBUTTONUP => {
                // WM_BAND_DOWN 이 아직 처리되기 전일 수 있으므로 항상 보낸다(band_start 가 없으면 무시됨)
                PostMessageW(main, WM_BAND_UP, ms.pt.x as usize, ms.pt.y as isize);
            }
            _ => {}
        }
    }
    CallNextHookEx(null_mut(), code, wp, lp)
}

// Fence 는 explorer 의 자식 창이라 키 입력이 우리 스레드로 오지 않는다.
// 바탕화면이 전경일 때만 선택된 Fence 를 대상으로 키를 가로챈다.
pub unsafe extern "system" fn keyboard_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && (wp as u32 == WM_KEYDOWN || wp as u32 == WM_SYSKEYDOWN) {
        let kb = &*(lp as *const KBDLLHOOKSTRUCT);
        let vk = kb.vkCode;
        let main = hook_main();
        let ctrl = GetAsyncKeyState(VK_CONTROL as i32) < 0;
        if HOOK_PEEKING.load(Relaxed) && vk == VK_ESCAPE as u32 {
            PostMessageW(main, WM_KEY, vk as usize, 0);
            return 1;
        }
        if HOOK_FOCUSED.load(Relaxed) && desktop_is_foreground() {
            let handled = matches!(
                vk as u16,
                VK_RETURN | VK_DELETE | VK_F2 | VK_ESCAPE | VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT | VK_BACK
            ) || (ctrl && vk == b'A' as u32);
            if handled {
                PostMessageW(main, WM_KEY, vk as usize, ctrl as isize);
                return 1;
            }
        }
    }
    CallNextHookEx(null_mut(), code, wp, lp)
}

// WM_KEY 처리 (메인 창에서 호출)
pub unsafe fn handle_key(vk: u32, ctrl: bool) {
    let a = app();
    if vk == VK_ESCAPE as u32 && a.peeking {
        peek_end();
        return;
    }
    let Some(idx) = a.focused else { return };
    if idx >= a.fences.len() {
        set_focused(None);
        return;
    }
    let f = &mut a.fences[idx];
    match vk as u16 {
        VK_RETURN => {
            let sel: Vec<usize> = f.sel.clone();
            for i in sel {
                open_item(idx, i);
            }
        }
        VK_DELETE => {
            if f.portal.is_none() && !f.sel.is_empty() {
                let mut sel = f.sel.clone();
                sel.sort_unstable_by(|a, b| b.cmp(a));
                for i in sel {
                    if i < f.items.len() {
                        f.items.remove(i);
                    }
                }
                f.sel.clear();
                invalidate(idx);
                save();
                rescan();
            }
        }
        VK_F2 => open_prompt(PromptKind::Rename, idx, None),
        VK_ESCAPE => {
            f.sel.clear();
            invalidate(idx);
        }
        VK_BACK => {
            if f.portal.is_some() {
                portal_back(idx);
            }
        }
        VK_UP | VK_DOWN | VK_LEFT | VK_RIGHT => {
            if f.items.is_empty() {
                return;
            }
            let cols = f.cols() as i32;
            let cur = f.anchor.filter(|&i| i < f.items.len()).unwrap_or(0) as i32;
            let n = f.items.len() as i32;
            let next = match vk as u16 {
                VK_UP => cur - cols,
                VK_DOWN => cur + cols,
                VK_LEFT => cur - 1,
                _ => cur + 1,
            };
            if (0..n).contains(&next) {
                f.sel = vec![next as usize];
                f.anchor = Some(next as usize);
                f.ensure_visible(next as usize);
                invalidate(idx);
            }
        }
        _ if ctrl && vk == b'A' as u32 => {
            f.sel = (0..f.items.len()).collect();
            invalidate(idx);
        }
        _ => {}
    }
}
