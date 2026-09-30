<p align="center">
  <img src="crates/pulsar-monitor/assets/pulsar-256.png" width="96" alt="Pulsar icon">
</p>

<h1 align="center">Pulsar</h1>

<p align="center">A lightweight Windows 11 taskbar monitor for CPU, memory, disk, network, GPU and ping.</p>

---

Pulsar puts live graphs or compact text for your system's vital signs right on the Windows 11 taskbar, next to the clock. Rest the mouse on an item for a detailed popup.

## Features

- **Graphs or text.** Graph tiles with scrolling history, or a dense text layout that stays readable on a standard 48 px taskbar.
- **Labels your way.** Full labels (`CPU`), one-letter labels (`C`), icons, or icons alone.
- **Hover details.** Each item opens a popup:
  - **CPU:** per-core bars, clock speed and top processes
  - **Memory:** commit charge
  - **Disk:** free space per drive
  - **Network:** traffic per adapter
  - **GPU:** engine breakdown, VRAM, and temperature, fan and power
  - **Ping:** min/avg/max and packet loss
- **GPU monitoring for any vendor.** Utilisation matches Task Manager. Temperature is read the same way Task Manager does, with no drivers or vendor SDKs.
- **Per-drive disks and adapter choice.** Show each drive's activity or used space, and count one network adapter or all physical ones.
- **Stays put.** Survives Explorer restarts, taskbar moves, auto-hide, DPI changes and multiple monitors, and fades out while a fullscreen game or video is in front.
- **Floating mode.** Detach it from the taskbar, drag it anywhere, and lock it in place.
- **Looks right on your taskbar.**
  - theme presets
  - custom label, value and item colours
  - tile and panel backgrounds, which help with TranslucentTB and similar
  - any installed font, bold optional
- **Light.** About 10 MB of memory and well under 1% CPU. No admin rights, no drivers, no background services.

## Install

Pulsar needs Windows 11.

- **winget:**

  ```
  winget install oMaN-Rod.Pulsar
  ```

- **Installer:** download `pulsar-<version>-setup.exe` from [Releases](https://github.com/oMaN-Rod/Pulsar/releases). It installs for your user only (no admin prompt), adds a Start Menu entry, and can start Pulsar when you sign in. A portable zip is also on the Releases page.
- **Cargo:**

  ```
  cargo install pulsar-monitor
  ```

  This puts `pulsar.exe` and `pulsar-settings.exe` in `%USERPROFILE%\.cargo\bin`; run `pulsar` to start it. Building needs Rust 1.92+, the MSVC build tools and the Windows SDK. To remove it, turn off *Start with Windows* in Settings, exit Pulsar, then run `cargo uninstall pulsar-monitor`.

> **SmartScreen:** releases are not code-signed yet, so Windows may warn that the installer is from an unknown publisher. Choose **More info → Run anyway**. Signing through the SignPath Foundation is planned.

## Use

- **Tray icon:** left-click opens Settings; right-click opens the menu (graphs or text, labels, icons, hover details, hide in fullscreen, floating, Task Manager, exit).
- **Overlay:**
  - left-click opens Task Manager
  - right-click opens the same menu
  - rest the mouse on an item for details
  - in floating mode, drag it to move it
- Launching Pulsar again while it runs opens Settings.

## Settings and files

| What | Where |
|---|---|
| Settings (applied live) | `%APPDATA%\Pulsar\config.toml` |
| Logs and crash reports | `%LOCALAPPDATA%\Pulsar\logs` |
| A settings file that could not be read | backed up as `config.toml.bak`, and defaults are used |

## Privacy

Pulsar makes only two kinds of network request:

- **Ping:** pings the host you choose. Ping is off until you enable the Ping item.
- **Update check:** asks GitHub's releases API about new versions once a day. Turn it off in *Settings → General*. Nothing is ever downloaded or installed automatically.

There is no telemetry.

## Troubleshooting

If something looks wrong, open *Settings → About → Logs* and look at `pulsar.log`. If Pulsar crashes, it writes a `crash-*.txt` report there and offers to open it the next time it starts.

Please [open an issue](https://github.com/oMaN-Rod/Pulsar/issues/new/choose) with the log and any crash report attached.

## Build from source

```
cargo build --release
cargo test --workspace
```

`scripts\package.ps1` builds the installer, the portable zip and the winget manifests into `dist\`. It needs [Inno Setup](https://jrsoftware.org/isinfo.php). Also see the [manual testing checklist](docs/testing.md) and the [release guide](docs/releasing.md).

## License

[GPL-3.0-or-later](LICENSE). The settings window is built with [Slint](https://slint.dev), used under its GPLv3 option.
