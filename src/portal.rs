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

// ───────────────────────── 폴더 보기 옵션 ─────────────────────────
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SortKey {
    Name,
    Modified,
    Size,
    Type,
}
pub const SORT_KEYS: [(SortKey, &str, &str); 4] = [
    (SortKey::Name, "name", "이름"),
    (SortKey::Modified, "date", "수정한 날짜"),
    (SortKey::Size, "size", "크기"),
    (SortKey::Type, "type", "종류(확장자)"),
];
impl SortKey {
    pub fn as_str(self) -> &'static str {
        SORT_KEYS.iter().find(|k| k.0 == self).map(|k| k.1).unwrap_or("name")
    }
    pub fn parse(s: &str) -> SortKey {
        SORT_KEYS.iter().find(|k| k.1 == s).map(|k| k.0).unwrap_or(SortKey::Name)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FolderView {
    pub sort: SortKey,
    pub desc: bool,
    pub dirs_first: bool,
    pub show_hidden: bool,
}
impl Default for FolderView {
    fn default() -> Self {
        FolderView { sort: SortKey::Name, desc: false, dirs_first: true, show_hidden: false }
    }
}

// 정렬에 필요한 값만 미리 뽑아 둔 항목 (비교 중 파일 시스템을 다시 읽지 않기 위함)
pub struct EntryInfo {
    pub path: PathBuf,
    pub name: Vec<u16>, // StrCmpLogicalW 용 (NUL 종료)
    pub ext: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: u64,
}
impl EntryInfo {
    pub fn new(path: PathBuf, is_dir: bool, size: u64, modified: u64) -> EntryInfo {
        let name = wide(&display_name(&path));
        let ext = if is_dir { String::new() } else { file_ext(&path) };
        EntryInfo { path, name, ext, is_dir, size, modified }
    }
    pub fn from_meta(path: PathBuf, md: &std::fs::Metadata) -> EntryInfo {
        use std::os::windows::fs::MetadataExt;
        let is_dir = md.file_attributes() & 0x10 != 0; // 정션·폴더 심볼릭 링크도 폴더로
        EntryInfo::new(path, is_dir, if is_dir { 0 } else { md.file_size() }, md.last_write_time())
    }
}

// 탐색기와 같은 자연 정렬(파일2 < 파일10, 대소문자 무시)
fn cmp_name(a: &EntryInfo, b: &EntryInfo) -> std::cmp::Ordering {
    unsafe { StrCmpLogicalW(a.name.as_ptr(), b.name.as_ptr()) }.cmp(&0)
}

// 폴더 먼저 → 기준(오름/내림) → 이름 순으로 비교. 폴더 먼저는 내림차순이어도 유지한다.
pub fn compare_entries(a: &EntryInfo, b: &EntryInfo, v: &FolderView) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if v.dirs_first && a.is_dir != b.is_dir {
        return if a.is_dir { Ordering::Less } else { Ordering::Greater };
    }
    let key = match v.sort {
        SortKey::Name => cmp_name(a, b),
        SortKey::Modified => a.modified.cmp(&b.modified),
        SortKey::Size => a.size.cmp(&b.size),
        SortKey::Type => a.ext.cmp(&b.ext),
    };
    let key = if v.desc { key.reverse() } else { key };
    key.then_with(|| cmp_name(a, b))
}

pub fn portal_dir(f: &Fence) -> Option<PathBuf> {
    f.nav.last().cloned().or_else(|| f.portal.clone())
}

// 폴더 내용이나 순서가 바뀐 경우에만 items 를 다시 만든다. 남아 있는 항목의 아이콘은 재사용한다.
pub unsafe fn refresh_portal(idx: usize) {
    let f = &mut app().fences[idx];
    let Some(dir) = portal_dir(f) else { return };
    let view = f.view;
    let mut list: Vec<EntryInfo> = scan_entries(&dir, view.show_hidden).into_iter().map(|(p, md)| EntryInfo::from_meta(p, &md)).collect();
    list.sort_by(|a, b| compare_entries(a, b, &view));
    let same = list.len() == f.items.len() && list.iter().zip(f.items.iter()).all(|(e, it)| e.path.to_string_lossy() == it.path.as_str());
    if same {
        return;
    }
    let mut old: std::collections::HashMap<String, Item> = f.items.drain(..).map(|it| (it.path.clone(), it)).collect();
    f.items = list
        .iter()
        .map(|e| {
            let p = e.path.to_string_lossy();
            old.remove(p.as_ref()).unwrap_or_else(|| Item::new(&p))
        })
        .collect();
    f.sel.clear();
    invalidate(idx);
}

