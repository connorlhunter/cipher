//! Native desktop entry point and commands for Cipher.

use tauri::Manager;

pub mod auth;
#[cfg(not(coverage))]
pub mod auth_service;
#[cfg(coverage)]
#[path = "auth_service_coverage.rs"]
/// Deterministic native authentication surface used by coverage-only builds.
pub mod auth_service;
pub mod cognito;
pub mod credential_store;
pub mod session;
pub mod theme;
pub mod transport;

mod ipc;
pub mod lifecycle;
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

/// Returns the native-resolved application appearance for a current-protocol webview.
#[tauri::command]
fn desktop_theme<R: tauri::Runtime>(
    protocol_version: Option<u16>,
    app: tauri::AppHandle<R>,
    theme: tauri::State<'_, theme::DesktopThemeService>,
) -> Result<theme::DesktopTheme, ipc::IpcError> {
    theme.current(&app, protocol_version)
}

/// Applies one native-owned system or explicit scheme preference across the app window.
#[tauri::command]
fn desktop_set_theme<R: tauri::Runtime>(
    preference: theme::DesktopThemePreference,
    protocol_version: Option<u16>,
    app: tauri::AppHandle<R>,
    theme: tauri::State<'_, theme::DesktopThemeService>,
) -> Result<theme::DesktopTheme, ipc::IpcError> {
    theme.set(&app, preference, protocol_version)
}

/// Submits one bounded sign-in or administrator-invitation request to native Cognito handling.
#[tauri::command]
async fn desktop_authenticate(
    request: auth::AuthenticationRequest,
    protocol_version: Option<u16>,
    authentication: tauri::State<'_, auth_service::DesktopAuthenticationService>,
    session: tauri::State<'_, session::DesktopSessionService>,
    lifecycle: tauri::State<'_, lifecycle::DesktopLifecycleService>,
) -> Result<auth::AuthenticationView, ipc::IpcError> {
    ipc::require_current_protocol_version(protocol_version)?;
    let operation = lifecycle
        .begin_native_operation(lifecycle::NativeOperationKind::Authentication)
        .map_err(|_| ipc::IpcError::unavailable())?;
    let cancellation = operation.cancellation();
    let view = authentication
        .authenticate(&request, &session, &cancellation)
        .await;
    lifecycle
        .finish_native_operation(operation)
        .map_err(|_| ipc::IpcError::unavailable())?;
    Ok(view)
}

fn main() {
    let builder = tauri::Builder::default();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        lifecycle::handle_single_instance_launch(app);
    }));

    let app = builder
        .manage(lifecycle::DesktopLifecycleService::new())
        .manage(auth_service::DesktopAuthenticationService::new())
        .manage(session::DesktopSessionService::new())
        .manage(theme::DesktopThemeService::new())
        .setup(|app| {
            let window_theme = app
                .state::<theme::DesktopThemeService>()
                .initialize(app.handle())
                .map_err(|_| {
                    tauri::Error::AssetNotFound("desktop appearance configuration".into())
                })?
                .window_theme();
            let main_window = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == security::MAIN_WINDOW_LABEL)
                .expect("Cipher main window must be configured");

            tauri::WebviewWindowBuilder::from_config(app.handle(), main_window)?
                .theme(window_theme)
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

            let lifecycle = app.state::<lifecycle::DesktopLifecycleService>();
            let restoration = lifecycle
                .begin_native_operation(lifecycle::NativeOperationKind::Authentication)
                .map_err(|_| tauri::Error::AssetNotFound("desktop session restoration".into()))?;
            let cancellation = restoration.cancellation();
            let session = app.state::<session::DesktopSessionService>();
            let authentication = app.state::<auth_service::DesktopAuthenticationService>();
            let _ = authentication.install_refresher(&session, &cancellation);
            let _ = session.restore_on_startup(&cancellation);
            lifecycle
                .finish_native_operation(restoration)
                .map_err(|_| tauri::Error::AssetNotFound("desktop session restoration".into()))?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            desktop_status,
            desktop_diagnostics,
            desktop_theme,
            desktop_set_theme,
            desktop_authenticate
        ])
        .build(tauri::generate_context!())
        .expect("Cipher desktop failed to start");
    app.run(|app, event| {
        lifecycle::handle_run_event(app, &event);
        theme::handle_run_event(app, &event);
    });
}

#[cfg(test)]
mod tests {
    use tauri::{Manager, WebviewWindowBuilder};

    use super::{
        desktop_diagnostics, desktop_set_theme, desktop_status, desktop_theme,
        ipc::CURRENT_PROTOCOL_VERSION,
    };

    #[test]
    fn reports_the_desktop_core_status() {
        assert_eq!(
            desktop_status(Some(CURRENT_PROTOCOL_VERSION.get()))
                .unwrap()
                .message,
            "Desktop core is ready."
        );
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![desktop_theme, desktop_set_theme])
            .manage(super::lifecycle::DesktopLifecycleService::new())
            .manage(super::theme::DesktopThemeService::new())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let webview = WebviewWindowBuilder::new(
            &app,
            super::security::MAIN_WINDOW_LABEL,
            tauri::WebviewUrl::default(),
        )
        .build()
        .unwrap();
        assert!(desktop_diagnostics(Some(CURRENT_PROTOCOL_VERSION.get()), app.state()).is_ok());

        let theme_response = tauri::test::get_ipc_response(
            &webview,
            invoke_request(
                "desktop_theme",
                serde_json::json!({ "protocolVersion": CURRENT_PROTOCOL_VERSION.get() }),
            ),
        )
        .unwrap()
        .deserialize::<serde_json::Value>()
        .unwrap();
        assert_eq!(theme_response["preference"], "system");
        assert_eq!(theme_response["scheme"], "atlas");
        assert_eq!(theme_response["resolved"], "light");

        let set_response = tauri::test::get_ipc_response(
            &webview,
            invoke_request(
                "desktop_set_theme",
                serde_json::json!({
                    "preference": "harbor",
                    "protocolVersion": CURRENT_PROTOCOL_VERSION.get()
                }),
            ),
        );
        assert!(set_response.is_err());
    }

    fn invoke_request(command: &str, body: serde_json::Value) -> tauri::webview::InvokeRequest {
        tauri::webview::InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: tauri::ipc::InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_owned(),
        }
    }
}
