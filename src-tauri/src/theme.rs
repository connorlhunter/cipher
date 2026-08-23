//! Native-owned appearance selection shared by the title bar and webview.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, Theme};

use crate::{ipc, security::MAIN_WINDOW_LABEL};

/// A content-free event telling the webview to re-read the resolved native theme.
pub const DESKTOP_THEME_CHANGED_EVENT: &str = "cipher://theme/changed";
const THEME_CONFIG_FILE_NAME: &str = "appearance.json";
const MAX_THEME_CONFIG_BYTES: usize = 128;

/// The one application-wide appearance preference selected in native code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopThemePreference {
    /// Follow the current platform window appearance.
    System,
    /// Use a light application appearance.
    Light,
    /// Use a dark application appearance.
    Dark,
}

impl DesktopThemePreference {
    /// Converts the preference into the native window override expected by Tauri.
    pub(crate) const fn window_theme(self) -> Option<Theme> {
        match self {
            Self::System => None,
            Self::Light => Some(Theme::Light),
            Self::Dark => Some(Theme::Dark),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredDesktopTheme {
    preference: DesktopThemePreference,
}

/// A resolved display-safe theme view for the webview shell.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopTheme {
    /// The preference owned by this desktop process.
    pub preference: DesktopThemePreference,
    /// The concrete light or dark theme currently drawn by the native window.
    pub resolved: ResolvedDesktopTheme,
}

/// The only concrete color schemes supplied to the webview.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedDesktopTheme {
    /// A light appearance.
    Light,
    /// A dark appearance.
    Dark,
}

impl From<Theme> for ResolvedDesktopTheme {
    fn from(value: Theme) -> Self {
        match value {
            Theme::Light => Self::Light,
            Theme::Dark => Self::Dark,
            _ => Self::Light,
        }
    }
}

/// Holds the single native preference without exposing browser persistence to the webview.
pub struct DesktopThemeService {
    preference: Mutex<DesktopThemePreference>,
    config_path: Mutex<Option<PathBuf>>,
}

impl DesktopThemeService {
    /// Creates a service that follows the system window theme until a choice is made.
    pub const fn new() -> Self {
        Self {
            preference: Mutex::new(DesktopThemePreference::System),
            config_path: Mutex::new(None),
        }
    }

    /// Loads native application configuration before the first window is created.
    pub fn initialize<R: Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> Result<DesktopThemePreference, ipc::IpcError> {
        let config_directory = match app.path().app_config_dir() {
            Ok(path) => path,
            Err(_) => return Err(ipc::IpcError::unavailable()),
        };
        let config_path = config_directory.join(THEME_CONFIG_FILE_NAME);
        let preference = read_preference(&config_path);

        let mut stored_path = match self.config_path.lock() {
            Ok(path) => path,
            Err(_) => return Err(ipc::IpcError::unavailable()),
        };
        *stored_path = Some(config_path);
        drop(stored_path);

        let mut stored_preference = match self.preference.lock() {
            Ok(preference) => preference,
            Err(_) => return Err(ipc::IpcError::unavailable()),
        };
        *stored_preference = preference;

        Ok(preference)
    }

    /// Returns the current safe theme view after requiring the current IPC protocol.
    pub fn current<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        protocol_version: Option<u16>,
    ) -> Result<DesktopTheme, ipc::IpcError> {
        ipc::require_current_protocol_version(protocol_version)?;
        let preference = match self.preference.lock() {
            Ok(preference) => *preference,
            Err(_) => return Err(ipc::IpcError::unavailable()),
        };
        Ok(DesktopTheme {
            preference,
            resolved: self.resolve(app, preference)?,
        })
    }

    /// Sets one native preference and applies the matching title-bar treatment.
    pub fn set<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        preference: DesktopThemePreference,
        protocol_version: Option<u16>,
    ) -> Result<DesktopTheme, ipc::IpcError> {
        ipc::require_current_protocol_version(protocol_version)?;
        let mut stored_preference = match self.preference.lock() {
            Ok(preference) => preference,
            Err(_) => return Err(ipc::IpcError::unavailable()),
        };
        let previous = *stored_preference;
        let config_path = match self.config_path.lock() {
            Ok(path) => match path.clone() {
                Some(path) => path,
                None => return Err(ipc::IpcError::unavailable()),
            },
            Err(_) => return Err(ipc::IpcError::unavailable()),
        };

        write_preference(&config_path, preference)?;
        if apply_to_controlled_windows(app, preference).is_err() {
            let _ = write_preference(&config_path, previous);
            let _ = apply_to_controlled_windows(app, previous);
            return Err(ipc::IpcError::unavailable());
        }
        *stored_preference = preference;
        drop(stored_preference);
        let _ = app.emit(DESKTOP_THEME_CHANGED_EVENT, ());

        Ok(DesktopTheme {
            preference,
            resolved: self.resolve(app, preference)?,
        })
    }

    fn follows_system(&self) -> bool {
        match self.preference.lock() {
            Ok(preference) => *preference == DesktopThemePreference::System,
            Err(_) => false,
        }
    }

    fn resolve<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        preference: DesktopThemePreference,
    ) -> Result<ResolvedDesktopTheme, ipc::IpcError> {
        match preference {
            DesktopThemePreference::Light => Ok(ResolvedDesktopTheme::Light),
            DesktopThemePreference::Dark => Ok(ResolvedDesktopTheme::Dark),
            DesktopThemePreference::System => {
                let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
                    return Err(ipc::IpcError::unavailable());
                };
                match window.theme() {
                    Ok(theme) => Ok(ResolvedDesktopTheme::from(theme)),
                    Err(_) => Err(ipc::IpcError::unavailable()),
                }
            }
        }
    }
}

