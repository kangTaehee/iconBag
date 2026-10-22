// 바탕화면 검색: 단축키(기본 Win+Space)로 화면 중앙에 검색창을 띄우고, 바탕화면 항목을 이름으로 찾아 Enter 로 실행한다.
#![allow(unused_imports, dead_code)]
use crate::*;
use core::ffi::c_void;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const SEARCH_W: i32 = 560;
const SEARCH_TOP_H: i32 = 52; // 입력란 영역
const SEARCH_ROW_H: i32 = 40;
const SEARCH_MAX: usize = 8;
const EM_SETCUEBANNER: u32 = 0x1501;
const EN_CHANGE: u32 = 0x0300;

// ───────────────────────── 검색 순위 ─────────────────────────
// 같은 점수에서의 순서: 실행 항목(바로 가기·실행 파일) → 실행 중인 창 → 그 밖의 파일
const KIND_APP: u8 = 0;
const KIND_WINDOW: u8 = 1;
const KIND_FILE: u8 = 2;

pub struct SearchEntry {
    pub item: Item,
    key: String,      // 소문자 이름
    initials: String, // 한글 초성으로 바꾼 이름 ("카카오톡" → "ㅋㅋㅇㅌ")
    kind: u8,         // KIND_*
    window: HWND,     // 실행 중인 창 항목이면 그 창(파일 항목은 null)
}
impl SearchEntry {
    pub fn new(path: &str) -> SearchEntry {
        let item = Item::new(path);
        let is_app = matches!(file_ext(Path::new(path)).as_str(), "lnk" | "exe" | "url" | "appref-ms");
        Self::build(item, if is_app { KIND_APP } else { KIND_FILE }, null_mut())
    }
    // 실행 중인 최상위 창. 이름은 "창 제목 - 프로그램 이름" 이라 프로그램 이름으로도 검색된다.
    // 아이콘은 창에서 복사해 둔다(Item 이 DestroyIcon 으로 해제하므로 원본을 쓰면 안 된다).
    pub unsafe fn window(hwnd: HWND, title: &str) -> SearchEntry {
        let exe = process_path(hwnd);
        let prog = match exe.as_deref() {
            Some(p) => program_name(p),
            None => String::new(),
        };
        let mut item = Item::new("");
        item.name = window_display_name(title, &prog);
        item.icon = window_icon(hwnd, exe.as_deref());
        Self::build(item, KIND_WINDOW, hwnd)
    }
    fn build(item: Item, kind: u8, window: HWND) -> SearchEntry {
        let key = lower(&item.name);
        let initials = key.chars().map(choseong).collect();
        SearchEntry { item, key, initials, kind, window }
    }
    fn is_window(&self) -> bool {
        !self.window.is_null()
    }
}

// ───────────────────────── 실행 중인 창 ─────────────────────────
const DWMWA_CLOAKED: u32 = 14;

// 작업 표시줄에 보이는 창과 비슷한 기준: 보이고, 제목이 있고, 도구 창이 아니고, 숨겨진(cloaked) UWP 창이 아니다.
unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
    let out = &mut *(lp as *mut Vec<SearchEntry>);
    if IsWindowVisible(hwnd) == 0 || GetWindow(hwnd, GW_OWNER) != null_mut() {
        return 1;
    }
    let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if ex & WS_EX_TOOLWINDOW != 0 {
        return 1;
    }
    let mut cloaked: u32 = 0;
    DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4);
    if cloaked != 0 {
        return 1;
    }
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if n <= 0 {
        return 1;
    }
    let mut cls = [0u16; 64];
    let cn = GetClassNameW(hwnd, cls.as_mut_ptr(), cls.len() as i32);
    let class = from_wide(&cls[..cn.max(0) as usize]);
    if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
        || class.starts_with("IconDocs")
    {
        return 1;
    }
    out.push(SearchEntry::window(hwnd, &from_wide(&buf[..n as usize])));
    1
}

unsafe fn running_windows() -> Vec<SearchEntry> {
    let mut v: Vec<SearchEntry> = Vec::new();
    EnumWindows(Some(enum_windows_cb), &mut v as *mut Vec<SearchEntry> as LPARAM);
    v
}

