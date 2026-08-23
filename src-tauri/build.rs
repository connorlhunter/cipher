//! Build-script entry point for the Cipher desktop app.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(coverage)");
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "desktop_status",
            "desktop_diagnostics",
            "desktop_theme",
            "desktop_set_theme",
            "desktop_authenticate",
        ]),
    ))
    .expect("failed to build Cipher desktop configuration");
}
