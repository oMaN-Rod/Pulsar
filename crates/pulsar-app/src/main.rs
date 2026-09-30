#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod display;
mod hover;
mod menu;
mod messages;
mod overlay;
mod popup;
mod render;
mod sampler_thread;
mod single_instance;
mod taskbar;
mod text;
mod theme;
mod tray;

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

use single_instance::SingleInstance;

fn main() -> windows::core::Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let Some(_instance) = SingleInstance::acquire(single_instance::NAME) else {
        return Ok(());
    };
    app::run()
}