// 창 아이콘 복사본. 응답 없는 앱에 막히지 않게 WM_GETICON 은 짧은 시간만 기다리고, 없으면 실행 파일 아이콘을 쓴다.
unsafe fn window_icon(hwnd: HWND, exe: Option<&str>) -> HICON {
    let mut h: usize = 0;
    for kind in [ICON_SMALL2, ICON_BIG, ICON_SMALL] {
        SendMessageTimeoutW(hwnd, WM_GETICON, kind as usize, 0, SMTO_ABORTIFHUNG | SMTO_BLOCK, 100, &mut h);
        if h != 0 {
            break;
        }
    }
    if h == 0 {
        h = GetClassLongPtrW(hwnd, GCLP_HICON) as usize;
    }
    if h == 0 {
        h = GetClassLongPtrW(hwnd, GCLP_HICONSM) as usize;
    }
    if h != 0 {
        return CopyIcon(h as HICON);
    }
    exe.map(|p| load_icon(p, 32)).unwrap_or(null_mut())
}

// 창을 만든 프로세스의 실행 파일 경로
unsafe fn process_path(hwnd: HWND) -> Option<String> {
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if proc.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(proc, 0, buf.as_mut_ptr(), &mut len);
    CloseHandle(proc);
    (ok != 0).then(|| from_wide(&buf[..len as usize]))
}

// 프로그램 이름: 실행 파일의 버전 정보 FileDescription("Visual Studio 2022" 등), 없으면 파일 이름(확장자 제외)
unsafe fn program_name(exe: &str) -> String {
    file_description(exe).unwrap_or_else(|| {
        Path::new(exe).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    })
}

unsafe fn file_description(exe: &str) -> Option<String> {
    let p = wide(exe);
    let mut handle = 0u32;
    let size = GetFileVersionInfoSizeW(p.as_ptr(), &mut handle);
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    if GetFileVersionInfoW(p.as_ptr(), 0, size, data.as_mut_ptr() as *mut c_void) == 0 {
        return None;
    }
    // 번역 표(언어·코드 페이지) 첫 항목을 쓰고, 없으면 영어(미국)·유니코드
    let mut ptr: *mut c_void = null_mut();
    let mut len = 0u32;
    let mut lang_cp = (0x0409u16, 0x04B0u16);
    if VerQueryValueW(data.as_ptr() as *const c_void, wide("\\VarFileInfo\\Translation").as_ptr(), &mut ptr, &mut len) != 0
        && len >= 4
    {
        let t = ptr as *const u16;
        lang_cp = (*t, *t.add(1));
    }
    let key = wide(&format!("\\StringFileInfo\\{:04X}{:04X}\\FileDescription", lang_cp.0, lang_cp.1));
    if VerQueryValueW(data.as_ptr() as *const c_void, key.as_ptr(), &mut ptr, &mut len) == 0 || len == 0 {
        return None;
    }
    let s = std::slice::from_raw_parts(ptr as *const u16, len as usize);
    let s = from_wide(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]);
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

// "창 제목 - 프로그램 이름". 프로그램 이름이 없거나 제목이 이미 그 이름으로 끝나면 제목만
pub fn window_display_name(title: &str, prog: &str) -> String {
    let title = title.trim();
    if prog.is_empty() || lower(title).ends_with(&lower(prog)) {
        title.to_string()
    } else {
        format!("{title} - {prog}")
    }
}

// 창으로 전환: 최소화돼 있으면 복원. 검색창이 전경인 동안 호출해야 전경 전환이 허용된다.
unsafe fn switch_to_window(hwnd: HWND) {
    if IsWindow(hwnd) == 0 {
        return;
    }
    if IsIconic(hwnd) != 0 {
        ShowWindow(hwnd, SW_RESTORE);
    }
    SetForegroundWindow(hwnd);
}

const CHOSEONG: [char; 19] =
    ['ㄱ', 'ㄲ', 'ㄴ', 'ㄷ', 'ㄸ', 'ㄹ', 'ㅁ', 'ㅂ', 'ㅃ', 'ㅅ', 'ㅆ', 'ㅇ', 'ㅈ', 'ㅉ', 'ㅊ', 'ㅋ', 'ㅌ', 'ㅍ', 'ㅎ'];

