// One ACL permission per HQ command (`allow-<name>`), so the capabilities in
// `capabilities/` can grant the main window every HQ command and the capture window
// only what capture needs. Declaring the manifest also makes Tauri check every app
// command against the ACL, not only plugin ones.
include!("src/commands/names.rs");

fn main() {
    let _ = CAPTURE_WINDOW;
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build script failed");
}
