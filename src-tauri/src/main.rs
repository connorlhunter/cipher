//! Native desktop entry point and commands for Cipher.

pub mod credential_store;
pub mod transport;

mod ipc;
mod security;

/// Returns the desktop core's current status for a compatible webview.
#[tauri::command]
fn desktop_status(protocol_version: Option<u16>) -> Result<ipc::DesktopStatus, ipc::IpcError> {
    ipc::desktop_status(protocol_version)
}

fn main() {
    tauri::Builder::default()
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

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![desktop_status])
        .run(tauri::generate_context!())
        .expect("Cipher desktop failed to start");
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
