use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DesktopStatus {
    message: &'static str,
}

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
