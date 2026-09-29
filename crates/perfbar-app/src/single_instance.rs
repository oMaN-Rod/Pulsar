use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex};
use windows::core::HSTRING;

pub const NAME: &str = r"Local\PerfBar.SingleInstance";

/// Held for the lifetime of the process; a second instance fails to acquire it.
pub struct SingleInstance(HANDLE);

impl SingleInstance {
    pub fn acquire(name: &str) -> Option<Self> {
        let handle = unsafe { CreateMutexW(None, true, &HSTRING::from(name)) }.ok()?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return None;
        }
        Some(Self(handle))
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = ReleaseMutex(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_fails_until_first_is_dropped() {
        let name = format!(r"Local\PerfBar.Test.{}", std::process::id());
        let first = SingleInstance::acquire(&name).expect("first instance");
        assert!(SingleInstance::acquire(&name).is_none());
        drop(first);
        assert!(SingleInstance::acquire(&name).is_some());
    }
}
