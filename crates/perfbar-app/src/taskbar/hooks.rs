//! WinEvent hooks that tell the host window when the taskbar moves or the
//! foreground window changes. Callbacks only post a message; a pending flag
//! coalesces bursts (e.g. a taskbar slide animation) into one repositioning.

use std::cell::RefCell;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::messages::{WM_APP_FOREGROUND, WM_APP_TASKBAR};

const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
const WINEVENT_OUTOFCONTEXT: u32 = 0;
const OBJID_WINDOW: i32 = 0;

#[derive(Default)]
struct State {
    host: Option<HWND>,
    watched: Vec<HWND>,
    pending: Vec<u32>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub struct Hooks {
    handles: Vec<HWINEVENTHOOK>,
}

impl Hooks {
    /// Location changes are only hooked for Explorer's process, which keeps
    /// the callback volume low; foreground changes come from any process.
    pub fn install(host: HWND, explorer_pid: u32) -> Self {
        STATE.with_borrow_mut(|s| s.host = Some(host));
        let hook = |event: u32, pid: u32| unsafe {
            SetWinEventHook(
                event,
                event,
                None,
                Some(on_event),
                pid,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        };
        let handles = vec![
            hook(EVENT_SYSTEM_FOREGROUND, 0),
            hook(EVENT_OBJECT_LOCATIONCHANGE, explorer_pid),
        ];
        Self {
            handles: handles.into_iter().filter(|h| !h.is_invalid()).collect(),
        }
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        for &h in &self.handles {
            unsafe {
                let _ = UnhookWinEvent(h);
            }
        }
    }
}

/// Windows whose movement should trigger repositioning (taskbars and trays).
pub fn watch(windows: Vec<HWND>) {
    STATE.with_borrow_mut(|s| s.watched = windows);
}

/// Called by the host when it handles `message`, re-arming the coalescing flag.
pub fn handled(message: u32) {
    STATE.with_borrow_mut(|s| s.pending.retain(|&m| m != message));
}

fn post(message: u32) {
    STATE.with_borrow_mut(|s| {
        let Some(host) = s.host else { return };
        if s.pending.contains(&message) {
            return;
        }
        if unsafe { PostMessageW(Some(host), message, WPARAM(0), LPARAM(0)) }.is_ok() {
            s.pending.push(message);
        }
    });
}

unsafe extern "system" fn on_event(
    _: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    match event {
        EVENT_SYSTEM_FOREGROUND => post(WM_APP_FOREGROUND),
        EVENT_OBJECT_LOCATIONCHANGE if id_object == OBJID_WINDOW => {
            let watched = STATE.with_borrow(|s| s.watched.contains(&hwnd));
            if watched {
                post(WM_APP_TASKBAR);
            }
        }
        _ => {}
    }
}
