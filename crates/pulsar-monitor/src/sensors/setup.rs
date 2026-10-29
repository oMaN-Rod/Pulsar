//! Registers and removes the service. Both need an elevated caller.

use std::io::{self, Seek};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;
use std::{env, fs};

use pulsar_core::sensors::SERVICE_NAME;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::UI::Shell::{FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

const EXE: &str = "pulsar-sensors.exe";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// A stopping service holds its file open for a moment.
const FILE_RETRIES: u32 = 20;
const FILE_RETRY_DELAY: Duration = Duration::from_millis(250);

/// Asked of the shell rather than read from the environment, which the
/// unelevated process that launched this one controls.
fn program_files() -> io::Result<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_ProgramFiles, KF_FLAG_DEFAULT, None)
            .map_err(io::Error::other)?;
        let text = path.to_string();
        CoTaskMemFree(Some(path.0.cast()));
        text.map(PathBuf::from).map_err(io::Error::other)
    }
}

/// The service runs from a folder only administrators can write to, so
/// nothing unelevated can replace what runs as SYSTEM.
fn service_dir() -> io::Result<PathBuf> {
    Ok(program_files()?.join("Pulsar"))
}

fn sc(args: &[&str]) -> bool {
    let mut buffer = [0u16; 260];
    let len = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    let sc = PathBuf::from(String::from_utf16_lossy(&buffer[..len])).join("sc.exe");
    Command::new(sc)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .is_ok_and(|status| status.success())
}

fn retry(mut action: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut result = action();
    for _ in 0..FILE_RETRIES {
        if result.is_ok() {
            break;
        }
        sleep(FILE_RETRY_DELAY);
        result = action();
    }
    result
}

const SHARE_READ: u32 = 1;

fn open_own_image(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(path)
}

/// What the service control manager runs.
fn service_command(exe: &Path) -> String {
    format!("\"{}\" --service", exe.display())
}

pub fn install() -> io::Result<()> {
    let dir = service_dir()?;
    let target = dir.join(EXE);
    let source = env::current_exe()?;
    let mut image = open_own_image(&source)?;
    sc(&["stop", SERVICE_NAME]);
    fs::create_dir_all(&dir)?;
    if source != target {
        retry(|| {
            image.rewind()?;
            io::copy(&mut image, &mut fs::File::create(&target)?).map(drop)
        })?;
    }
    drop(image);
    let command = service_command(&target);
    let registered = sc(&[
        "create",
        SERVICE_NAME,
        "binPath=",
        &command,
        "start=",
        "auto",
        "DisplayName=",
        "Pulsar Sensors",
    ]) || sc(&[
        "config",
        SERVICE_NAME,
        "binPath=",
        &command,
        "start=",
        "auto",
    ]);
    if !registered {
        return Err(io::Error::other("could not register the service"));
    }
    sc(&[
        "description",
        SERVICE_NAME,
        "Reads the CPU temperature for the Pulsar taskbar monitor.",
    ]);
    if sc(&["start", SERVICE_NAME]) {
        Ok(())
    } else {
        Err(io::Error::other("could not start the service"))
    }
}

pub fn uninstall() -> io::Result<()> {
    sc(&["stop", SERVICE_NAME]);
    sc(&["delete", SERVICE_NAME]);
    let dir = service_dir()?;
    let target = dir.join(EXE);
    if target.exists() && env::current_exe()? != target {
        retry(|| fs::remove_file(&target))?;
    }
    let _ = fs::remove_dir(&dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_running_image_can_be_held_against_renames() {
        let exe = env::current_exe().unwrap();
        let _held = open_own_image(&exe).unwrap();
        let moved = exe.with_extension("moved");
        assert!(fs::rename(&exe, &moved).is_err());
    }

    #[test]
    fn the_service_path_is_quoted() {
        assert_eq!(
            service_command(Path::new(r"C:\Program Files\Pulsar\pulsar-sensors.exe")),
            r#""C:\Program Files\Pulsar\pulsar-sensors.exe" --service"#
        );
    }

    #[test]
    fn the_service_lives_under_program_files() {
        let dir = service_dir().unwrap();
        assert!(dir.ends_with("Pulsar"));
        assert!(dir.parent().unwrap().is_absolute());
    }
}
