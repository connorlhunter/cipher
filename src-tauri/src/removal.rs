//! Platform-aware application removal handoff.
//!
//! The webview can request removal, but it cannot provide an executable path.
//! This module derives every path from the running installed application and
//! only starts a platform removal flow after that path has been validated.

use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use serde::{Deserialize, Serialize};

/// The bounded removal preference received from the desktop settings view.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopRemovalRequest {
    /// Whether Cipher's current native credentials and appearance preference are removed first.
    pub remove_local_data: bool,
}

/// A display-safe acknowledgement returned before Cipher closes.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRemovalView {
    /// Short, platform-neutral confirmation text.
    pub message: &'static str,
}

/// A removal operation could not be started safely.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopRemovalError {
    /// Cipher is running from a development location or unsupported layout.
    NotInstalled,
    /// The platform removal process could not be launched.
    Unavailable,
}

/// A validated installed application removal target.
#[derive(Debug, Eq, PartialEq)]
pub enum DesktopRemovalPlan {
    /// A macOS application bundle passed to Finder after Cipher exits.
    #[cfg(target_os = "macos")]
    MacosBundle(PathBuf),
    /// A Windows NSIS uninstaller started after Cipher exits.
    #[cfg(target_os = "windows")]
    WindowsUninstaller(PathBuf),
}

/// Validates the running application layout before native state is removed.
pub fn installed_removal_plan() -> Result<DesktopRemovalPlan, DesktopRemovalError> {
    let executable = env::current_exe().map_err(|_| DesktopRemovalError::NotInstalled)?;
    installed_removal_plan_for(&executable)
}

/// Starts the already-validated platform removal flow after Cipher has acknowledged the request.
pub fn schedule_removal(plan: DesktopRemovalPlan) -> Result<(), DesktopRemovalError> {
    #[cfg(target_os = "macos")]
    {
        let DesktopRemovalPlan::MacosBundle(bundle) = plan;
        Command::new("/usr/bin/osascript")
            .args([
                "-e",
                "on run argv\n  delay 1\n  tell application \"Finder\" to delete POSIX file (item 1 of argv)\nend run",
                "--",
            ])
            .arg(bundle)
            .spawn()
            .map_err(|_| DesktopRemovalError::Unavailable)?;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let DesktopRemovalPlan::WindowsUninstaller(uninstaller) = plan;
        Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 1; Start-Process -FilePath $env:CIPHER_UNINSTALLER -ArgumentList '/S' -Wait",
            ])
            .env("CIPHER_UNINSTALLER", uninstaller)
            .spawn()
            .map_err(|_| DesktopRemovalError::Unavailable)?;
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err(DesktopRemovalError::Unavailable)
}

fn installed_removal_plan_for(
    executable: &Path,
) -> Result<DesktopRemovalPlan, DesktopRemovalError> {
    #[cfg(target_os = "macos")]
    {
        let macos = executable
            .parent()
            .ok_or(DesktopRemovalError::NotInstalled)?;
        let contents = macos.parent().ok_or(DesktopRemovalError::NotInstalled)?;
        let bundle = contents.parent().ok_or(DesktopRemovalError::NotInstalled)?;
        if macos.file_name().and_then(|name| name.to_str()) != Some("MacOS")
            || contents.file_name().and_then(|name| name.to_str()) != Some("Contents")
            || bundle.extension().and_then(|extension| extension.to_str()) != Some("app")
            || !bundle.is_dir()
        {
            return Err(DesktopRemovalError::NotInstalled);
        }
        Ok(DesktopRemovalPlan::MacosBundle(bundle.to_path_buf()))
    }

    #[cfg(target_os = "windows")]
    {
        let directory = executable
            .parent()
            .ok_or(DesktopRemovalError::NotInstalled)?;
        let uninstaller = directory.join("uninstall.exe");
        if !uninstaller.is_file() {
            return Err(DesktopRemovalError::NotInstalled);
        }
        Ok(DesktopRemovalPlan::WindowsUninstaller(uninstaller))
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = executable;
        Err(DesktopRemovalError::NotInstalled)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::{DesktopRemovalError, DesktopRemovalPlan, installed_removal_plan_for};

    #[test]
    fn accepts_only_a_complete_macos_application_bundle() {
        let directory = tempdir().unwrap();
        let executable = directory.path().join("Cipher.app/Contents/MacOS/Cipher");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, []).unwrap();

        assert_eq!(
            installed_removal_plan_for(&executable),
            Ok(DesktopRemovalPlan::MacosBundle(
                directory.path().join("Cipher.app")
            ))
        );
        assert_eq!(
            installed_removal_plan_for(Path::new("/tmp/Cipher")),
            Err(DesktopRemovalError::NotInstalled)
        );
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{DesktopRemovalError, DesktopRemovalPlan, installed_removal_plan_for};

    #[test]
    fn accepts_only_an_installed_windows_uninstaller() {
        let directory = tempdir().unwrap();
        let executable = directory.path().join("Cipher.exe");
        fs::write(&executable, []).unwrap();
        assert_eq!(
            installed_removal_plan_for(&executable),
            Err(DesktopRemovalError::NotInstalled)
        );

        let uninstaller = directory.path().join("uninstall.exe");
        fs::write(&uninstaller, []).unwrap();
        assert_eq!(
            installed_removal_plan_for(&executable),
            Ok(DesktopRemovalPlan::WindowsUninstaller(uninstaller))
        );
    }
}
