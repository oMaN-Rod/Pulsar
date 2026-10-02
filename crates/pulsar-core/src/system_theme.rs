//! Theme settings Windows keeps in the registry.

use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::{PCWSTR, w};

fn read_dword(subkey: PCWSTR, value: PCWSTR) -> Option<u32> {
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some((&mut data as *mut u32).cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    Some(data)
}

/// The taskbar follows the "Windows mode" setting, not the app mode.
pub fn taskbar_is_light() -> bool {
    read_dword(
        w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        w!("SystemUsesLightTheme"),
    )
    .is_some_and(|v| v != 0)
}

/// DWM's accent colour as `0xAABBGGRR`.
pub fn accent_abgr() -> Option<u32> {
    read_dword(w!(r"Software\Microsoft\Windows\DWM"), w!("AccentColor"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_theme_from_registry_without_panicking() {
        let _ = taskbar_is_light();
        let _ = accent_abgr();
    }
}