fn apply_to_controlled_windows<R: Runtime>(
    app: &AppHandle<R>,
    preference: DesktopThemePreference,
) -> Result<(), ipc::IpcError> {
    let windows = app.webview_windows();
    if windows.is_empty() {
        return Err(ipc::IpcError::unavailable());
    }
    for window in windows.values() {
        if window.set_theme(preference.window_theme()).is_err() {
            return Err(ipc::IpcError::unavailable());
        }
    }
    Ok(())
}

fn read_preference(path: &Path) -> DesktopThemePreference {
    let Ok(metadata) = fs::metadata(path) else {
        return DesktopThemePreference::System;
    };
    if !metadata.is_file() || metadata.len() > MAX_THEME_CONFIG_BYTES as u64 {
        return DesktopThemePreference::System;
    }
    let Ok(bytes) = fs::read(path) else {
        return DesktopThemePreference::System;
    };
    if bytes.len() > MAX_THEME_CONFIG_BYTES {
        return DesktopThemePreference::System;
    }

    match serde_json::from_slice::<StoredDesktopTheme>(&bytes) {
        Ok(stored) => stored.preference,
        Err(_) => DesktopThemePreference::System,
    }
}

fn write_preference(path: &Path, preference: DesktopThemePreference) -> Result<(), ipc::IpcError> {
    let Some(parent) = path.parent() else {
        return Err(ipc::IpcError::unavailable());
    };
    if fs::create_dir_all(parent).is_err() {
        return Err(ipc::IpcError::unavailable());
    }

    let mut temporary = match tempfile::NamedTempFile::new_in(parent) {
        Ok(file) => file,
        Err(_) => return Err(ipc::IpcError::unavailable()),
    };
    if serde_json::to_writer(temporary.as_file_mut(), &StoredDesktopTheme { preference }).is_err() {
        return Err(ipc::IpcError::unavailable());
    }
    if temporary.as_file_mut().write_all(b"\n").is_err()
        || temporary.as_file().sync_all().is_err()
        || temporary.persist(path).is_err()
    {
        return Err(ipc::IpcError::unavailable());
    }
    Ok(())
}

impl Default for DesktopThemeService {
    fn default() -> Self {
        Self::new()
    }
}

