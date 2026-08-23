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

/// A concrete application color scheme safe to expose to the webview.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopThemeScheme {
    /// Cool neutral light surfaces with a teal accent.
    Atlas,
    /// Low-chroma light surfaces.
    Paper,
    /// Warm yellow-green light surfaces.
    Citrine,
    /// Blue-gray dark surfaces with a cyan accent.
    Harbor,
    /// Deep blue dark surfaces.
    Midnight,
    /// Neutral dark surfaces with a blue accent.
    Onyx,
    /// Soft rose light surfaces.
    Rose,
    /// Cool blue light surfaces.
    Tide,
    /// Warm orange light surfaces.
    Ember,
    /// Violet-tinted light surfaces.
    Quartz,
}

impl DesktopThemeScheme {
    /// Returns the light or dark classification used by native window chrome.
    pub const fn resolved(self) -> ResolvedDesktopTheme {
        match self {
            Self::Harbor | Self::Midnight | Self::Onyx => ResolvedDesktopTheme::Dark,
            Self::Atlas
            | Self::Paper
            | Self::Citrine
            | Self::Rose
            | Self::Tide
            | Self::Ember
            | Self::Quartz => ResolvedDesktopTheme::Light,
        }
    }

    const fn window_theme(self) -> Theme {
        match self.resolved() {
            ResolvedDesktopTheme::Light => Theme::Light,
            ResolvedDesktopTheme::Dark => Theme::Dark,
        }
    }

    const fn system_default(theme: Theme) -> Self {
        match theme {
            Theme::Dark => Self::Midnight,
            Theme::Light => Self::Atlas,
            _ => Self::Atlas,
        }
    }
}

/// The one application-wide appearance preference selected in native code.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopThemePreference {
    /// Follow the platform light/dark setting and use the matching default scheme.
    System,
    /// Always use the Atlas scheme.
    Atlas,
    /// Always use the Paper scheme.
    Paper,
    /// Always use the Citrine scheme.
    Citrine,
    /// Always use the Harbor scheme.
    Harbor,
    /// Always use the Midnight scheme.
    Midnight,
    /// Always use the Onyx scheme.
    Onyx,
    /// Always use the Rose scheme.
    Rose,
    /// Always use the Tide scheme.
    Tide,
    /// Always use the Ember scheme.
    Ember,
    /// Always use the Quartz scheme.
    Quartz,
}

impl DesktopThemePreference {
    /// Returns an explicitly selected scheme, or none when following the system.
    pub const fn explicit_scheme(self) -> Option<DesktopThemeScheme> {
        match self {
            Self::System => None,
            Self::Atlas => Some(DesktopThemeScheme::Atlas),
            Self::Paper => Some(DesktopThemeScheme::Paper),
            Self::Citrine => Some(DesktopThemeScheme::Citrine),
            Self::Harbor => Some(DesktopThemeScheme::Harbor),
            Self::Midnight => Some(DesktopThemeScheme::Midnight),
            Self::Onyx => Some(DesktopThemeScheme::Onyx),
            Self::Rose => Some(DesktopThemeScheme::Rose),
            Self::Tide => Some(DesktopThemeScheme::Tide),
            Self::Ember => Some(DesktopThemeScheme::Ember),
            Self::Quartz => Some(DesktopThemeScheme::Quartz),
        }
    }

