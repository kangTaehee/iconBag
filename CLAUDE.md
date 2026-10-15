# CLAUDE.md

## 프로젝트
`icondocs`(아이콘독스) — Stardock Fences 스타일의 바탕화면 아이콘 정리 도구. Rust + `windows-sys`(Win32 API 직접 호출)만 사용하며
런타임/프레임워크가 없다.

| 파일 | 내용 |
| --- | --- |
| `src/main.rs` | 상수·메뉴 ID, `App` 전역 상태, 트레이 메뉴, 메인(숨김) 창 프로시저, 진입점 |
| `src/fence.rs` | `Item`/`Fence`, 창 생성, 그리기, Fence/아이콘 메뉴, 텍스트 입력 팝업, `fence_proc`, 항목 드래그 |
| `src/desktop.rs` | explorer 창 탐색, 바탕화면 스캔, `rescan`(규칙 분류), 아이콘 숨김, 자동 정리 |
| `src/config.rs` | `layout.txt` 직렬화/파싱, 규칙(`Rule`), 시작 프로그램 등록 |
| `src/input.rs` | 저수준 마우스/키보드 훅, 전역 핫키, 핫코너, 키 처리 |
| `src/peek.rs` | 피크(바탕화면 자식 ↔ 최상위 팝업 전환) |
| `src/portal.rs` | 폴더 포털(폴더 스캔·탐색·드롭 이동·폴더 선택) |
| `src/snapshot.rs` | 레이아웃 스냅샷 저장/복원, 해상도 변경 대응 |
| `src/util.rs` | 문자열/색/glob/핫키 파싱 유틸 |

## 빌드/실행
```
cargo build --release          # target/release/icondocs.exe (~320KB)
./target/release/icondocs.exe   # 트레이 아이콘으로 실행됨
```
- 툴체인: `rust-toolchain.toml` 이 `stable-x86_64-pc-windows-gnu` 를 고정한다. 이 PC 에는 MSVC 빌드 도구/Windows SDK 가 없어
  rustup 에 번들된 MinGW 링커를 쓰기 위함이다. (`rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`)
- 설정 파일: `%APPDATA%\IconDocs\layout.txt` (단순 key=value 텍스트), 스냅샷은 `snapshots\*.txt`. 삭제하면 다음 실행 때 자동 정리를 다시 수행한다.
- 테스트 자동화: PowerShell 에서 `keybd_event` 로 Ctrl+Shift+F/P 를 보내 Fence 창의 `IsWindowVisible`/`GetParent` 를 확인하고, 바탕화면에 `*.pdf` 를 만들어 3초 뒤 `layout.txt` 의 소속 Fence 를 확인하는 방식이 동작한다. 렌더링은 각 Fence HWND 에 `PrintWindow` 로 캡처한다.

## 핵심 동작 (Windows 11 에서 검증된 함정)
- Fence 창은 explorer 의 `SHELLDLL_DefView` 의 자식으로 붙여 바탕화면 레이어에 둔다. 다른 프로세스 창을 부모로 지정해
  `CreateWindowExW` 하면 실패하므로, **팝업으로 만든 뒤 `SetParent`** 한다.
- 바탕화면 원본 아이콘(`SysListView32`) 숨기기는 **`ShowWindowAsync` 만 동작**한다. 동기 `ShowWindow` 와
  `WM_COMMAND 0x7402` 토글은 Windows 11 26200 에서 무시된다. 종료(WM_DESTROY) 시 반드시 다시 표시한다.
- 바탕화면 빈 곳 더블클릭으로 Fence 숨김/표시: `WH_MOUSE_LL` 훅으로 더블클릭을 감지해 메인 창에 post 한다.
- 키보드: Fence 는 explorer 자식 창이라 WM_KEYDOWN 이 오지 않는다. `WH_KEYBOARD_LL` 훅으로 받되 **전경 창이 Progman/WorkerW 일 때만** 처리한다. Fence 클릭 시 `SetForegroundWindow(Progman)` 으로 바탕화면을 전경으로 만든다.
- 피크: 자식 창은 다른 창 위로 못 올라가므로 `SetParent(NULL)`+WS_POPUP+HWND_TOPMOST 로 떼었다가 `attach_to_desktop` 으로 되돌린다. TOPMOST 해제는 반드시 `SetWindowPos(HWND_NOTOPMOST)`.
- 제목 영역은 `WM_NCHITTEST` 가 HTCAPTION 을 돌려주므로 마우스 이동은 `WM_NCMOUSEMOVE` 로 온다(hover 펼치기 처리 시 주의).
- 48px 아이콘은 windows-sys 에 COM 인터페이스가 없어 `IImageList` vtable 을 수동 선언해 `GetIcon` 을 호출한다(`fence.rs`).
- 강제 종료(taskkill /F)하면 아이콘이 숨겨진 채 남는다. 테스트 후에는 메인 창(`IconDocsMain`)에 WM_CLOSE 를 보내 정상 종료할 것.
