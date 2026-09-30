//! The release check. `pulsar.exe --check-update` fetches GitHub's latest
//! release in its own short-lived process, so WinHTTP and TLS never stay
//! loaded in the resident app.

use std::ffi::c_void;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use pulsar_core::project::{API_HOST, LATEST_RELEASE_PATH};
use pulsar_core::update::{CHECK_ARG, EXIT_NO_RELEASE, Outcome, Version, interpret};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;
use windows::core::{HSTRING, PCWSTR, w};

use crate::messages::WM_APP_UPDATE;

const TIMEOUT_MS: i32 = 15_000;
const MAX_BODY: usize = 1024 * 1024;

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn handle(raw: *mut c_void) -> Result<Handle, String> {
    if raw.is_null() {
        Err(windows::core::Error::from_thread().message())
    } else {
        Ok(Handle(raw))
    }
}

/// GETs `https://{host}{path}`; returns the status code and the body.
fn fetch(host: &str, path: &str, user_agent: &str) -> Result<(u32, Vec<u8>), String> {
    let text = |e: windows::core::Error| e.message();
    unsafe {
        let session = handle(WinHttpOpen(
            &HSTRING::from(user_agent),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        WinHttpSetTimeouts(session.0, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS, TIMEOUT_MS)
            .map_err(text)?;
        let connection = handle(WinHttpConnect(
            session.0,
            &HSTRING::from(host),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        ))?;
        let request = handle(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        let headers: Vec<u16> =
            "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n"
                .encode_utf16()
                .collect();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).map_err(text)?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(text)?;
        let mut status = 0u32;
        let mut size = size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status as *mut u32).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(text)?;
        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                chunk.as_mut_ptr().cast(),
                chunk.len() as u32,
                &mut read,
            )
            .map_err(text)?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
            if body.len() > MAX_BODY {
                return Err("the response is too large".into());
            }
        }
        Ok((status, body))
    }
}

/// Entry point of `pulsar.exe --check-update`: prints the release JSON and
/// exits 0, exits `EXIT_NO_RELEASE` when there is none, or 1 with the reason
/// on stderr.
pub fn helper_main() -> i32 {
    let agent = format!("Pulsar/{}", env!("CARGO_PKG_VERSION"));
    match fetch(API_HOST, LATEST_RELEASE_PATH, &agent) {
        Ok((200, body)) => {
            let mut out = std::io::stdout();
            match out.write_all(&body).and_then(|()| out.flush()) {
                Ok(()) => 0,
                Err(_) => 1,
            }
        }
        Ok((404, _)) => EXIT_NO_RELEASE,
        Ok((status, _)) => {
            eprint!("HTTP {status}");
            1
        }
        Err(reason) => {
            eprint!("{reason}");
            1
        }
    }
}

fn run_helper() -> Outcome {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    match Command::new(exe)
        .arg(CHECK_ARG)
        .stdin(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW.0)
        .output()
    {
        Ok(out) => interpret(out.status.code(), &out.stdout, &out.stderr),
        Err(e) => Outcome::Failed(e.to_string()),
    }
}

/// Runs the helper on a background thread; `WM_APP_UPDATE` is posted to
/// `host` once the outcome is waiting in the returned channel.
pub fn spawn(host: HWND) -> Option<Receiver<Outcome>> {
    let (tx, rx) = mpsc::channel();
    let host = host.0 as isize;
    thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let _ = tx.send(run_helper());
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(host as *mut _)),
                    WM_APP_UPDATE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        })
        .ok()?;
    Some(rx)
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn current() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_running_version_is_comparable() {
        assert_eq!(current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn now_is_after_this_code_was_written() {
        assert!(now() > 1_790_000_000);
    }

    /// Needs the network; run with `cargo test -p pulsar-monitor -- --ignored`.
    #[test]
    #[ignore]
    fn fetch_reaches_github() {
        let (status, _) = fetch(API_HOST, LATEST_RELEASE_PATH, "Pulsar-test").unwrap();
        assert!(status == 200 || status == 404, "HTTP {status}");
    }
}
