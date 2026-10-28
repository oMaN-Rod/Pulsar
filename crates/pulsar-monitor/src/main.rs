#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod alerts;
mod app;
mod details;
mod drag;
mod fade;
mod game;
mod hover;
mod language;
mod launcher;
mod menu;
mod messages;
mod names;
mod overlay;
mod popup;
mod render;
mod sampler_thread;
mod taskbar;
mod text;
mod theme;
mod tray;
mod update;
mod widget;

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

use pulsar_core::ipc::APP_MUTEX;
use pulsar_core::single_instance::SingleInstance;
use pulsar_core::{crash, logging, paths};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> windows::core::Result<()> {
    if std::env::args().any(|a| a == pulsar_core::update::CHECK_ARG) {
        std::process::exit(update::helper_main());
    }
    if let Some(at) = std::env::args().position(|a| a == pulsar_core::diag::ARG) {
        std::process::exit(diagnostics(std::env::args().nth(at + 1)));
    }
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
    }
    let Some(_instance) = SingleInstance::acquire(APP_MUTEX) else {
        // Already running: a second launch opens Settings instead.
        let _ = launcher::open(launcher::Page::General);
        return Ok(());
    };
    if let Some(dir) = paths::logs_dir() {
        logging::init(&dir, "pulsar");
        crash::install_panic_hook(dir, "pulsar", VERSION);
    }
    log::info!("Pulsar {VERSION} started");
    let result = app::run();
    log::info!("Pulsar exited");
    result
}

/// `pulsar --diag [file]`: writes the diagnostics report and opens it.
fn diagnostics(path: Option<String>) -> i32 {
    let config = pulsar_core::config::default_path()
        .and_then(|p| pulsar_core::config::read(&p).ok())
        .unwrap_or_default();
    pulsar_core::format::set_units(config.units);
    let header = format!("Pulsar {VERSION} diagnostics");
    match pulsar_core::diag::save(&config, &header, path.as_deref().map(std::path::Path::new)) {
        Ok(path) => {
            menu::shell_open(&path.display().to_string());
            0
        }
        Err(_) => 1,
    }
}