pub unsafe fn set_folder_view(idx: usize, view: FolderView) {
    app().fences[idx].view = view;
    refresh_portal(idx);
    save();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    fn file(name: &str, size: u64, modified: u64) -> EntryInfo {
        EntryInfo::new(PathBuf::from(format!("C:/t/{}", name)), false, size, modified)
    }
    fn dir(name: &str) -> EntryInfo {
        EntryInfo::new(PathBuf::from(format!("C:/t/{}", name)), true, 0, 0)
    }
    fn sorted(mut v: Vec<EntryInfo>, view: FolderView) -> Vec<String> {
        v.sort_by(|a, b| compare_entries(a, b, &view));
        v.iter().map(|e| from_wide(&e.name)).collect()
    }

    #[test]
    fn should_sort_names_naturally_and_case_insensitively() {
        let v = vec![file("file10.txt", 0, 0), file("File2.txt", 0, 0), file("file1.txt", 0, 0)];
        assert_eq!(sorted(v, FolderView::default()), ["file1.txt", "File2.txt", "file10.txt"]);
    }

    #[test]
    fn should_keep_folders_first_even_when_descending() {
        let v = vec![file("a.txt", 1, 0), dir("zdir"), file("b.txt", 2, 0), dir("adir")];
        let view = FolderView { desc: true, ..FolderView::default() };
        assert_eq!(sorted(v, view), ["zdir", "adir", "b.txt", "a.txt"]);
    }

    #[test]
    fn should_mix_folders_by_name_when_dirs_first_is_off() {
        let v = vec![file("b.txt", 0, 0), dir("c"), file("a.txt", 0, 0)];
        let view = FolderView { dirs_first: false, ..FolderView::default() };
        assert_eq!(sorted(v, view), ["a.txt", "b.txt", "c"]);
    }

    #[test]
    fn should_sort_by_size_then_name_for_ties() {
        let v = vec![file("big.bin", 300, 0), file("b.txt", 10, 0), file("a.txt", 10, 0)];
        let view = FolderView { sort: SortKey::Size, ..FolderView::default() };
        assert_eq!(sorted(v, view), ["a.txt", "b.txt", "big.bin"]);
    }

    #[test]
    fn should_sort_by_modified_time_descending_newest_first() {
        let v = vec![file("old.txt", 0, 100), file("new.txt", 0, 300), file("mid.txt", 0, 200)];
        let view = FolderView { sort: SortKey::Modified, desc: true, ..FolderView::default() };
        assert_eq!(sorted(v, view), ["new.txt", "mid.txt", "old.txt"]);
    }

    #[test]
    fn should_sort_by_extension_case_insensitively() {
        let v = vec![file("z.TXT", 0, 0), file("y.doc", 0, 0), file("x.pdf", 0, 0)];
        let view = FolderView { sort: SortKey::Type, ..FolderView::default() };
        assert_eq!(sorted(v, view), ["y.doc", "x.pdf", "z.TXT"]);
    }

    #[test]
    fn should_treat_identical_entries_as_equal() {
        let (a, b) = (file("same.txt", 5, 5), file("same.txt", 5, 5));
        assert_eq!(compare_entries(&a, &b, &FolderView::default()), Ordering::Equal);
    }

    #[test]
    fn should_round_trip_sort_key_and_fall_back_to_name_for_unknown_value() {
        for (key, s, _) in SORT_KEYS {
            assert_eq!(SortKey::parse(s), key);
            assert_eq!(key.as_str(), s);
        }
        assert_eq!(SortKey::parse("bogus"), SortKey::Name);
        assert_eq!(SortKey::parse(""), SortKey::Name);
    }
}
