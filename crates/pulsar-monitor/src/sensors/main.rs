//! The optional sensor service: the one part of Pulsar that runs elevated.
//! It reads the CPU temperature through the PawnIO driver and publishes it
//! for the unelevated app. It accepts no input from other processes.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod chipset;
mod cores;
mod gpu;
mod memory;
mod pawnio;
mod reader;
mod service;
mod setup;

use std::sync::atomic::AtomicBool;

fn main() {
    let code = match std::env::args().nth(1).as_deref() {
        Some("--service") => service::run(),
        // In the foreground, for trying it from an elevated terminal.
        Some("--run") => {
            reader::run(&AtomicBool::new(false));
            0
        }
        Some("--install") => exit_code(setup::install()),
        Some("--uninstall") => exit_code(setup::uninstall()),
        _ => 2,
    };
    std::process::exit(code);
}

fn exit_code(result: std::io::Result<()>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(e) => e.raw_os_error().unwrap_or(1),
    }
}