    /// Converts the preference into the native window override expected by Tauri.
    pub(crate) const fn window_theme(self) -> Option<Theme> {
        match self.explicit_scheme() {
            Some(scheme) => Some(scheme.window_theme()),
            None => None,
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
    /// The native-owned system or explicit-scheme preference.
    pub preference: DesktopThemePreference,
    /// The concrete scheme selected for semantic webview tokens.
    pub scheme: DesktopThemeScheme,
    /// The native light/dark classification for window and control treatment.
    pub resolved: ResolvedDesktopTheme,
}

/// The light/dark classification applied to native window controls.
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
        DesktopThemeScheme::system_default(value).resolved()
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
        let config_directory = app
            .path()
            .app_config_dir()
            .map_err(|_| ipc::IpcError::unavailable())?;
        let config_path = config_directory.join(THEME_CONFIG_FILE_NAME);
        let preference = read_preference(&config_path);

        *self
            .config_path
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())? = Some(config_path);
        *self
            .preference
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())? = preference;

        Ok(preference)
    }

    /// Returns the current safe theme view after requiring the current IPC protocol.
    pub fn current<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        protocol_version: Option<u16>,
    ) -> Result<DesktopTheme, ipc::IpcError> {
        ipc::require_current_protocol_version(protocol_version)?;
        let preference = *self
            .preference
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())?;
        self.resolve(app, preference)
    }

    /// Sets one native preference and applies the matching title-bar treatment.
    pub fn set<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        preference: DesktopThemePreference,
        protocol_version: Option<u16>,
    ) -> Result<DesktopTheme, ipc::IpcError> {
        ipc::require_current_protocol_version(protocol_version)?;
        let mut stored_preference = self
            .preference
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())?;
        let previous = *stored_preference;
        let config_path = self
            .config_path
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())?
            .clone()
            .ok_or_else(ipc::IpcError::unavailable)?;

        write_preference(&config_path, preference)?;
        if apply_to_controlled_windows(app, preference).is_err() {
            let _ = write_preference(&config_path, previous);
            let _ = apply_to_controlled_windows(app, previous);
            return Err(ipc::IpcError::unavailable());
        }
        *stored_preference = preference;
        drop(stored_preference);
        let _ = app.emit(DESKTOP_THEME_CHANGED_EVENT, ());

        self.resolve(app, preference)
    }

    /// Removes the native appearance preference before the application is removed.
    pub fn remove_local_preference(&self) -> Result<(), ipc::IpcError> {
        let config_path = self
            .config_path
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())?
            .clone();
        if let Some(config_path) = config_path {
            match fs::remove_file(config_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(ipc::IpcError::unavailable()),
            }
        }
        *self
            .preference
            .lock()
            .map_err(|_| ipc::IpcError::unavailable())? = DesktopThemePreference::System;
        Ok(())
    }

    fn follows_system(&self) -> bool {
        self.preference
            .lock()
            .map(|preference| *preference == DesktopThemePreference::System)
            .unwrap_or(false)
    }

    fn resolve<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        preference: DesktopThemePreference,
    ) -> Result<DesktopTheme, ipc::IpcError> {
        let scheme = match preference.explicit_scheme() {
            Some(scheme) => scheme,
            None => {
                let window = app
                    .get_webview_window(MAIN_WINDOW_LABEL)
                    .ok_or_else(ipc::IpcError::unavailable)?;
                DesktopThemeScheme::system_default(
                    window.theme().map_err(|_| ipc::IpcError::unavailable())?,
                )
            }
        };

        Ok(DesktopTheme {
            preference,
            scheme,
            resolved: scheme.resolved(),
        })
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
        window
            .set_theme(preference.window_theme())
            .map_err(|_| ipc::IpcError::unavailable())?;
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

    serde_json::from_slice::<StoredDesktopTheme>(&bytes)
        .map(|stored| stored.preference)
        .unwrap_or(DesktopThemePreference::System)
}

