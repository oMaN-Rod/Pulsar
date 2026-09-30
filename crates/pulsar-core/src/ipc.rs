//! Names shared by `pulsar.exe` and `pulsar-settings.exe`.

use std::path::{Path, PathBuf};

/// Window class of the app's hidden host window, which receives `CONFIG_CHANGED`.
pub const HOST_CLASS: &str = "PulsarHost";
/// Registered window message the settings process posts after saving.
pub const CONFIG_CHANGED: &str = "Pulsar.ConfigChanged";
pub const APP_MUTEX: &str = r"Local\Pulsar.SingleInstance";
pub const SETTINGS_MUTEX: &str = r"Local\Pulsar.Settings";
pub const SETTINGS_TITLE: &str = "Pulsar Settings";
pub const APP_EXE: &str = "pulsar.exe";
pub const SETTINGS_EXE: &str = "pulsar-settings.exe";
/// Opens the settings window on its About tab.
pub const ABOUT_ARG: &str = "--about";

/// The two executables are installed side by side.
pub fn sibling(current_exe: &Path, name: &str) -> PathBuf {
    current_exe.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_replaces_the_file_name() {
        let exe = Path::new(r"C:\Users\me\AppData\Local\Programs\Pulsar\pulsar.exe");
        assert_eq!(
            sibling(exe, SETTINGS_EXE),
            Path::new(r"C:\Users\me\AppData\Local\Programs\Pulsar\pulsar-settings.exe")
        );
    }
}
