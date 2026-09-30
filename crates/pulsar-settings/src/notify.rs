use windows::Win32::Foundation::{CloseHandle, HANDLE, LPARAM, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::Threading::{
    CreateEventW, EVENT_MODIFY_STATE, OpenEventW, SetEvent, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, PostMessageW, RegisterWindowMessageW, SW_RESTORE, SetForegroundWindow,
    ShowWindow,
};
use windows::core::{HSTRING, PCWSTR};

/// Posts `message` to the window of class `class`; false when none is running.
pub fn post_to_class(class: &str, message: &str) -> bool {
    unsafe {
        let id = RegisterWindowMessageW(&HSTRING::from(message));
        match FindWindowW(&HSTRING::from(class), PCWSTR::null()) {
            Ok(hwnd) => PostMessageW(Some(hwnd), id, WPARAM(0), LPARAM(0)).is_ok(),
            Err(_) => false,
        }
    }
}

/// Brings an already open window with this title to the front.
pub fn focus_window(title: &str) -> bool {
    unsafe {
        let Ok(hwnd) = FindWindowW(PCWSTR::null(), &HSTRING::from(title)) else {
            return false;
        };
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd).as_bool()
    }
}

/// A named, auto-resetting event another process can set to ask the running
/// settings window for something (e.g. to show the About page).
pub struct Request(HANDLE);

impl Request {
    pub fn create(name: &str) -> Option<Self> {
        unsafe { CreateEventW(None, false, false, &HSTRING::from(name)) }
            .ok()
            .map(Self)
    }

    /// True once per `signal`.
    pub fn take(&self) -> bool {
        (unsafe { WaitForSingleObject(self.0, 0) }) == WAIT_OBJECT_0
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Sets the named request; false when no process is listening.
pub fn signal(name: &str) -> bool {
    unsafe {
        let Ok(event) = OpenEventW(EVENT_MODIFY_STATE, false, &HSTRING::from(name)) else {
            return false;
        };
        let set = SetEvent(event).is_ok();
        let _ = CloseHandle(event);
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{HWND, LRESULT};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW,
        RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW,
    };

    unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    #[test]
    fn posts_the_registered_message_to_the_class_window() {
        // A private class, so a running Pulsar is never notified by tests.
        let class = format!("PulsarHostTest{}", std::process::id());
        let message = format!("Pulsar.Test.{}", std::process::id());
        let class_w = HSTRING::from(class.as_str());
        unsafe {
            let instance = GetModuleHandleW(None).unwrap();
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: PCWSTR(class_w.as_ptr()),
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                &class_w,
                &HSTRING::from("test"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .unwrap();

            assert!(post_to_class(&class, &message));
            let id = RegisterWindowMessageW(&HSTRING::from(message.as_str()));
            let mut msg = MSG::default();
            assert!(PeekMessageW(&mut msg, Some(hwnd), id, id, PM_REMOVE).as_bool());
            let _ = DestroyWindow(hwnd);
        }
        assert!(
            !post_to_class(&class, &message),
            "no window after it is destroyed"
        );
    }

    #[test]
    fn a_signalled_request_is_taken_once() {
        let name = format!(r"Local\PulsarTest.ShowAbout.{}", std::process::id());
        let request = Request::create(&name).unwrap();
        assert!(!request.take());
        assert!(signal(&name));
        assert!(request.take());
        assert!(!request.take(), "resets after being taken");
    }

    #[test]
    fn signalling_without_a_listener_is_false() {
        assert!(!signal(r"Local\PulsarTest.NobodyListening"));
    }

    #[test]
    fn focusing_a_missing_window_is_false() {
        assert!(!focus_window("Pulsar window that does not exist"));
    }
}
