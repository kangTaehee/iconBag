// 실행 파일 매니페스트 삽입.
// - supportedOS(Windows 7~11): 이 선언이 있어야 Windows 8+ 의 레이어드 자식 창(Fence 투명도)이 동작한다.
// - PerMonitorV2 DPI 인식: main() 의 SetProcessDpiAwarenessContext 와 같은 값.
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("IconDocs")).expect("매니페스트 삽입 실패");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
