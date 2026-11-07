//! Entries in the Application event log, which records what the service
//! saw without it writing to any folder an unelevated user could redirect.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use pulsar_core::sensors::SERVICE_NAME;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::EventLog::{
    DeregisterEventSource, EVENTLOG_INFORMATION_TYPE, EVENTLOG_WARNING_TYPE, REPORT_EVENT_TYPE,
    RegisterEventSourceW, ReportEventW,
};
use windows::Win32::System::Registry::{
    HKEY_LOCAL_MACHINE, REG_DWORD, REG_EXPAND_SZ, RegDeleteTreeW, RegSetKeyValueW,
};
use windows::core::{HSTRING, PCWSTR};

const SOURCE_KEY: &str = r"SYSTEM\CurrentControlSet\Services\EventLog\Application\PulsarSensors";
/// Its messages for IDs 1 to 1000 are the inserted text alone.
const MESSAGE_FILE: &str = r"%SystemRoot%\System32\EventCreate.exe";
const ERROR_WARNING_INFORMATION: u32 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    TraceSilent = 1,
    SilentReaderReturned = 2,
    ReadersFull = 3,
}

impl Event {
    fn kind(self) -> REPORT_EVENT_TYPE {
        match self {
            Event::TraceSilent | Event::ReadersFull => EVENTLOG_WARNING_TYPE,
            Event::SilentReaderReturned => EVENTLOG_INFORMATION_TYPE,
        }
    }
}

pub fn register() {
    let message_file: Vec<u16> = MESSAGE_FILE.encode_utf16().chain([0]).collect();
    let types = ERROR_WARNING_INFORMATION;
    let key = HSTRING::from(SOURCE_KEY);
    unsafe {
        let _ = RegSetKeyValueW(
            HKEY_LOCAL_MACHINE,
            &key,
            &HSTRING::from("EventMessageFile"),
            REG_EXPAND_SZ.0,
            Some(message_file.as_ptr().cast()),
            (message_file.len() * 2) as u32,
        );
        let _ = RegSetKeyValueW(
            HKEY_LOCAL_MACHINE,
            &key,
            &HSTRING::from("TypesSupported"),
            REG_DWORD.0,
            Some((&raw const types).cast()),
            4,
        );
    }
}

pub fn unregister() {
    unsafe {
        let _ = RegDeleteTreeW(HKEY_LOCAL_MACHINE, &HSTRING::from(SOURCE_KEY));
    }
}

const QUEUED: usize = 16;

fn writer() -> Option<SyncSender<(Event, String)>> {
    let (sender, entries) = sync_channel(QUEUED);
    std::thread::Builder::new()
        .name("event log".into())
        .spawn(move || write(&entries))
        .ok()
        .map(|_| sender)
}

struct Source(HANDLE);

impl Drop for Source {
    fn drop(&mut self) {
        unsafe {
            let _ = DeregisterEventSource(self.0);
        }
    }
}

/// On a thread of its own, as the event log may be slow.
fn write(entries: &Receiver<(Event, String)>) {
    let mut source: Option<Source> = None;
    for (event, text) in entries {
        if source.is_none() {
            source = unsafe { RegisterEventSourceW(PCWSTR::null(), &HSTRING::from(SERVICE_NAME)) }
                .ok()
                .map(Source);
        }
        let Some(Source(source)) = source else {
            continue;
        };
        let text = HSTRING::from(text);
        unsafe {
            let _ = ReportEventW(
                source,
                event.kind(),
                0,
                event as u32,
                None,
                0,
                Some(&[PCWSTR(text.as_ptr())]),
                None,
            );
        }
    }
}

pub fn report(event: Event, text: String) {
    static QUEUE: Mutex<Option<SyncSender<(Event, String)>>> = Mutex::new(None);
    let Ok(mut queue) = QUEUE.lock() else {
        return;
    };
    if queue.is_none() {
        *queue = writer();
    }
    if let Some(sender) = queue.as_ref()
        && let Err(TrySendError::Disconnected(_)) = sender.try_send((event, text))
    {
        *queue = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_is_named_after_the_service() {
        assert!(SOURCE_KEY.ends_with(&format!(r"\Application\{SERVICE_NAME}")));
    }
}
