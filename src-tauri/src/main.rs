//! Native desktop entry point and commands for Cipher.

use tauri::Manager;

pub mod credential_store;
pub mod transport;

mod ipc;
mod lifecycle;
mod security;

/// Returns the desktop core's current status for a compatible webview.
#[tauri::command]
fn desktop_status(protocol_version: Option<u16>) -> Result<ipc::DesktopStatus, ipc::IpcError> {
    ipc::desktop_status(protocol_version)
}

/// Returns redacted desktop lifecycle state for a current-protocol webview.
#[tauri::command]
fn desktop_diagnostics(
    protocol_version: Option<u16>,
    lifecycle: tauri::State<'_, lifecycle::DesktopLifecycleService>,
) -> Result<cipher_desktop_lifecycle::SafeDesktopDiagnostic, ipc::IpcError> {
    lifecycle.diagnostic(protocol_version)
}

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        lifecycle::handle_single_instance_launch(app);
    }));

    let app = builder
        .manage(lifecycle::DesktopLifecycleService::new())
        .setup(|app| {
            let main_window = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == security::MAIN_WINDOW_LABEL)
                .expect("Cipher main window must be configured");

            tauri::WebviewWindowBuilder::from_config(app.handle(), main_window)?
                .on_navigation(security::allows_navigation)
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .on_download(|_, _| false)
                .build()?;

            app.state::<lifecycle::DesktopLifecycleService>()
                .handle_native_event(
                    app.handle(),
                    cipher_desktop_lifecycle::DesktopLifecycleEvent::ColdStart,
                )
                .map_err(|_| {
                    tauri::Error::AssetNotFound("desktop lifecycle initialization".into())
                })?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_status,
            desktop_diagnostics
        ])
        .build(tauri::generate_context!())
        .expect("Cipher desktop failed to start");
    app.run(|app, event| lifecycle::handle_run_event(app, &event));
}

#[cfg(test)]
mod tests {
    use super::{desktop_status, ipc::CURRENT_PROTOCOL_VERSION};

    #[test]
    fn reports_the_desktop_core_status() {
        assert_eq!(
            desktop_status(Some(CURRENT_PROTOCOL_VERSION.get()))
                .unwrap()
                .message,
            "Desktop core is ready."
        );
    }
}
