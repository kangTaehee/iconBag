// 저수준 마우스 훅
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

// 바탕화면 빈 공간 더블클릭 감지용 저수준 마우스 훅
pub unsafe extern "system" fn mouse_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
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
