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
- 설정 파일: `%APPDATA%\IconDocs\layout.txt` (단순 key=value 텍스트), 스냅샷은 `snapshots\*.txt`, 패닉 기록은 `crash.log`.
  `layout.txt` 를 삭제하면 다음 실행 때 자동 정리를 다시 수행한다.
- 실행 중인 exe 는 덮어쓸 수 없으므로 다시 빌드하기 전에 앱을 정상 종료해야 한다(아래 WM_CLOSE 참고).

## 테스트
- 단위 테스트: `cargo test --release` (단일 테스트는 `cargo test --release <이름 일부>`). 현재는 `portal.rs` 의 폴더 정렬 비교(`compare_entries`, `SortKey`)만 있다.
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
- 강제 종료(taskkill /F)하면 아이콘이 숨겨진 채 남는다. 테스트 후에는 메인 창(`IconDocsMain` 클래스)에 WM_CLOSE 를 보내 정상 종료할 것.

## 아키텍처 (여러 파일에 걸친 흐름)
- **전역 상태**: 모든 상태는 `static mut APP: *mut App` 하나에 있고 `app()` 으로 꺼낸다. 단일 UI 스레드라 잠금은 없다.
  `main.rs` 가 모든 모듈을 `pub use x::*` 로 재노출하고 각 모듈은 `use crate::*` 를 쓰므로, 함수·상수 이름은 crate 전체에서 겹치면 안 된다.
- **Fence 식별**: Fence 는 `app().fences: Vec<Fence>` 의 인덱스로 다룬다. 창 프로시저는 `fence_idx(hwnd)` 로 인덱스를 찾는다.
  `delete_fence` 는 뒤 인덱스를 당기므로 `focused` 처럼 인덱스를 들고 있는 상태를 함께 보정해야 한다.
- **훅 → 메인 창**: 저수준 훅(`input.rs`)은 상태를 직접 바꾸지 않고 `WM_APP+n` 메시지(`WM_DESKTOP_DBLCLK`, `WM_BAND_*`, `WM_KEY`,
  `WM_PEEK_CLICK`, `WM_HOTCORNER`)를 메인 창에 post 한다. 실제 처리는 `main_proc` 에서 한다.
- **저장**: 상태를 바꾸는 동작은 끝에 `save()` 를 호출해 `layout.txt` 전체를 다시 쓴다. 스냅샷도 같은 `serialize()`/`parse_into()` 를 쓰며,
  `apply_layout` 은 모든 Fence 창을 없애고 다시 만든다.
- **재검색 루프**: 메인 창 타이머(`TIMER_RESCAN`, 3초)가 `rescan()` 을 부른다. 사라진 파일은 제거하고, 어느 Fence 에도 없는 새 파일은
  첫 매칭 규칙(`Rule.target` = Fence 제목)으로, 없으면 "새 항목" Fence 로 보낸다. 대상 제목의 Fence 가 없으면 새로 만든다.
  explorer 가 재시작되어 `defview` 가 무효해지면 `reattach()` 로 모든 Fence 창을 다시 만든다.
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
- 제목 영역은 `WM_NCHITTEST` 가 HTCAPTION 을 돌려주므로 마우스 이동은 `WM_NCMOUSEMOVE` 로 온다(hover 펼치기 처리 시 주의).
- 48px 아이콘은 windows-sys 에 COM 인터페이스가 없어 `IImageList` vtable 을 수동 선언해 `GetIcon` 을 호출한다(`fence.rs`).
- 크기·좌표 상수는 96 DPI 기준이며 `m()` 으로 현재 DPI 에 맞춰 환산해서 쓴다.
