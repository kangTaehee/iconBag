# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 프로젝트
`icondocs`(아이콘독스)는 Stardock Fences 스타일의 바탕화면 아이콘 정리 도구다. Windows 전용이며, Rust 와 `windows-sys`(Win32 API 직접 호출)만 쓰고
런타임이나 프레임워크는 없다. 사용자 기능 목록은 `README.md` 에 있다.

| 파일 | 내용 |
| --- | --- |
| `src/main.rs` | 상수·메뉴 ID·`WM_APP` 메시지, `App` 전역 상태, 트레이 메뉴, 메인(숨김) 창 프로시저, 진입점, 패닉 훅 |
| `src/fence.rs` | `Item`/`Fence`, 창 생성, 그리기, Fence/아이콘 메뉴, 텍스트 입력 팝업, `fence_proc`, 항목 드래그, 탐색기 드롭 |
| `src/desktop.rs` | explorer 창 탐색, 바탕화면 스캔, `rescan`(규칙 분류), 아이콘 숨김, 자동 정리 |
| `src/config.rs` | `layout.txt` 직렬화/파싱, 규칙(`Rule`), 시작 프로그램 등록(HKCU Run) |
| `src/input.rs` | 저수준 마우스/키보드 훅, 전역 핫키, 핫코너, 키 처리 |
| `src/peek.rs` | 피크(바탕화면 자식 ↔ 최상위 팝업 전환) |
| `src/portal.rs` | 폴더 포털(폴더 스캔·탐색·드롭 이동·폴더 선택) |
| `src/snapshot.rs` | 레이아웃 스냅샷 저장/복원, 해상도 변경 대응 |
| `src/search.rs` | 바탕화면 검색창(Win+Space): 검색 순위(`rank_matches`, 한글 초성 매칭), 팝업 창·입력·실행 |
| `src/util.rs` | 문자열/색/glob/핫키 파싱 유틸 |

## 빌드/실행
```
cargo build --release           # target/release/icondocs.exe (~390KB)
cargo clippy --release          # 린트 (main.rs 에서 일부 lint 를 crate 단위로 allow)
./target/release/icondocs.exe   # 트레이 아이콘으로 실행됨, 이미 실행 중이면 바로 종료
```
- 툴체인: `rust-toolchain.toml` 이 `stable-x86_64-pc-windows-gnu` 를 고정한다. 이 PC 에는 MSVC 빌드 도구/Windows SDK 가 없어
  rustup 에 번들된 MinGW 링커를 쓰기 위함이다. (`rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`)
- release 프로필은 `opt-level="z"`, LTO, `panic="abort"`, `strip` 이다. 배포용 실행 파일은 `dist/icondocs.exe` 로 커밋되어 있다.
- `build.rs` 가 빌드 전용 크레이트 `embed-manifest` 로 실행 파일 매니페스트(supportedOS Win7~11, PerMonitorV2)를 넣는다.
  이 PC 의 GNU 툴체인에는 windres/rc 가 없어서 리소스 컴파일러 없이 COFF 오브젝트를 만드는 이 크레이트를 쓴다.
- 설정 파일: `%APPDATA%\IconDocs\layout.txt` (단순 key=value 텍스트), 스냅샷은 `snapshots\*.txt`, 패닉 기록은 `crash.log`.
  `layout.txt` 를 삭제하면 다음 실행 때 자동 정리를 다시 수행한다.
- 실행 중인 exe 는 덮어쓸 수 없으므로 다시 빌드하기 전에 앱을 정상 종료해야 한다(아래 WM_CLOSE 참고).

## 테스트
- 단위 테스트: `cargo test --release` (단일 테스트는 `cargo test --release <이름 일부>`). `portal.rs` 의 폴더 정렬 비교(`compare_entries`, `SortKey`), `desktop.rs` 의 `is_on_desktop`·`unique_path`·`create_shortcut`(실제 .lnk 생성),
  `search.rs` 의 `rank_matches`, `util.rs` 의 `parse_hotkey`/`hotkey_to_string` 테스트가 있다.
  `util.rs` 의 `glob_match`/`parse_hotkey`, `config.rs` 의 `rule_matches`, `snapshot.rs` 의 `sanitize` 도 순수 함수라 테스트하기 쉽다.
  `parse_into`·`m()` 처럼 전역 `APP` 을 쓰는 함수는 테스트에서 `APP` 을 먼저 초기화해야 한다.
