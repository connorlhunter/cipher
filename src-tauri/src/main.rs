//! Native desktop entry point and commands for Cipher.

use serde::Serialize;

/// Status returned to the webview by the desktop command.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopStatus {
    /// Desktop status message.
    message: &'static str,
}

/// Returns the desktop core's current status.
#[tauri::command]
fn desktop_status() -> DesktopStatus {
    DesktopStatus {
        message: "Desktop core is ready.",
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![desktop_status])
        .run(tauri::generate_context!())
        .expect("Cipher desktop failed to start");
}