// 한글 음절이면 초성, 아니면 그대로
fn choseong(c: char) -> char {
    if ('가'..='힣').contains(&c) {
        CHOSEONG[((c as u32 - 0xAC00) / 588) as usize]
    } else {
        c
    }
}

fn is_initials_query(t: &str) -> bool {
    t.chars().all(|c| CHOSEONG.contains(&c))
}

// text 에서 token 이 처음 나오는 위치가 단어 시작인지(None = 없음)
fn find_token(text: &str, token: &str) -> Option<bool> {
    let p = text.find(token)?;
    Some(text[..p].chars().last().is_none_or(|c| matches!(c, ' ' | '-' | '_' | '.' | '(' | '[' | '+')))
}

// 낮을수록 앞: 0 = 이름이 검색어로 시작, 1 = 첫 단어가 어떤 단어의 시작, 2 = 중간. 모든 단어가 들어 있어야 한다.
fn match_score(e: &SearchEntry, query: &str) -> Option<u8> {
    let mut score = 2;
    for (i, t) in query.split_whitespace().enumerate() {
        let text = if is_initials_query(t) && !e.key.contains(t) { &e.initials } else { &e.key };
        let at_word = find_token(text, t)?;
        if i == 0 {
            score = if text.starts_with(query) {
                0
            } else if at_word {
                1
            } else {
                2
            };
        }
    }
    Some(score)
}

