//! Build-script entry point for the Cipher desktop app.

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["desktop_status"])),
    )
    .expect("failed to build Cipher desktop configuration");
}