- 통합 확인은 실제 바탕화면에서 하며, 다음 함정이 있다.
  - **샌드박스 밖에서 실행**해야 한다. 샌드박스(job 객체) 안에서 띄운 앱은 Fence 를 만들고 부착까지 하지만 화면에는 아무것도 그려지지 않는다.
  - PowerShell P/Invoke 에서 문자열 인자에 `$null` 을 넘기면 빈 문자열로 마샬링되어 `FindWindowW("Progman", $null)` 이 0 을 돌려준다. `[NullString]::Value` 를 쓸 것.
  - 사용자가 PC 를 쓰는 중일 수 있다. `keybd_event` 단축키는 사용자 창에 입력될 수 있고, 피크는 클릭 한 번에 끝나 단계가 어긋난다.
    단축키 대신 메인 창(`IconDocsMain`)에 `WM_HOTKEY`(wParam 1=숨김 토글, 2=피크)를 PostMessage 하고, 단계마다 Fence 의 `GetParent` 로 상태를 확인한다.
  - 바탕화면에 실제로 합성됐는지는 **Progman 창을 `PrintWindow(…, PW_RENDERFULLCONTENT)`** 로 받으면 다른 창에 가려져도 확인된다.
    Fence HWND 단독 `PrintWindow` 는 검게 나와 판단에 쓸 수 없다.
  - 실행 중인 exe 는 빌드 산출물을 잠그므로 `target/release/icondocs.exe` 를 다른 곳에 복사해 실행한다.
  - 파일이 수천 개인 폴더(예: System32) 포털은 성능 측정용이다. 측정이 끝나면 `layout.txt` 에서 지울 것.
  - 테스트용 Fence 는 `layout.txt` 에 제목을 붙여 추가하고(예: `title=XTEST`) 끝나면 그 블록만 지운다. 사용자 Fence 는 건드리지 않는다.
  - `IconDocsMain` 에 WM_CLOSE 를 보내면 **사용자가 쓰던 인스턴스도 닫힌다**(중복 실행 방지로 하나뿐). 테스트 후 다시 띄워 줄 것.
  - 투명도는 Progman `PrintWindow` 에서 Fence 영역 평균색을 alpha 255/128 로 비교하면 확인된다(같으면 투명도 무시).
  - 포털 동작은 Fence 에 `WM_LBUTTONDBLCLK`(항목 좌표) / `WM_XBUTTONUP`(wParam 상위 워드 1) 을 PostMessage 해 확인한다.
  - 핫코너는 **가상 화면** 모서리 기준이다. 보조 모니터가 있으면 `mouse_event` 에 `MOUSEEVENTF_VIRTUALDESK` 를 붙여야 그 모서리로 간다.
- 강제 종료(taskkill /F)하면 아이콘이 숨겨진 채 남는다. 테스트 후에는 메인 창(`IconDocsMain` 클래스)에 WM_CLOSE 를 보내 정상 종료할 것.

## 아키텍처 (여러 파일에 걸친 흐름)
- **전역 상태**: 모든 상태는 `static mut APP: *mut App` 하나에 있고 `app()` 으로 꺼낸다. 단일 UI 스레드라 잠금은 없다.
  `main.rs` 가 모든 모듈을 `pub use x::*` 로 재노출하고 각 모듈은 `use crate::*` 를 쓰므로, 함수·상수 이름은 crate 전체에서 겹치면 안 된다.
- **Fence 식별**: Fence 는 `app().fences: Vec<Fence>` 의 인덱스로 다룬다. 창 프로시저는 `fence_idx(hwnd)` 로 인덱스를 찾는다.
  `delete_fence` 는 뒤 인덱스를 당기므로 `focused` 처럼 인덱스를 들고 있는 상태를 함께 보정해야 한다.
- **훅 스레드 → 메인 창**: 저수준 훅(`input.rs`)은 `start_hook_thread()` 가 만든 **전용 스레드**에서 돈다. 훅은 `App` 을 읽지 않고
  원자 변수(피크·포커스·밴드 드래그·핫코너)만 읽으며, `WM_APP+n` 메시지(`WM_DESKTOP_DBLCLK`, `WM_BAND_*`, `WM_KEY`,
  `WM_PEEK_CLICK`, `WM_HOTCORNER`, `WM_SEARCH`)를 메인 창에 post 한다. 실제 처리는 `main_proc` 에서 한다.
  그래서 `peeking`·`focused`·`band_start`·`hot_corner`·`hotkey_search` 는 직접 대입하지 말고
  `set_peeking`/`set_focused`/`set_band_start`/`set_hot_corner`/`set_search_hotkey` 로 바꾼다.
- **저장**: 상태를 바꾸는 동작은 끝에 `save()` 를 호출해 `layout.txt` 전체를 다시 쓴다. 스냅샷도 같은 `serialize()`/`parse_into()` 를 쓰며,
  `apply_layout` 은 모든 Fence 창을 없애고 다시 만든다.