// 검색어에 맞는 항목 인덱스를 순위대로. 빈 검색어는 결과 없음.
pub fn rank_matches(entries: &[SearchEntry], query: &str) -> Vec<usize> {
    let q = lower(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(u8, usize)> = entries.iter().enumerate().filter_map(|(i, e)| Some((match_score(e, &q)?, i))).collect();
    hits.sort_by(|&(sa, a), &(sb, b)| {
        let (ea, eb) = (&entries[a], &entries[b]);
        sa.cmp(&sb)
            .then(ea.kind.cmp(&eb.kind))
            .then(ea.key.chars().count().cmp(&eb.key.chars().count()))
            .then(ea.key.cmp(&eb.key))
    });
    hits.into_iter().map(|(_, i)| i).collect()
}

// ───────────────────────── 검색창 ─────────────────────────
struct SearchState {
    hwnd: HWND,
    edit: HWND,
    font: HFONT,
    entries: Vec<SearchEntry>,
    results: Vec<usize>,
    sel: usize,
    first: usize, // 화면에 보이는 첫 결과(SEARCH_MAX 행만 보이므로 선택이 밖으로 나가면 민다)
    has_query: bool,
    active: bool, // 한 번이라도 전경이 됐는지. 그 전의 WA_INACTIVE 는 전경 전환 과정의 잡음이라 무시한다
    tries: u8,    // 전경 확보 시도 횟수
    activated_at: u32, // 처음 전경이 된 시각(GetTickCount). 직후의 전경 상실은 닫지 않고 되찾는다
    regrabs: u8,       // 전경을 되찾은 횟수
}
static mut SEARCH: Option<SearchState> = None;
static mut SEARCH_EDIT_OLD: isize = 0;
const ACTIVATE_TRIES: u8 = 10;
const ACTIVATE_RETRY_MS: u32 = 40;
const REGRAB_WINDOW_MS: u32 = 800; // 처음 전경이 된 뒤 이 시간 안의 전경 상실은 되찾는다
const REGRAB_MAX: u8 = 2;

pub unsafe fn toggle_search() {
    if let Some(s) = SEARCH.as_mut() {
        let hwnd = s.hwnd;
        if GetForegroundWindow() == hwnd {
            DestroyWindow(hwnd);
        } else {
            // 떠 있지만 전경이 아니면(다른 창 뒤, 포커스 없음) 닫지 말고 다시 앞으로 가져온다
            s.tries = 0;
            activate_search();
        }
    } else {
        open_search();
    }
}

unsafe fn open_search() {
    let a = app();
    let mut entries: Vec<SearchEntry> =
        scan_desktop(&a.desktop_dirs).iter().map(|p| SearchEntry::new(&p.to_string_lossy())).collect();
    entries.extend(running_windows());
    // 커서가 있는 모니터의 작업 영역 중앙. 결과가 늘어도 위치가 튀지 않게 최대 높이 기준으로 세로 위치를 잡는다.
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    let mut mi: MONITORINFO = core::mem::zeroed();
    mi.cbSize = core::mem::size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST), &mut mi);
    let wk = mi.rcWork;
    let w = m(SEARCH_W).min(wk.right - wk.left - m(32));
    let full_h = m(SEARCH_TOP_H) + SEARCH_MAX as i32 * m(SEARCH_ROW_H) + m(8);
    let x = wk.left + (wk.right - wk.left - w) / 2;
    let y = wk.top + ((wk.bottom - wk.top - full_h) / 2).max(0);
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        wide(CLASS_SEARCH).as_ptr(),
        wide("바탕화면 검색").as_ptr(),
        WS_POPUP | WS_BORDER,
        x,
        y,
        w,
        m(SEARCH_TOP_H),
        a.main,
        null_mut(),
        a.hinst,
        null(),
    );
    let font = CreateFontW(
        -m(20),
        0,
        0,
        0,
        FW_NORMAL as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET as u32,
        OUT_DEFAULT_PRECIS as u32,
        CLIP_DEFAULT_PRECIS as u32,
        CLEARTYPE_QUALITY as u32,
        DEFAULT_PITCH as u32,
        wide("Malgun Gothic").as_ptr(),
    );
    let eh = m(30);
    let edit = CreateWindowExW(
        0,
        wide("EDIT").as_ptr(),
        wide("").as_ptr(),
        WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
        m(14),
        (m(SEARCH_TOP_H) - 2 - eh) / 2,
        w - 2 - m(28),
        eh,
        hwnd,
        null_mut(),
        a.hinst,
        null(),
    );
    SendMessageW(edit, WM_SETFONT, font as usize, 1);
    SendMessageW(edit, EM_SETCUEBANNER, 1, wide("바탕화면 검색 (앱 · 바로 가기 · 파일)").as_ptr() as isize);
    SEARCH_EDIT_OLD =
        SetWindowLongPtrW(edit, GWLP_WNDPROC, search_edit_proc as unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT as usize as isize);
    SEARCH = Some(SearchState {
        hwnd,
        edit,
        font,
        entries,
        results: Vec::new(),
        sel: 0,
        first: 0,
        has_query: false,
        active: false,
        tries: 0,
        activated_at: 0,
        regrabs: 0,
    });
    // SW_SHOW 로 띄우면 활성화를 시도하다 막혔을 때 활성→비활성이 연달아 와서 창이 뜨자마자 닫혔다(깜박임).
    // 활성화 없이 띄운 뒤 전경 확보는 따로 한다.
    update_results(); // 검색어가 없을 때의 초기 목록(실행 중인 창)
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    activate_search();
}

