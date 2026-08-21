//! Native desktop entry point and commands for Cipher.

mod ipc;

/// Returns the desktop core's current status for a compatible webview.
#[tauri::command]
fn desktop_status(protocol_version: Option<u16>) -> Result<ipc::DesktopStatus, ipc::IpcError> {
    ipc::desktop_status(protocol_version)
}

fn main() {
    tauri::Builder::default()
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
            desktop_status(Some(CURRENT_PROTOCOL_VERSION))
                .unwrap()
                .message,
            "Desktop core is ready."
        );
    }
}
