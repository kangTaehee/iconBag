// 폴더 포털: 임의 폴더 내용을 Fence 안에 표시, 하위 폴더 탐색, 드롭 시 파일 이동
#![allow(unused_imports, dead_code)]
use crate::*;
use core::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub fn portal_dir(f: &Fence) -> Option<PathBuf> {
    f.nav.last().cloned().or_else(|| f.portal.clone())
}

// 폴더 내용이 바뀐 경우에만 items 를 다시 만든다
pub unsafe fn refresh_portal(idx: usize) {
    let f = &mut app().fences[idx];
    let Some(dir) = portal_dir(f) else { return };
    let mut list = scan_dir(&dir);
    list.sort_by_key(|p| (!p.is_dir(), lower(&p.to_string_lossy())));
    let same = list.len() == f.items.len()
        && list.iter().zip(f.items.iter()).all(|(p, it)| lower(&p.to_string_lossy()) == lower(&it.path));
    if same {
        return;
    }
    f.items = list.iter().map(|p| Item::new(&p.to_string_lossy())).collect();
    f.sel.clear();
    invalidate(idx);
}

pub unsafe fn portal_enter(idx: usize, dir: PathBuf) {
    let f = &mut app().fences[idx];
    f.nav.push(dir);
    f.items.clear();
    f.scroll = 0;
    refresh_portal(idx);
}

pub unsafe fn portal_back(idx: usize) {
    let f = &mut app().fences[idx];
    if f.nav.pop().is_some() {
        f.items.clear();
        f.scroll = 0;
        refresh_portal(idx);
    }
}

pub unsafe fn set_portal(idx: usize, root: Option<PathBuf>) {
    let f = &mut app().fences[idx];
    f.portal = root;
    f.nav.clear();
    f.items.clear();
    f.sel.clear();
    f.scroll = 0;
    if f.portal.is_some() {
        refresh_portal(idx);
    }
    invalidate(idx);
    save();
    rescan(); // 포털 전환 시 기존 항목은 규칙/새 항목으로 돌아감
}

// 드롭된 파일을 포털 폴더로 이동
pub unsafe fn portal_move_in(idx: usize, paths: &[String]) {
    let Some(dir) = portal_dir(&app().fences[idx]) else { return };
    for p in paths {
        let src = Path::new(p);
        let Some(name) = src.file_name() else { continue };
        let dst = dir.join(name);
        if dst.exists() || lower(&dst.to_string_lossy()) == lower(p) {
            continue;
        }
        MoveFileExW(wide(p).as_ptr(), wide(&dst.to_string_lossy()).as_ptr(), MOVEFILE_COPY_ALLOWED);
    }
    app().fences[idx].items.clear();
    refresh_portal(idx);
}

// 폴더 선택 대화상자
pub unsafe fn pick_folder(title: &str) -> Option<PathBuf> {
    let mut name = [0u16; 260];
    let t = wide(title);
    let bi = BROWSEINFOW {
        hwndOwner: app().main,
        pidlRoot: null_mut(),
        pszDisplayName: name.as_mut_ptr(),
        lpszTitle: t.as_ptr(),
        ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE | BIF_EDITBOX,
        lpfn: None,
        lParam: 0,
        iImage: 0,
    };
    SetForegroundWindow(app().main);
    let pidl = SHBrowseForFolderW(&bi);
    if pidl.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let ok = SHGetPathFromIDListW(pidl, buf.as_mut_ptr());
    CoTaskMemFree(pidl as *const c_void);
    if ok == 0 {
        return None;
    }
    Some(PathBuf::from(from_wide(&buf)))
}
