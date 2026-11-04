use std::sync::atomic::{AtomicIsize, Ordering};

use pulsar_core::sensors::SERVICE_NAME;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Services::{
    RegisterServiceCtrlHandlerW, SERVICE_ACCEPT_SHUTDOWN, SERVICE_ACCEPT_STOP,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_RUNNING, SERVICE_STATUS,
    SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_HANDLE, SERVICE_STOPPED, SERVICE_TABLE_ENTRYW,
    SERVICE_WIN32_OWN_PROCESS, SetServiceStatus, StartServiceCtrlDispatcherW,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent};
use windows::core::{HSTRING, PWSTR};

use crate::reader;

static STOP: AtomicIsize = AtomicIsize::new(0);

/// Runs under the service control manager until it asks the service to stop.
pub fn run() -> i32 {
    let name = HSTRING::from(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(name.as_ptr().cast_mut()),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    match unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut PWSTR) {
    let Ok(stop) = (unsafe { CreateEventW(None, true, false, None) }) else {
        return;
    };
    // Before the handler exists, so a stop it receives is never lost.
    STOP.store(stop.0 as isize, Ordering::Release);
    let name = HSTRING::from(SERVICE_NAME);
    let Ok(handle) = (unsafe { RegisterServiceCtrlHandlerW(&name, Some(handler)) }) else {
        return;
    };
    report(handle, SERVICE_RUNNING);
    reader::run(stop);
    report(handle, SERVICE_STOPPED);
}

unsafe extern "system" fn handler(control: u32) {
    if control == SERVICE_CONTROL_STOP || control == SERVICE_CONTROL_SHUTDOWN {
        let stop = HANDLE(STOP.load(Ordering::Acquire) as *mut _);
        unsafe {
            let _ = SetEvent(stop);
        }
    }
}

fn report(handle: SERVICE_STATUS_HANDLE, state: SERVICE_STATUS_CURRENT_STATE) {
    let accepted = if state == SERVICE_RUNNING {
        SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN
    } else {
        0
    };
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepted,
        ..Default::default()
    };
    unsafe {
        let _ = SetServiceStatus(handle, &status);
    }
}