- **재검색 루프**: 메인 창 타이머(`TIMER_RESCAN`, 3초)가 `rescan()` 을 부른다. 사라진 파일은 제거하고, 어느 Fence 에도 없는 새 파일은
  첫 매칭 규칙(`Rule.target` = Fence 제목)으로, 없으면 "새 항목" Fence 로 보낸다. 대상 제목의 Fence 가 없으면 새로 만든다.
  explorer 가 재시작되어 `defview` 가 무효해지면 `reattach()` 로 모든 Fence 창을 다시 만든다.
  재검색은 바탕화면 폴더(사용자/공용) 바로 아래만 보므로, 그 밖의 파일을 Fence 항목으로 넣으면 3초 뒤 지워진다.
  탐색기 드롭(`WM_DROPFILES`)은 `desktop_entry_for` 로 사용자 바탕화면에 바로 가기(.lnk, `IShellLinkW` 수동 vtable)를 만들어 그 경로를 넣는다.
- **일반 Fence vs 포털**: `Fence.portal` 이 `Some` 이면 항목은 그 폴더(`nav` 스택의 마지막)에서 매번 스캔하며 저장되지 않는다.
  정렬·숨김 파일 표시는 `Fence.view`(`FolderView`)가 정하고 새로 고칠 때마다 다시 적용된다.
  규칙 분류·`rescan`·Fence 간 드래그는 포털을 제외한다. 포털로 드롭하면 파일을 실제로 이동한다(`MoveFileExW`).
- **자동 높이**: `Fence.auto_h` 가 켜져 있으면 `invalidate()` 가 `apply_auto_height()` 를 불러 높이를 내용에 맞춘다.
  항목 수를 바꾸는 코드는 `InvalidateRect` 를 직접 부르지 말고 `invalidate(idx)` 를 거쳐야 높이가 따라간다.
- **창 생성**: `create_fence_window` 는 팝업으로 만들고 `attach_to_desktop` 으로 `SHELLDLL_DefView` 의 자식으로 붙인다(아래 함정 참고).

## 핵심 동작 (Windows 11 에서 검증된 함정)
- Fence 창은 explorer 의 `SHELLDLL_DefView` 의 자식으로 붙여 바탕화면 레이어에 둔다. 다른 프로세스 창을 부모로 지정해
  `CreateWindowExW` 하면 실패하므로, **팝업으로 만든 뒤 `SetParent`** 한다.
- 바탕화면 원본 아이콘(`SysListView32`) 숨기기는 **`ShowWindowAsync` 만 동작**한다. 동기 `ShowWindow` 와
  `WM_COMMAND 0x7402` 토글은 Windows 11 26200 에서 무시된다. 종료(WM_DESTROY)와 패닉 훅에서 반드시 다시 표시한다.
- 바탕화면 빈 곳 더블클릭으로 Fence 숨김/표시: `WH_MOUSE_LL` 훅으로 더블클릭을 감지해 메인 창에 post 한다.
- 키보드: Fence 는 explorer 자식 창이라 WM_KEYDOWN 이 오지 않는다. `WH_KEYBOARD_LL` 훅으로 받되 **전경 창이 Progman/WorkerW 일 때만** 처리한다. Fence 클릭 시 `SetForegroundWindow(Progman)` 으로 바탕화면을 전경으로 만든다.
- 피크: 자식 창은 다른 창 위로 못 올라가므로 `SetParent(NULL)`+WS_POPUP+HWND_TOPMOST 로 떼었다가 `attach_to_desktop` 으로 되돌린다. TOPMOST 해제는 반드시 `SetWindowPos(HWND_NOTOPMOST)` 이며, WS_CHILD 로 바꾸기 **전에** 해야 한다.
- **레이어드 창의 부모를 바꾸면 화면에서 사라진다**: 최상위 ↔ 자식으로 `SetParent` 하면 DWM 합성 표면이 끊긴다.
  부모·스타일·Z 순서·`IsWindowVisible`·`GetLayeredWindowAttributes` 가 모두 정상인데도 안 그려지므로 상태 조회로는 찾을 수 없다.
  부모를 바꾼 뒤에는 `reset_layered()`(WS_EX_LAYERED 끄고 켜기 + 알파 재지정)를 반드시 부른다. 피크 종료 후 Fence 가 사라지던 원인이다.
- **UI 스레드에서 파일 시스템을 반복 조회하지 말 것**: 3초 재검색이 UI 스레드에서 돈다. 정렬 키에서 `Path::is_dir()` 를 부르면 비교마다 stat 이 일어나
  파일 5000개 폴더에서 UI 가 4.5초씩 멈췄다. 메타데이터는 `scan_entries()`(DirEntry 재사용)로 한 번만 얻는다.
- 아이콘(`SHGetFileInfoW`)은 그리기 중에 처음 읽는다. 한 번 그릴 때 `ICON_BUDGET_MS` 만 쓰고 나머지는 `TIMER_ICONS` 로 이어 읽는다.
  이 예산이 없으면 처음 크기를 조절하거나 스크롤할 때 수 초씩 멈춘다.