fn write_preference(path: &Path, preference: DesktopThemePreference) -> Result<(), ipc::IpcError> {
    let parent = path.parent().ok_or_else(ipc::IpcError::unavailable)?;
    fs::create_dir_all(parent).map_err(|_| ipc::IpcError::unavailable())?;

    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| ipc::IpcError::unavailable())?;
    serde_json::to_writer(temporary.as_file_mut(), &StoredDesktopTheme { preference })
        .map_err(|_| ipc::IpcError::unavailable())?;
    temporary
        .as_file_mut()
        .write_all(b"\n")
        .map_err(|_| ipc::IpcError::unavailable())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| ipc::IpcError::unavailable())?;
    temporary
        .persist(path)
        .map_err(|_| ipc::IpcError::unavailable())?;
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
        path::Path,
    };

    use super::{
        DesktopThemePreference, DesktopThemeScheme, DesktopThemeService, MAX_THEME_CONFIG_BYTES,
        ResolvedDesktopTheme, apply_to_controlled_windows, handle_run_event, read_preference,
        write_preference,
    };
    use tauri::{Manager, Theme, WebviewWindowBuilder};

    const EXPLICIT_PREFERENCES: [DesktopThemePreference; 10] = [
        DesktopThemePreference::Atlas,
        DesktopThemePreference::Paper,
        DesktopThemePreference::Citrine,
        DesktopThemePreference::Harbor,
        DesktopThemePreference::Midnight,
        DesktopThemePreference::Onyx,
        DesktopThemePreference::Rose,
        DesktopThemePreference::Tide,
        DesktopThemePreference::Ember,
        DesktopThemePreference::Quartz,
    ];

    fn managed_mock_app(theme: Option<Theme>) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_builder()
            .manage(DesktopThemeService::new())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        WebviewWindowBuilder::new(&app, MAIN_WINDOW_LABEL, tauri::WebviewUrl::default())
            .theme(theme)
            .build()
            .unwrap();
        app
    }

    use crate::security::MAIN_WINDOW_LABEL;

    #[test]
    fn serializes_every_supported_preference_and_bounded_view_field() {
        assert_eq!(
            serde_json::to_string(&DesktopThemePreference::System).unwrap(),
            "\"system\""
        );
        for preference in EXPLICIT_PREFERENCES {
            let scheme = preference.explicit_scheme().unwrap();
            assert_eq!(
                serde_json::to_string(&preference).unwrap(),
                serde_json::to_string(&scheme).unwrap()
            );
            assert_eq!(preference.window_theme(), Some(scheme.window_theme()));
        }
        assert_eq!(DesktopThemePreference::System.window_theme(), None);

        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../contracts/ipc/v1/desktop-theme.json"))
                .unwrap();
        assert_eq!(fixture["protocolVersion"], 1);
        assert_eq!(fixture["command"], "desktop_theme");
        assert_eq!(fixture["response"]["preference"], "system");
        assert_eq!(fixture["response"]["scheme"], "midnight");
        assert_eq!(fixture["response"]["resolved"], "dark");
    }

    #[test]
    fn classifies_all_ten_schemes_for_native_window_treatment() {
        for scheme in [
            DesktopThemeScheme::Atlas,
            DesktopThemeScheme::Paper,
            DesktopThemeScheme::Citrine,
            DesktopThemeScheme::Rose,
            DesktopThemeScheme::Tide,
            DesktopThemeScheme::Ember,
            DesktopThemeScheme::Quartz,
        ] {
            assert_eq!(scheme.resolved(), ResolvedDesktopTheme::Light);
            assert_eq!(scheme.window_theme(), Theme::Light);
        }
        for scheme in [
            DesktopThemeScheme::Harbor,
            DesktopThemeScheme::Midnight,
            DesktopThemeScheme::Onyx,
        ] {
            assert_eq!(scheme.resolved(), ResolvedDesktopTheme::Dark);
            assert_eq!(scheme.window_theme(), Theme::Dark);
        }
    }

    #[test]
    fn native_configuration_round_trips_and_replaces_the_previous_preference() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("appearance.json");

        write_preference(&path, DesktopThemePreference::Rose).unwrap();
        assert_eq!(read_preference(&path), DesktopThemePreference::Rose);

        write_preference(&path, DesktopThemePreference::Onyx).unwrap();
        assert_eq!(read_preference(&path), DesktopThemePreference::Onyx);
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "{\"preference\":\"onyx\"}\n"
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
        for malformed in [
            br#"{"preference":"dark"}"#.as_slice(),
            br#"{"preference":"atlas","extra":true}"#.as_slice(),
            br#"{"preference":3}"#.as_slice(),
        ] {
            fs::write(&path, malformed).unwrap();
            assert_eq!(read_preference(&path), DesktopThemePreference::System);
        }
        fs::write(&path, vec![b'x'; MAX_THEME_CONFIG_BYTES + 1]).unwrap();
        assert_eq!(read_preference(&path), DesktopThemePreference::System);
    }

    #[test]
    fn system_resolution_uses_only_the_matching_default_scheme() {
        for (native, scheme, resolved) in [
            (
                Theme::Light,
                DesktopThemeScheme::Atlas,
                ResolvedDesktopTheme::Light,
            ),
            (
                Theme::Dark,
                DesktopThemeScheme::Midnight,
                ResolvedDesktopTheme::Dark,
            ),
        ] {
            let default = DesktopThemeScheme::system_default(native);
            assert_eq!(default, scheme);
            assert_eq!(default.resolved(), resolved);
        }

        let app = managed_mock_app(None);
        let service = app.state::<DesktopThemeService>();
        let view = service.current(app.handle(), Some(1)).unwrap();
        assert_eq!(view.preference, DesktopThemePreference::System);
        assert_eq!(view.scheme, DesktopThemeScheme::Atlas);
        assert_eq!(view.resolved, ResolvedDesktopTheme::Light);
        assert!(service.follows_system());
    }

    #[test]
    fn native_defaults_initialize_and_ignore_unrelated_run_events() {
        let standalone = DesktopThemeService::default();
        assert!(standalone.follows_system());
        assert_eq!(
            ResolvedDesktopTheme::from(Theme::Light),
            ResolvedDesktopTheme::Light
        );
        assert_eq!(
            ResolvedDesktopTheme::from(Theme::Dark),
            ResolvedDesktopTheme::Dark
        );

        let app = managed_mock_app(Some(Theme::Light));
        let service = app.state::<DesktopThemeService>();
        assert!(service.initialize(app.handle()).is_ok());
        assert!(service.config_path.lock().unwrap().is_some());
        handle_run_event(app.handle(), &tauri::RunEvent::Ready);
        assert!(service.follows_system());
    }

    #[test]
    fn explicit_schemes_persist_and_apply_their_native_classification() {
        let app = managed_mock_app(Some(Theme::Light));
        let handle = app.handle().clone();
        let service = app.state::<DesktopThemeService>();
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("appearance.json");
        *service.config_path.lock().unwrap() = Some(config_path.clone());

        let harbor = service
            .set(&handle, DesktopThemePreference::Harbor, Some(1))
            .unwrap();
        assert_eq!(harbor.scheme, DesktopThemeScheme::Harbor);
        assert_eq!(harbor.resolved, ResolvedDesktopTheme::Dark);
        assert!(!service.follows_system());

        let rose = service
            .set(&handle, DesktopThemePreference::Rose, Some(1))
            .unwrap();
        assert_eq!(rose.scheme, DesktopThemeScheme::Rose);
        assert_eq!(rose.resolved, ResolvedDesktopTheme::Light);
        assert_eq!(read_preference(&config_path), DesktopThemePreference::Rose);
        assert!(service.current(&handle, Some(0)).is_err());
    }

    #[test]
    fn device_removal_discards_the_native_appearance_preference() {
        let app = managed_mock_app(Some(Theme::Light));
        let service = app.state::<DesktopThemeService>();
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("appearance.json");
        *service.config_path.lock().unwrap() = Some(config_path.clone());
        write_preference(&config_path, DesktopThemePreference::Onyx).unwrap();
        *service.preference.lock().unwrap() = DesktopThemePreference::Onyx;

        service.remove_local_preference().unwrap();

        assert!(!config_path.exists());
        assert_eq!(
            *service.preference.lock().unwrap(),
            DesktopThemePreference::System
        );
    }

    #[test]
    fn managed_theme_service_fails_closed_without_configuration_or_windows() {
        let app = tauri::test::mock_builder()
            .manage(DesktopThemeService::new())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let handle = app.handle().clone();
        let service = app.state::<DesktopThemeService>();

        assert!(service.current(&handle, Some(1)).is_err());
        assert!(apply_to_controlled_windows(&handle, DesktopThemePreference::System).is_err());
        let directory = tempfile::tempdir().unwrap();
        *service.config_path.lock().unwrap() = Some(directory.path().join("appearance.json"));
        assert!(
            service
                .set(&handle, DesktopThemePreference::Quartz, Some(1))
                .is_err()
        );
        assert_eq!(
            *service.preference.lock().unwrap(),
            DesktopThemePreference::System
        );
    }

    #[test]
    fn poisoned_theme_state_never_exposes_an_unresolved_preference() {
        let app = managed_mock_app(Some(Theme::Light));
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
                .set(&handle, DesktopThemePreference::Onyx, Some(1))
                .is_err()
        );
    }

    #[test]
    fn poisoned_configuration_and_preference_locks_fail_initialization_closed() {
        let config_app = managed_mock_app(Some(Theme::Light));
        let config_service = config_app.state::<DesktopThemeService>();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _guard = config_service.config_path.lock().unwrap();
                panic!("test theme config lock poisoning");
            }))
            .is_err()
        );
        assert!(config_service.initialize(config_app.handle()).is_err());
        assert!(
            config_service
                .set(config_app.handle(), DesktopThemePreference::Atlas, Some(1))
                .is_err()
        );

        let preference_app = managed_mock_app(Some(Theme::Light));
        let preference_service = preference_app.state::<DesktopThemeService>();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _guard = preference_service.preference.lock().unwrap();
                panic!("test theme preference lock poisoning");
            }))
            .is_err()
        );
        assert!(
            preference_service
                .initialize(preference_app.handle())
                .is_err()
        );
    }

    #[test]
    fn native_configuration_write_failures_are_redacted() {
        assert!(write_preference(Path::new("/"), DesktopThemePreference::Atlas).is_err());

        let directory = tempfile::tempdir().unwrap();
        let blocking_file = directory.path().join("blocking-file");
        fs::write(&blocking_file, b"not a directory").unwrap();
        assert!(
            write_preference(
                &blocking_file.join("appearance.json"),
                DesktopThemePreference::Atlas
            )
            .is_err()
        );
        assert!(write_preference(directory.path(), DesktopThemePreference::Atlas).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn native_configuration_rejects_an_unwritable_directory() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let locked = directory.path().join("locked");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
        let result = write_preference(
            &locked.join("appearance.json"),
            DesktopThemePreference::Atlas,
        );
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(result.is_err());
    }
}
