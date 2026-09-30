//! Start-with-Windows via the per-user `Run` registry key (no admin needed).

use std::path::Path;

use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};
use windows::core::{HSTRING, Result};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const VALUE_NAME: &str = "Pulsar";

/// The command stored under `value_name`, if any.
pub fn get(value_name: &str) -> Option<String> {
    let key = HSTRING::from(RUN_KEY);
    let name = HSTRING::from(value_name);
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &name,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// The quoted command line stored in the `Run` value.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Writes `command` under `value_name`, or removes the value when `None`.
pub fn set(value_name: &str, command: Option<&str>) -> Result<()> {
    let key = HSTRING::from(RUN_KEY);
    let name = HSTRING::from(value_name);
    match command {
        Some(command) => {
            let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    &key,
                    &name,
                    REG_SZ.0,
                    Some(wide.as_ptr().cast()),
                    (wide.len() * 2) as u32,
                )
            }
            .ok()
        }
        None => {
            let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, &key, &name) };
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            status.ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_is_quoted() {
        let exe = Path::new(r"C:\Program Files\Pulsar\pulsar.exe");
        assert_eq!(command_for(exe), r#""C:\Program Files\Pulsar\pulsar.exe""#);
    }

    #[test]
    fn set_get_and_remove_a_value() {
        // A throwaway value name so the real Pulsar entry is never touched.
        let name = format!("PulsarTest{}", std::process::id());
        set(&name, Some(r#""C:\x\pulsar.exe""#)).unwrap();
        assert_eq!(get(&name).as_deref(), Some(r#""C:\x\pulsar.exe""#));
        set(&name, None).unwrap();
        assert_eq!(get(&name), None);
        set(&name, None).unwrap();
    }
}