- **매니페스트가 없으면 Fence 투명도가 무시된다**: 레이어드 "자식" 창은 Windows 8+ 호환을 선언한 앱에서만 동작한다.
  선언이 없으면 `SetLayeredWindowAttributes` 가 성공하고 `GetLayeredWindowAttributes` 도 설정값을 돌려주지만 화면은 불투명하다.
- **저수준 훅을 UI 스레드에 두지 말 것**: `WH_MOUSE_LL`/`WH_KEYBOARD_LL` 은 설치한 스레드에서 실행되므로, 그 스레드가 아이콘 로드 등으로 바쁘면
  **시스템 전체** 마우스·키 입력이 밀린다(초기 실행 버벅임의 원인). 훅은 전용 스레드에 두고 UI 상태는 원자 변수로 넘긴다.
  훅 스레드에서는 `GetKeyState` 대신 `GetAsyncKeyState` 를 쓴다(스레드별 키 상태라 값이 틀림).
- **검색 단축키 Win+Space 는 `RegisterHotKey` 가 아니라 키보드 훅에서 가로챈다**: 시스템(입력 언어 전환)이 쓰는 조합이다.
  키를 삼킨 뒤 Win 을 떼면 "Win 단독 입력"으로 보여 시작 메뉴가 열리므로, 훅에서 할당 안 된 키(0xE8)를 주입해 막는다.
  다른 앱이 전경일 때 뜨는 창이라 `SetForegroundWindow` 만으로는 전경 잠금에 걸릴 수 있어 `AttachThreadInput` 으로 넘겨받고,
  그래도 안 되면 Alt 를 누른 채 다시 시도하고, 전경이 권한 상승 창(작업 관리자 등)이라 키 주입·입력 큐 연결이 UIPI 로 막히면
  `SwitchToThisWindow` 로 전환한 뒤 타이머로 재시도한다(`activate_search`). 끝내 실패하면 창을 닫는다.
  전경이 아닌 채 남은 창은 입력을 못 받는 유령이 되고, 다음 단축키가 그 창을 닫는 데 쓰여 "반응 없음"처럼 보이기 때문이다.
- **훅 안에서 `GetAsyncKeyState` 로 수정키를 읽으면 늦는다**: 훅이 Space 를 볼 때 바로 앞서 눌린 Win 이 아직 반영되지 않아
  빠르게 누르면 단축키를 놓쳤다. 훅이 본 수정키 눌림/뗌을 `HOOK_MODS` 에 직접 기록해 비동기 상태와 OR 한다.
  뗌은 무조건 지운다(뗌 시점에 비동기 상태로 판단하면 같은 지연으로 비트가 남아 평범한 Space 가 단축키로 오인된다).
- **단축키로 띄우는 팝업은 `SW_SHOW` 로 띄우면서 활성화하지 말 것**: 전경 잠금에 막히면 활성→비활성이 연달아 와서
  "비활성화되면 닫기" 로직이 창을 뜨자마자 닫는다(깜박임). `SW_SHOWNOACTIVATE` 로 띄운 뒤 전경을 따로 확보하고,
  비활성화 시 닫기는 한 번이라도 전경이 된 뒤에만, 그것도 즉시가 아니라 잠시 뒤 `GetForegroundWindow` 로 재확인해서 닫는다.
  처음 전경이 된 뒤 `REGRAB_WINDOW_MS`(0.8초) 안에 전경을 잃으면 사용자가 아니라 전환 과정의 잡음(시작 메뉴 닫힘, 전환기 창 등)이므로
  닫지 않고 `activate_search` 로 되찾는다(최대 `REGRAB_MAX` 회). 시작 메뉴가 떠 있을 때 Win+Space 로 열린 창이 바로 꺼지던 원인이다.
- 마우스 뒤로 가기(XBUTTON1)는 클라이언트에서 `WM_XBUTTONUP`, 제목(HTCAPTION) 위에서 `WM_NCXBUTTONUP` 으로 온다. 처리했으면 TRUE 를 돌려
  `WM_APPCOMMAND` 가 부모(explorer)로 올라가지 않게 한다.
- 제목 영역은 `WM_NCHITTEST` 가 HTCAPTION 을 돌려주므로 마우스 이동은 `WM_NCMOUSEMOVE` 로 온다(hover 펼치기 처리 시 주의).
- 48px 아이콘은 windows-sys 에 COM 인터페이스가 없어 `IImageList` vtable 을 수동 선언해 `GetIcon` 을 호출한다(`fence.rs`).
- 크기·좌표 상수는 96 DPI 기준이며 `m()` 으로 현재 DPI 에 맞춰 환산해서 쓴다.
