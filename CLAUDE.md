# CLAUDE.md

## 프로젝트
`icondocs`(아이콘독스) — Stardock Fences 스타일의 바탕화면 아이콘 정리 도구. Rust + `windows-sys`(Win32 API 직접 호출)만 사용하며
런타임/프레임워크가 없다. 단일 파일 `src/main.rs`.

## 빌드/실행
```
cargo build --release          # target/release/icondocs.exe (~320KB)
./target/release/icondocs.exe   # 트레이 아이콘으로 실행됨
```
- 툴체인: `rust-toolchain.toml` 이 `stable-x86_64-pc-windows-gnu` 를 고정한다. 이 PC 에는 MSVC 빌드 도구/Windows SDK 가 없어
  rustup 에 번들된 MinGW 링커를 쓰기 위함이다. (`rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal`)
- 설정 파일: `%APPDATA%\IconDocs\layout.txt` (단순 key=value 텍스트). 삭제하면 다음 실행 때 자동 정리를 다시 수행한다.

## 핵심 동작 (Windows 11 에서 검증된 함정)
- Fence 창은 explorer 의 `SHELLDLL_DefView` 의 자식으로 붙여 바탕화면 레이어에 둔다. 다른 프로세스 창을 부모로 지정해
  `CreateWindowExW` 하면 실패하므로, **팝업으로 만든 뒤 `SetParent`** 한다.
- 바탕화면 원본 아이콘(`SysListView32`) 숨기기는 **`ShowWindowAsync` 만 동작**한다. 동기 `ShowWindow` 와
  `WM_COMMAND 0x7402` 토글은 Windows 11 26200 에서 무시된다. 종료(WM_DESTROY) 시 반드시 다시 표시한다.
- 바탕화면 빈 곳 더블클릭으로 Fence 숨김/표시: `WH_MOUSE_LL` 훅으로 더블클릭을 감지해 메인 창에 post 한다.
- 강제 종료(taskkill /F)하면 아이콘이 숨겨진 채 남는다. 테스트 후에는 메인 창(`IconDocsMain`)에 WM_CLOSE 를 보내 정상 종료할 것.