// 검색창을 전경으로 가져오고 입력란에 포커스를 준다. 단축키는 다른 앱이 전경일 때 눌리므로 그냥
// SetForegroundWindow 하면 전경 잠금에 막힌다. (1) 전경 창의 입력 큐에 붙어 넘겨받고, 안 되면 (2) Alt 를
// 눌렀다 떼서 잠금을 풀고 다시 시도한다. 그래도 안 되면 잠시 뒤 타이머로 재시도하고, 끝내 실패하면 창을 닫는다
// (전경이 아닌 채 남겨 두면 다른 창 뒤에서 입력을 못 받는 "유령" 창이 되고, 다음 단축키가 그 창을 닫는 데 쓰여
// 반응이 없는 것처럼 보인다).
unsafe fn activate_search() {
    let Some(s) = SEARCH.as_mut() else { return };
    let (hwnd, edit) = (s.hwnd, s.edit);
    KillTimer(hwnd, TIMER_SEARCH_CLOSE);
    if GetForegroundWindow() != hwnd {
        SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        force_foreground(hwnd);
        if GetForegroundWindow() != hwnd {
            // Alt 를 누른 동안 전경을 바꾸면 잠금이 풀린다. 전경이 우리 창으로 바뀐 뒤 Alt 를 떼므로
            // 원래 전경 앱의 메뉴 막대가 열리지 않는다.
            keybd_event(VK_MENU as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            force_foreground(hwnd);
            keybd_event(VK_MENU as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
        }
        if GetForegroundWindow() != hwnd {
            // 전경이 권한 상승 창(작업 관리자 등)이면 UIPI 때문에 키 주입과 입력 큐 연결이 모두 막힌다.
            // Alt+Tab 이 쓰는 전환은 그 경우에도 동작한다.
            SwitchToThisWindow(hwnd, 1);
        }
    }
    if GetForegroundWindow() == hwnd {
        s.active = true;
        if s.activated_at == 0 {
            s.activated_at = GetTickCount();
        }
        s.tries = 0;
        KillTimer(hwnd, TIMER_SEARCH_ACTIVATE);
        SetFocus(edit);
    } else if s.tries < ACTIVATE_TRIES {
        s.tries += 1;
        SetTimer(hwnd, TIMER_SEARCH_ACTIVATE, ACTIVATE_RETRY_MS, None);
    } else {
        DestroyWindow(hwnd);
    }
}

unsafe fn force_foreground(hwnd: HWND) {
    let fg = GetForegroundWindow();
    let fg_tid = if fg.is_null() { 0 } else { GetWindowThreadProcessId(fg, null_mut()) };
    let me = GetCurrentThreadId();
    let attach = fg_tid != 0 && fg_tid != me;
    if attach {
        AttachThreadInput(me, fg_tid, 1);
    }
    SetForegroundWindow(hwnd);
    BringWindowToTop(hwnd);
    if attach {
        AttachThreadInput(me, fg_tid, 0);
    }
}

unsafe fn update_results() {
    let Some(s) = SEARCH.as_mut() else { return };
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(s.edit, buf.as_mut_ptr(), buf.len() as i32);
    let q = from_wide(&buf[..n as usize]);
    s.has_query = !q.trim().is_empty();
    // 검색어가 없으면 실행 중인 창 목록(Z 순서: 방금 쓰던 창이 먼저)을 보여 방향키로 고를 수 있게 한다
    s.results = if s.has_query {
        rank_matches(&s.entries, &q)
    } else {
        s.entries.iter().enumerate().filter(|(_, e)| e.is_window()).map(|(i, _)| i).collect()
    };
    s.sel = 0;
    s.first = 0;
    // 결과가 없으면 "결과 없음" 한 줄
    let rows = if s.results.is_empty() { s.has_query as i32 } else { s.results.len().min(SEARCH_MAX) as i32 };
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(s.hwnd, &mut r);
    let h = m(SEARCH_TOP_H) + rows * m(SEARCH_ROW_H) + if rows > 0 { m(8) } else { 0 };
    SetWindowPos(s.hwnd, null_mut(), 0, 0, r.right - r.left, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    InvalidateRect(s.hwnd, null(), 0);
}

unsafe fn move_selection(delta: i32) {
    let Some(s) = SEARCH.as_mut() else { return };
    if s.results.is_empty() {
        return;
    }
    let n = s.results.len() as i32;
    s.sel = (s.sel as i32 + delta).rem_euclid(n) as usize;
    // 선택이 보이는 범위를 벗어나면 그쪽으로 민다
    if s.sel < s.first {
        s.first = s.sel;
    } else if s.sel >= s.first + SEARCH_MAX {
        s.first = s.sel + 1 - SEARCH_MAX;
    }
    InvalidateRect(s.hwnd, null(), 0);
}

// 선택 항목(기본 첫 번째) 실행 후 검색창 닫기
unsafe fn run_selected() {
    let Some(s) = SEARCH.as_ref() else { return };
    let Some(&i) = s.results.get(s.sel) else { return };
    let e = &s.entries[i];
    let (hwnd, target, path) = (s.hwnd, e.window, e.item.path.clone());
    if !target.is_null() {
        switch_to_window(target);
        DestroyWindow(hwnd);
    } else {
        DestroyWindow(hwnd);
        open_path(&path);
    }
}

unsafe fn row_at(y: i32) -> Option<usize> {
    let s = SEARCH.as_ref()?;
    let top = m(SEARCH_TOP_H) + m(4);
    if y < top {
        return None;
    }
    let r = s.first + ((y - top) / m(SEARCH_ROW_H)) as usize;
    (r < s.results.len() && r < s.first + SEARCH_MAX).then_some(r)
}

unsafe fn paint_search(hwnd: HWND) {
    let mut ps: PAINTSTRUCT = core::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(hwnd, &mut rc);
    let (w, h) = (rc.right, rc.bottom);
    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem, bmp as HGDIOBJ);
    let bg = CreateSolidBrush(0xFFFFFF);
    FillRect(mem, &rc, bg);
    DeleteObject(bg as HGDIOBJ);
    if let Some(s) = SEARCH.as_mut() {
        let top = m(SEARCH_TOP_H) - 2;
        if s.has_query || !s.results.is_empty() {
            let line = CreateSolidBrush(0xE5E5E5);
            FillRect(mem, &RECT { left: 0, top, right: w, bottom: top + 1 }, line);
            DeleteObject(line as HGDIOBJ);
        }
        let old_font = SelectObject(mem, app().font_b as HGDIOBJ);
        SetBkMode(mem, TRANSPARENT as i32);
        let icon = m(24);
        let row_top = |r: i32| top + m(4) + r * m(SEARCH_ROW_H);
        let first = s.first;
        for (r, &ei) in s.results.iter().enumerate().skip(first).take(SEARCH_MAX) {
            let y = row_top((r - first) as i32);
            if r == s.sel {
                let br = CreateSolidBrush(0xF7E8D9); // 연한 파랑 (BGR)
                FillRect(mem, &RECT { left: m(4), top: y, right: w - m(4), bottom: y + m(SEARCH_ROW_H) }, br);
                DeleteObject(br as HGDIOBJ);
            }
            let is_win = s.entries[ei].is_window();
            let it = &mut s.entries[ei].item;
            if it.icon.is_null() {
                it.icon = load_icon(&it.path, 32);
            }
            if !it.icon.is_null() {
                DrawIconEx(mem, m(14), y + (m(SEARCH_ROW_H) - icon) / 2, it.icon, icon, icon, 0, null_mut(), DI_NORMAL);
            }
            SetTextColor(mem, 0x202020);
            let mut tr = RECT { left: m(14) + icon + m(12), top: y, right: w - m(14), bottom: y + m(SEARCH_ROW_H) };
            if is_win {
                // 오른쪽에 "실행 중" 표시를 두고 이름 영역은 그만큼 줄인다
                let tag = wide("실행 중");
                let mut tag_r = RECT { left: tr.left, top: y, right: w - m(14), bottom: y + m(SEARCH_ROW_H) };
                SetTextColor(mem, 0x909090);
                DrawTextW(mem, tag.as_ptr(), -1, &mut tag_r, DT_SINGLELINE | DT_VCENTER | DT_RIGHT | DT_NOPREFIX);
                tr.right -= m(60);
                SetTextColor(mem, 0x202020);
            }
            let name = wide(&it.name);
            DrawTextW(mem, name.as_ptr(), -1, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
        }
        if s.results.is_empty() && s.has_query {
            let y = row_top(0);
            SetTextColor(mem, 0x909090);
            let mut tr = RECT { left: m(14), top: y, right: w - m(14), bottom: y + m(SEARCH_ROW_H) };
            let t = wide("결과 없음");
            DrawTextW(mem, t.as_ptr(), -1, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
        }
        SelectObject(mem, old_font);
    }
    BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old_bmp);
    DeleteObject(bmp as HGDIOBJ);
    DeleteDC(mem);
    EndPaint(hwnd, &ps);
}

pub unsafe extern "system" fn search_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND if (wp >> 16) as u32 == EN_CHANGE => {
            update_results();
            return 0;
        }
        WM_PAINT => {
            paint_search(hwnd);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_CTLCOLOREDIT => return GetStockObject(WHITE_BRUSH) as LRESULT,
        WM_LBUTTONDOWN => {
            if let Some(r) = row_at(((lp >> 16) & 0xFFFF) as i16 as i32) {
                if let Some(s) = SEARCH.as_mut() {
                    s.sel = r;
                }
                run_selected();
            }
            return 0;
        }
        // 다른 곳을 클릭하는 등 비활성화되면 닫는다. 단, 전경이 된 적이 없을 때(전경 전환 중)의 비활성화는
        // 무시하고, 활성화 뒤에도 곧바로 닫지 않고 잠시 뒤 전경을 다시 확인한다(전환 과정에서 활성→비활성→활성이
        // 연달아 오면 창이 깜박이며 사라졌다).
        WM_ACTIVATE => {
            let s = SEARCH.as_mut();
            if (wp & 0xFFFF) as u32 == WA_INACTIVE {
                if s.is_some_and(|s| s.active) {
                    SetTimer(hwnd, TIMER_SEARCH_CLOSE, 80, None);
                }
            } else {
                KillTimer(hwnd, TIMER_SEARCH_CLOSE);
                if let Some(s) = s {
                    s.active = true;
                    if s.activated_at == 0 {
                        s.activated_at = GetTickCount();
                    }
                    SetFocus(s.edit);
                }
            }
            return 0;
        }
        WM_SETFOCUS => {
            if let Some(s) = SEARCH.as_ref() {
                SetFocus(s.edit);
            }
            return 0;
        }
        WM_TIMER => match wp {
            TIMER_SEARCH_ACTIVATE => {
                activate_search();
                return 0;
            }
            TIMER_SEARCH_CLOSE => {
                KillTimer(hwnd, TIMER_SEARCH_CLOSE);
                if GetForegroundWindow() != hwnd {
                    // 열린 직후의 전경 상실은 사용자가 아니라 전환 과정의 잡음이다(시작 메뉴 닫힘, SwitchToThisWindow 가 띄우는
                    // Alt+Tab 전환기 등). 그 경우 닫지 않고 전경을 되찾는다.
                    let regrab = SEARCH.as_mut().filter(|s| {
                        GetTickCount().wrapping_sub(s.activated_at) < REGRAB_WINDOW_MS && s.regrabs < REGRAB_MAX
                    });
                    if let Some(s) = regrab {
                        s.regrabs += 1;
                        s.tries = 0;
                        activate_search();
                    } else {
                        DestroyWindow(hwnd);
                    }
                }
                return 0;
            }
            _ => {}
        },
        WM_DESTROY => {
            if SEARCH.as_ref().is_some_and(|s| s.hwnd == hwnd) {
                if let Some(s) = SEARCH.take() {
                    DeleteObject(s.font as HGDIOBJ);
                }
            }
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe extern "system" fn search_edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_KEYDOWN => match wp as u16 {
            VK_RETURN => {
                run_selected();
                return 0;
            }
            VK_ESCAPE => {
                DestroyWindow(GetParent(hwnd));
                return 0;
            }
            VK_DOWN => {
                move_selection(1);
                return 0;
            }
            VK_UP => {
                move_selection(-1);
                return 0;
            }
            _ => {}
        },
        WM_CHAR if wp as u16 == VK_RETURN || wp as u16 == VK_ESCAPE => return 0,
        _ => {}
    }
    let old: WNDPROC = core::mem::transmute(SEARCH_EDIT_OLD);
    CallWindowProcW(old, hwnd, msg, wp, lp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(paths: &[&str]) -> Vec<SearchEntry> {
        paths.iter().map(|p| SearchEntry::new(p)).collect()
    }
    fn ranked(es: &[SearchEntry], q: &str) -> Vec<String> {
        rank_matches(es, q).into_iter().map(|i| es[i].item.name.clone()).collect()
    }

    #[test]
    fn should_return_nothing_when_query_is_blank() {
        let es = entries(&[r"C:\d\Chrome.lnk"]);
        assert!(rank_matches(&es, "").is_empty());
        assert!(rank_matches(&es, "   ").is_empty());
    }

    #[test]
    fn should_match_case_insensitively_and_exclude_non_matches() {
        let es = entries(&[r"C:\d\Google Chrome.lnk", r"C:\d\메모.txt"]);
        assert_eq!(ranked(&es, "CHROME"), ["Google Chrome"]);
    }

    #[test]
    fn should_rank_prefix_then_word_start_then_substring() {
        let es = entries(&[r"C:\d\Xcodeview.lnk", r"C:\d\Visual Code.lnk", r"C:\d\Code Runner.lnk"]);
        assert_eq!(ranked(&es, "code"), ["Code Runner", "Visual Code", "Xcodeview"]);
    }

    #[test]
    fn should_put_apps_before_files_when_scores_tie() {
        let es = entries(&[r"C:\d\report.txt", r"C:\d\report.lnk"]);
        assert_eq!(ranked(&es, "report"), ["report", "report.txt"]);
    }

    #[test]
    fn should_append_program_name_unless_title_already_ends_with_it() {
        assert_eq!(window_display_name("작업장", "Visual Studio"), "작업장 - Visual Studio");
        assert_eq!(window_display_name("문서 - Microsoft Word", "Microsoft Word"), "문서 - Microsoft Word");
        assert_eq!(window_display_name("  제목  ", ""), "제목");
    }

    #[test]
    fn should_read_program_name_from_version_info_of_real_exe() {
        let exe = format!("{}\\explorer.exe", std::env::var("SystemRoot").unwrap());
        let name = unsafe { program_name(&exe) };
        assert!(!name.is_empty() && name != "explorer", "FileDescription 을 읽어야 한다: {name}");
        assert_eq!(unsafe { program_name(r"C:\없는폴더\없는파일.exe") }, "없는파일");
    }

    #[test]
    fn should_find_window_by_program_name() {
        let mut win = Item::new("");
        win.name = window_display_name("작업장", "Visual Studio");
        let es = vec![SearchEntry::build(win, KIND_WINDOW, 1 as HWND)];
        assert_eq!(ranked(&es, "visual"), ["작업장 - Visual Studio"]);
        assert_eq!(ranked(&es, "ㅈㅇㅈ"), ["작업장 - Visual Studio"]);
        assert!(ranked(&es, "word").is_empty());
    }

    #[test]
    fn should_rank_running_window_after_app_and_before_file() {
        let mut es = entries(&[r"C:\d\report.txt", r"C:\d\report.lnk"]);
        let mut win = Item::new("");
        win.name = "report".to_string();
        es.push(SearchEntry::build(win, KIND_WINDOW, 1 as HWND));
        let order: Vec<u8> = rank_matches(&es, "report").into_iter().map(|i| es[i].kind).collect();
        assert_eq!(order, [KIND_APP, KIND_WINDOW, KIND_FILE]);
    }

    #[test]
    fn should_prefer_shorter_name_when_score_and_kind_tie() {
        let es = entries(&[r"C:\d\Notepad++.lnk", r"C:\d\Notepad.lnk"]);
        assert_eq!(ranked(&es, "note"), ["Notepad", "Notepad++"]);
    }

    #[test]
    fn should_require_every_word_of_query() {
        let es = entries(&[r"C:\d\Visual Studio Code.lnk", r"C:\d\Visual Studio 2022.lnk"]);
        assert_eq!(ranked(&es, "vis code"), ["Visual Studio Code"]);
    }

    #[test]
    fn should_match_korean_by_initial_consonants() {
        let es = entries(&[r"C:\d\카카오톡.lnk", r"C:\d\계산기.lnk"]);
        assert_eq!(ranked(&es, "ㅋㅋㅇ"), ["카카오톡"]);
        assert_eq!(ranked(&es, "계산"), ["계산기"]);
    }

    #[test]
    fn should_match_file_extension_of_regular_files() {
        let es = entries(&[r"C:\d\견적서.pdf", r"C:\d\견적서.lnk"]);
        assert_eq!(ranked(&es, "pdf"), ["견적서.pdf"]);
    }
}
