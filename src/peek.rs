// 피크(Peek): 모든 Fence 를 잠시 최상위로 끌어올렸다가 되돌린다.
// Fence 는 explorer 바탕화면의 자식 창이라 다른 창 위로 올라갈 수 없으므로
// SetParent(NULL) 로 떼어 최상위 팝업으로 만들었다가 끝나면 다시 붙인다.
#![allow(unused_imports, dead_code)]
use crate::*;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// 팝업 → 바탕화면 자식으로 부착 (create_fence_window, peek_end 공용)
pub unsafe fn attach_to_desktop(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) -> bool {
    let a = app();
    if a.defview.is_null() || IsWindow(a.defview) == 0 {
        return false;
    }
    let st = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    SetWindowLongPtrW(hwnd, GWL_STYLE, ((st & !WS_POPUP) | WS_CHILD) as isize);
    // TOPMOST 는 SetWindowPos(HWND_NOTOPMOST) 로만 해제된다
    SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    SetParent(hwnd, a.defview);
    SetWindowPos(hwnd, null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);
    true
}

// 바탕화면 자식 → 최상위 팝업으로 분리
pub unsafe fn detach_to_top(hwnd: HWND) {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(hwnd, &mut r);
    let st = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    SetWindowLongPtrW(hwnd, GWL_STYLE, ((st & !WS_CHILD) | WS_POPUP) as isize);
    SetParent(hwnd, null_mut());
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
        let hwnd = a.fences[i].hwnd;
        if hwnd.is_null() {
            continue;
        }
        detach_to_top(hwnd);
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
        if !attach_to_desktop(f.hwnd, f.x, f.y, f.w, h) {
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
