// 피크(Peek): 모든 Fence 를 잠시 최상위로 끌어올렸다가 되돌린다.
// Fence 는 explorer 바탕화면의 자식 창이라 다른 창 위로 올라갈 수 없으므로
// SetParent(NULL) 로 떼어 최상위 팝업으로 만들었다가 끝나면 다시 붙인다.
#![allow(unused_imports, dead_code)]
use crate::*;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// 레이어드 투명도 적용. 창을 최상위 ↔ 자식으로 옮기면 DWM 합성 표면이 끊겨 창이 그려지지 않으므로
// WS_EX_LAYERED 를 껐다 켜서 표면을 새로 만든 뒤 알파를 다시 지정해야 한다.
pub unsafe fn reset_layered(hwnd: HWND, alpha: u8) {
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & !WS_EX_TOPMOST;
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex & !WS_EX_LAYERED) as isize);
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex | WS_EX_LAYERED) as isize);
    SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA);
}

// 팝업 → 바탕화면 자식으로 부착 (create_fence_window, peek_end 공용)
pub unsafe fn attach_to_desktop(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, alpha: u8) -> bool {
    let a = app();
    if a.defview.is_null() || IsWindow(a.defview) == 0 {
        return false;
    }
    // TOPMOST 는 SetWindowPos(HWND_NOTOPMOST) 로만 해제된다. WS_CHILD 로 바꾸기 전, 팝업일 때 해제해야 적용된다.
    SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    let st = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    SetWindowLongPtrW(hwnd, GWL_STYLE, ((st & !WS_POPUP) | WS_CHILD) as isize);
    SetParent(hwnd, a.defview);
    reset_layered(hwnd, alpha);
    SetWindowPos(hwnd, HWND_TOP, x, y, w, h, SWP_NOACTIVATE | SWP_FRAMECHANGED);
    RedrawWindow(hwnd, null(), null_mut(), RDW_INVALIDATE | RDW_ERASE | RDW_FRAME | RDW_UPDATENOW);
    true
}

// 바탕화면 자식 → 최상위 팝업으로 분리
pub unsafe fn detach_to_top(hwnd: HWND, alpha: u8) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(hwnd, &mut r);
    let st = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    SetWindowLongPtrW(hwnd, GWL_STYLE, ((st & !WS_CHILD) | WS_POPUP) as isize);
    SetParent(hwnd, null_mut());
    reset_layered(hwnd, alpha);
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
    );
}

pub unsafe fn peek_start() {
    let a = app();
    if a.peeking {
        return;
    }
    a.peeking = true;
    a.peek_was_hidden = a.fences_hidden;
    for i in 0..a.fences.len() {
        let (hwnd, alpha) = (a.fences[i].hwnd, a.fences[i].alpha);
        if hwnd.is_null() {
            continue;
        }
        detach_to_top(hwnd, alpha);
        InvalidateRect(hwnd, null(), 1);
    }
}

pub unsafe fn peek_end() {
    let a = app();
    if !a.peeking {
        return;
    }
    a.peeking = false;
    for i in 0..a.fences.len() {
        let f = &a.fences[i];
        if f.hwnd.is_null() {
            continue;
        }
        let h = fence_height(f);
        if !attach_to_desktop(f.hwnd, f.x, f.y, f.w, h, f.alpha) {
            // 바탕화면을 못 찾으면 맨 아래 팝업으로 둔다
            let ex = GetWindowLongPtrW(f.hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(f.hwnd, GWL_EXSTYLE, (ex & !WS_EX_TOPMOST) as isize);
            SetWindowPos(f.hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        if a.peek_was_hidden {
            ShowWindow(f.hwnd, SW_HIDE);
        }
    }
}

pub unsafe fn peek_toggle() {
    if app().peeking {
        peek_end();
    } else {
        peek_start();
    }
}
