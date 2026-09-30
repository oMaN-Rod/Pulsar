//! Machine-local folders; the roaming config lives in `config::default_path`.

use std::path::PathBuf;

/// `%LOCALAPPDATA%\Pulsar`: logs and the update check state.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Pulsar"))
}

pub fn logs_dir() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("logs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_live_under_the_data_folder() {
        let data = data_dir().expect("LOCALAPPDATA is set on Windows");
        assert!(data.ends_with("Pulsar"));
        assert_eq!(logs_dir(), Some(data.join("logs")));
    }
}