/// Emits a no-payload refresh signal only when the native preference follows the system.
pub fn handle_run_event<R: Runtime>(app: &AppHandle<R>, event: &tauri::RunEvent) {
    let tauri::RunEvent::WindowEvent { label, event, .. } = event else {
        return;
    };
    if label != MAIN_WINDOW_LABEL || !matches!(event, tauri::WindowEvent::ThemeChanged(_)) {
        return;
    }

    let Some(service) = app.try_state::<DesktopThemeService>() else {
        return;
    };
    if service.follows_system() {
        let _ = app.emit(DESKTOP_THEME_CHANGED_EVENT, ());
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        panic::{AssertUnwindSafe, catch_unwind},
    };

    use super::{
        DesktopThemePreference, DesktopThemeService, MAX_THEME_CONFIG_BYTES, ResolvedDesktopTheme,
        apply_to_controlled_windows, handle_run_event, read_preference, write_preference,
    };
    use tauri::{Manager, Theme, WebviewWindowBuilder};

    fn managed_mock_app(create_window: bool) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_builder()
            .manage(DesktopThemeService::new())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        if create_window {
            WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
                .build()
                .unwrap();
        }
        app
    }

    #[test]
    fn maps_native_window_themes_to_the_bounded_webview_values() {
        assert_eq!(
            ResolvedDesktopTheme::from(Theme::Light),
            ResolvedDesktopTheme::Light
        );
        assert_eq!(
            ResolvedDesktopTheme::from(Theme::Dark),
            ResolvedDesktopTheme::Dark
        );
    }

    #[test]
    fn preference_serialization_uses_only_the_three_supported_values() {
        for (preference, expected) in [
            (DesktopThemePreference::System, "\"system\""),
            (DesktopThemePreference::Light, "\"light\""),
            (DesktopThemePreference::Dark, "\"dark\""),
        ] {
            assert_eq!(serde_json::to_string(&preference).unwrap(), expected);
        }

        assert_eq!(DesktopThemePreference::System.window_theme(), None);
        assert_eq!(
            DesktopThemePreference::Light.window_theme(),
            Some(Theme::Light)
        );
        assert_eq!(
            DesktopThemePreference::Dark.window_theme(),
            Some(Theme::Dark)
        );

        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../contracts/ipc/v1/desktop-theme.json"))
                .unwrap();
        assert_eq!(fixture["protocolVersion"], 1);
        assert_eq!(fixture["command"], "desktop_theme");
        assert_eq!(fixture["response"]["preference"], "system");
        assert_eq!(fixture["response"]["resolved"], "dark");
    }

    #[test]
    fn native_configuration_round_trips_and_replaces_the_previous_preference() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("appearance.json");

        write_preference(&path, DesktopThemePreference::Light).unwrap();
        assert_eq!(read_preference(&path), DesktopThemePreference::Light);

        write_preference(&path, DesktopThemePreference::Dark).unwrap();
        assert_eq!(read_preference(&path), DesktopThemePreference::Dark);
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "{\"preference\":\"dark\"}\n"
        );
    }

    #[test]
    fn missing_malformed_or_unbounded_configuration_follows_the_system() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("appearance.json");

        assert_eq!(read_preference(&path), DesktopThemePreference::System);
        assert_eq!(
            read_preference(directory.path()),
            DesktopThemePreference::System
        );
        fs::write(&path, br#"{"preference":"browser"}"#).unwrap();
        assert_eq!(read_preference(&path), DesktopThemePreference::System);
        fs::write(&path, vec![b'x'; MAX_THEME_CONFIG_BYTES + 1]).unwrap();
        assert_eq!(read_preference(&path), DesktopThemePreference::System);
    }

    #[test]
    fn managed_theme_service_initializes_resolves_persists_and_applies_preferences() {
        let app = managed_mock_app(true);
        let handle = app.handle().clone();
        let service = app.state::<DesktopThemeService>();
        let initialized = service.initialize(&handle).unwrap();
        assert!(matches!(
            initialized,
            DesktopThemePreference::System
                | DesktopThemePreference::Light
                | DesktopThemePreference::Dark
        ));

        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("appearance.json");
        *service.config_path.lock().unwrap() = Some(config_path.clone());
        *service.preference.lock().unwrap() = DesktopThemePreference::System;

        let system = service.current(&handle, Some(1)).unwrap();
        assert_eq!(system.preference, DesktopThemePreference::System);
        assert_eq!(system.resolved, ResolvedDesktopTheme::Light);
        assert!(service.follows_system());
        handle_run_event(&handle, &tauri::RunEvent::Resumed);

        let light = service
            .set(&handle, DesktopThemePreference::Light, Some(1))
            .unwrap();
        assert_eq!(light.preference, DesktopThemePreference::Light);
        assert_eq!(light.resolved, ResolvedDesktopTheme::Light);
        assert!(!service.follows_system());

        let dark = service
            .set(&handle, DesktopThemePreference::Dark, Some(1))
            .unwrap();
        assert_eq!(dark.preference, DesktopThemePreference::Dark);
        assert_eq!(dark.resolved, ResolvedDesktopTheme::Dark);
        assert_eq!(read_preference(&config_path), DesktopThemePreference::Dark);
        assert!(service.current(&handle, Some(0)).is_err());

        let default_service = DesktopThemeService::default();
        assert!(default_service.follows_system());
    }

    #[test]
    fn managed_theme_service_fails_closed_without_configuration_or_windows() {
        let app = managed_mock_app(false);
        let handle = app.handle().clone();
        let service = app.state::<DesktopThemeService>();

        assert!(service.current(&handle, Some(1)).is_err());
        assert!(apply_to_controlled_windows(&handle, DesktopThemePreference::System).is_err());
        assert!(
            service
                .set(&handle, DesktopThemePreference::Light, Some(1))
                .is_err()
        );

        let directory = tempfile::tempdir().unwrap();
        *service.config_path.lock().unwrap() = Some(directory.path().join("appearance.json"));
        assert!(
            service
                .set(&handle, DesktopThemePreference::Light, Some(1))
                .is_err()
        );
        assert_eq!(
            *service.preference.lock().unwrap(),
            DesktopThemePreference::System
        );
    }

    #[test]
    fn poisoned_theme_state_never_exposes_an_unresolved_preference() {
        let app = managed_mock_app(true);
        let handle = app.handle().clone();
        let service = app.state::<DesktopThemeService>();

        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _guard = service.preference.lock().unwrap();
                panic!("test theme lock poisoning");
            }))
            .is_err()
        );
        assert!(!service.follows_system());
        assert!(service.current(&handle, Some(1)).is_err());
        assert!(
            service
                .set(&handle, DesktopThemePreference::Dark, Some(1))
                .is_err()
        );
    }
}
