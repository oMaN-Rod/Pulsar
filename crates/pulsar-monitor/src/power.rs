use windows::Win32::Foundation::{HANDLE, HWND, LPARAM};
use windows::Win32::System::Power::{
    GetSystemPowerStatus, POWERBROADCAST_SETTING, RegisterPowerSettingNotification,
    SYSTEM_POWER_STATUS,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTS_CURRENT_SERVER_HANDLE, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK,
    WTSDisconnected, WTSFreeMemory, WTSINFOEXW, WTSQuerySessionInformationW,
    WTSRegisterSessionNotification, WTSSessionInfoEx, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::SystemServices::{GUID_ACDC_POWER_SOURCE, GUID_SESSION_DISPLAY_STATUS};
use windows::Win32::System::Threading::{
    GetCurrentProcess, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    ProcessPowerThrottling, SetProcessInformation,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_WINDOW_HANDLE, WTS_CONSOLE_CONNECT, WTS_CONSOLE_DISCONNECT, WTS_REMOTE_CONNECT,
    WTS_REMOTE_DISCONNECT, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::core::{GUID, PWSTR};

const DISPLAY_OFF: u32 = 0;
const POWER_AC: u32 = 0;

/// Whether anyone can see the overlay, and what the machine is running on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Presence {
    display_off: bool,
    locked: bool,
    disconnected: bool,
    on_battery: bool,
}

impl Presence {
    pub const fn new() -> Self {
        Self {
            display_off: false,
            locked: false,
            disconnected: false,
            on_battery: false,
        }
    }

    pub fn current() -> Self {
        let mut status = SYSTEM_POWER_STATUS::default();
        let known = unsafe { GetSystemPowerStatus(&mut status) }.is_ok();
        let (locked, disconnected) = session_state();
        Self {
            locked,
            disconnected,
            on_battery: known && status.ACLineStatus == 0,
            ..Self::default()
        }
    }

    pub fn hidden(self) -> bool {
        self.display_off || self.locked || self.disconnected
    }

    pub fn on_battery(self) -> bool {
        self.on_battery
    }

    fn apply_power_setting(&mut self, setting: &GUID, value: u32) {
        if *setting == GUID_SESSION_DISPLAY_STATUS {
            self.display_off = value == DISPLAY_OFF;
        } else if *setting == GUID_ACDC_POWER_SOURCE {
            self.on_battery = value != POWER_AC;
        }
    }

    /// `lp` of a `PBT_POWERSETTINGCHANGE` message.
    pub fn on_power_setting(&mut self, lp: LPARAM) {
        let setting = unsafe { &*(lp.0 as *const POWERBROADCAST_SETTING) };
        if setting.DataLength >= 4 {
            let value = unsafe { setting.Data.as_ptr().cast::<u32>().read_unaligned() };
            self.apply_power_setting(&setting.PowerSetting, value);
        }
    }

    /// `wParam` of a `WM_WTSSESSION_CHANGE` message.
    pub fn on_session_change(&mut self, event: u32) {
        match event {
            WTS_SESSION_LOCK => self.locked = true,
            WTS_SESSION_UNLOCK => self.locked = false,
            WTS_CONSOLE_DISCONNECT | WTS_REMOTE_DISCONNECT => self.disconnected = true,
            WTS_CONSOLE_CONNECT | WTS_REMOTE_CONNECT => self.disconnected = false,
            _ => {}
        }
    }
}

/// Whether this session is locked and whether it is disconnected.
fn session_state() -> (bool, bool) {
    let mut buffer = PWSTR::null();
    let mut bytes = 0;
    let queried = unsafe {
        WTSQuerySessionInformationW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )
    };
    if queried.is_err() || buffer.is_null() {
        return (false, false);
    }
    let info = unsafe { &*buffer.0.cast::<WTSINFOEXW>() };
    let state = (info.Level == 1).then(|| {
        let session = unsafe { info.Data.WTSInfoExLevel1 };
        (
            session.SessionFlags as u32 == WTS_SESSIONSTATE_LOCK,
            session.SessionState == WTSDisconnected,
        )
    });
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    state.unwrap_or((false, false))
}

pub fn register(hwnd: HWND) {
    for setting in [&GUID_SESSION_DISPLAY_STATUS, &GUID_ACDC_POWER_SOURCE] {
        let handle = HANDLE(hwnd.0);
        if let Err(e) = unsafe {
            RegisterPowerSettingNotification(handle, setting, DEVICE_NOTIFY_WINDOW_HANDLE)
        } {
            log::warn!("Power notifications unavailable: {e}");
        }
    }
    if let Err(e) = unsafe { WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) } {
        log::warn!("Session notifications unavailable: {e}");
    }
}

pub fn unregister(hwnd: HWND) {
    let _ = unsafe { WTSUnRegisterSessionNotification(hwnd) };
}

/// Asks Windows for EcoQoS: efficient cores and lower clocks for this process.
pub fn prefer_efficiency() {
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    };
    let applied = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            std::ptr::from_ref(&state).cast(),
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        )
    };
    if let Err(e) = applied {
        log::warn!("Efficiency mode unavailable: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Threading::GetProcessInformation;

    #[test]
    fn efficiency_mode_reaches_the_process() {
        prefer_efficiency();
        let mut state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ..Default::default()
        };
        unsafe {
            GetProcessInformation(
                GetCurrentProcess(),
                ProcessPowerThrottling,
                std::ptr::from_mut(&mut state).cast(),
                size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        }
        .unwrap();
        assert_eq!(state.StateMask, PROCESS_POWER_THROTTLING_EXECUTION_SPEED);
    }

    #[test]
    fn dimming_still_counts_as_visible() {
        let mut p = Presence::default();
        p.apply_power_setting(&GUID_SESSION_DISPLAY_STATUS, DISPLAY_OFF);
        assert!(p.hidden());
        p.apply_power_setting(&GUID_SESSION_DISPLAY_STATUS, 2);
        assert!(!p.hidden());
        p.apply_power_setting(&GUID_SESSION_DISPLAY_STATUS, 1);
        assert!(!p.hidden());
    }

    #[test]
    fn a_locked_session_stays_hidden_until_unlocked_and_the_display_is_on() {
        let mut p = Presence::default();
        p.on_session_change(WTS_SESSION_LOCK);
        p.apply_power_setting(&GUID_SESSION_DISPLAY_STATUS, DISPLAY_OFF);
        p.apply_power_setting(&GUID_SESSION_DISPLAY_STATUS, 1);
        assert!(p.hidden());
        p.on_session_change(WTS_SESSION_UNLOCK);
        assert!(!p.hidden());
    }

    #[test]
    fn a_disconnected_session_is_hidden() {
        let mut p = Presence::default();
        p.on_session_change(WTS_REMOTE_DISCONNECT);
        assert!(p.hidden());
        p.on_session_change(WTS_REMOTE_CONNECT);
        assert!(!p.hidden());
    }

    #[test]
    fn anything_but_mains_power_counts_as_battery() {
        let mut p = Presence::default();
        p.apply_power_setting(&GUID_ACDC_POWER_SOURCE, 1);
        assert!(p.on_battery());
        p.apply_power_setting(&GUID_ACDC_POWER_SOURCE, POWER_AC);
        assert!(!p.on_battery());
        assert!(!p.hidden());
    }
}
