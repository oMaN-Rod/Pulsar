<p align="center">
  <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/crates/pulsar-monitor/assets/pulsar-256.png" width="96" alt="Pulsar icon">
</p>

<h1 align="center">Pulsar</h1>

<p align="center">A lightweight Windows 11 taskbar monitor for CPU, memory, disk, network, GPU and ping.</p>

---

Pulsar puts live graphs or compact text for your system's vital signs right on the Windows 11 taskbar. Place it beside the tray or at the left edge, nudge it with an offset, or let it float anywhere on screen. Rest the mouse on an item for a detailed popup.

## See it

**Graph mode:** scrolling history for every item, here beside the tray.

![Graph mode](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/graph.gif)

**Hover for details:** CPU cores and top processes, memory, GPU engines and sensors, drives, network adapters and ping statistics.

![Hover popups](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/hover.gif)

**Text mode and label styles:** full labels, one-letter labels, icons with labels, and icons alone.

![Text mode label styles](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/labels.gif)

**Floating:** drag it anywhere, here with a panel background, in graph and text mode.

![Floating mode](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/floating.gif)

## Features

- **Graphs or text.** Graph tiles with scrolling history, or a dense text layout that stays readable on a standard 48 px taskbar. On a shorter taskbar, graph tiles can put the icon, label and value on one line. Turn off the graph on any item, such as network, to show just its text.
- **Labels your way.** Full labels (`CPU`), one-letter labels (`C`), icons, or icons alone.
- **Hover details.** Each item opens a popup:
  - **CPU:** per-core bars, clock speed and top processes
  - **CPU clock:** average and fastest core, with per-core bars
  - **Memory:** commit charge
  - **Disk:** free space per drive
  - **Network:** traffic per adapter
  - **GPU:** engine breakdown, VRAM, and temperature, fan and power
  - **Drive temperature:** each drive that reports one
  - **Ping:** min/avg/max and packet loss
- **A full system details window.** Ctrl+click the overlay for a CPU-Z-style view of the whole machine: processor (microarchitecture, cores, caches, instruction sets), memory modules, graphics cards with their PCIe link, drives with their temperatures, network adapters, motherboard, chipset and BIOS, plus every live sensor with its low and high. The hover graphs are there too, with their full history.
- **CPU and chipset temperature and CPU power, if you opt in.** Windows gives unelevated programs no real CPU temperature or power draw, so these need the optional [sensor service](#sensor-service), which also adds a few extra CPU, memory and GPU readings.
- **GPU monitoring for any vendor.** Utilisation matches Task Manager. Temperature is read the same way Task Manager does, with no drivers or vendor SDKs.
- **Per-drive disks and adapter choice.** Show each drive's activity or used space, and count one network adapter or all physical ones.
- **Stays put.** Survives Explorer restarts, taskbar moves, auto-hide, DPI changes and multiple monitors, and fades out while a fullscreen game or video is in front.
- **Floating mode.** Detach it from the taskbar, drag it anywhere, and lock it in place.
- **Looks right on your taskbar.**
  - theme presets
  - custom label, value and item colours
  - tile and panel backgrounds, which help with TranslucentTB and similar
  - any installed font, bold optional
- **Light.** About 10 MB of memory and well under 1% CPU. No admin rights, no drivers and no background services, unless you opt in to the [sensor service](#sensor-service).

## Install

Pulsar needs Windows 11.

- **winget:**

  ```
  winget install oMaN-Rod.Pulsar
  ```

- **Installer:** download `pulsar-<version>-setup.exe` from [Releases](https://github.com/oMaN-Rod/Pulsar/releases). It installs for your user only (no admin prompt), adds a Start Menu entry, and can start Pulsar when you sign in. A portable zip is also on the Releases page.
- **Cargo:**

  ```
  cargo install --locked pulsar-monitor
  ```

  This puts `pulsar.exe`, `pulsar-settings.exe`, `pulsar-details.exe` and `pulsar-sensors.exe` in `%USERPROFILE%\.cargo\bin`; run `pulsar` to start it. Building needs Rust 1.92+, the MSVC build tools and the Windows SDK. To remove it, turn off *Start with Windows* in Settings, exit Pulsar, then run `cargo uninstall pulsar-monitor`. If you set up the [sensor service](#sensor-service), remove it first.

> **SmartScreen:** releases are not code-signed yet, so Windows may warn that the installer is from an unknown publisher. Choose **More info → Run anyway**. Signing through the SignPath Foundation is planned.

## Use

- **Tray icon:** left-click opens Settings; right-click opens the menu (system details, graphs or text, labels, icons, hover details, hide in fullscreen, floating, Task Manager, exit).
- **Overlay:**
  - left-click opens Task Manager
  - Ctrl+click opens the system details window on the clicked item's page
  - right-click opens the same menu
  - rest the mouse on an item for details
  - in floating mode, drag it to move it
- Launching Pulsar again while it runs opens Settings.
- The tray icon can be turned off in Settings → General; it then appears only while a notification is showing.

## Sensor service

Everything Pulsar shows by default is read without administrator rights. A few readings need kernel access, which Pulsar does not have and does not ask for by default. Two optional pieces provide them, and both need administrator rights:

- **[PawnIO](https://pawnio.eu)** ([source](https://github.com/namazso/PawnIO)), an open-source signed driver by namazso. It only runs modules signed by its author, and the ones Pulsar uses can read a fixed list of registers.
- **The Pulsar sensor service** (`pulsar-sensors.exe`), which reads temperatures and power draw through PawnIO once a second and publishes those numbers. It runs as a Windows service from `%ProgramFiles%\Pulsar` and takes no input from other programs. Pulsar itself stays unelevated.

| Reading | Built in | With the sensor service |
|---|---|---|
| CPU usage, clock speed (item and popup), per-core load, top processes | ✓ | |
| Memory, disk, network and ping | ✓ | |
| Drive temperature (item and popup), for drives that report one | ✓ | |
| GPU board maker and PCIe link, chipset model (details window) | ✓ | |
| GPU usage, VRAM, temperature, fan and power | ✓ | |
| CPU temperature (item and popup) | | Intel, and AMD Ryzen (Zen and later) |
| CPU power (item and popup) | | Intel, and AMD Ryzen (Zen and later) |
| Chipset temperature (item and popup) | | Intel, chipsets from the 8 series on |
| TDP, power limits, multiplier range and ring clock (details window) | | Intel |
| Memory timings and clocks (details window) | | Intel 12th–14th gen; AMD Ryzen without the controller clock |
| Hottest core (popup) | | Intel |
| Per-core temperature (details window) | | Intel |
| Per-core clock (details window) | | Intel, and AMD Ryzen |
| Per-core voltage (details window) | | Intel |
| Per-core power (details window) | | AMD Ryzen |
| Cores, integrated graphics and memory power (details window) | | Intel; cores only on AMD Ryzen |
| Thermal throttling (popup) | | Intel |
| GPU hot spot (popup) | | NVIDIA RTX 50 series |
| GPU memory temperature (popup) | | NVIDIA RTX 30 series and later |

The GPU rows appear only where the card has those sensors, and they need one of the supported CPUs.

How to set it up depends on how you installed Pulsar:

- **Installer:** tick the boxes on its *CPU temperature* page. Run the installer again to add or remove them later.
- **winget:** `winget install` runs the installer silently, which never asks for administrator rights and so skips that page. Run `winget install -i oMaN-Rod.Pulsar` to see it, or follow the next step using `%LOCALAPPDATA%\Programs\Pulsar\pulsar-sensors.exe`.
- **Portable zip or Cargo:** install PawnIO (`winget install namazso.PawnIO`), then run `pulsar-sensors.exe --install` from an administrator terminal.

Then turn on *CPU temperature*, *CPU power* or *Chipset temperature* in Settings → Items.

The service runs from its own copy of `pulsar-sensors.exe`. The installer's wizard refreshes that copy, but a silent upgrade (`winget upgrade`) and `cargo install` do not, so run `pulsar-sensors.exe --install` again after those if Pulsar says the service needs updating.

To remove the service, run `pulsar-sensors.exe --uninstall` from an administrator terminal. Uninstalling Pulsar from Windows Settings or winget does this for you; `cargo uninstall` and deleting the portable folder do not.

Without the service, the *CPU temperature*, *CPU power* and *Chipset temperature* items show "—" and their popup says what is missing; the other rows are simply left out.

### The details window with the service

**Overview:** a summary of the machine, live graphs for every item, and temperatures, power and up time.

![Details window overview](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/Overview.png)

**Processor:** per-core load, clock and temperature as a matrix, strips, rings or a graph per thread, with power, voltage, caches and instruction sets.

![Details window processor page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/processor.gif)

**Graphics:** utilisation, temperature, engines, graphics memory and every adapter.

![Details window graphics page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/graphics.png)

**Storage:** activity, transfer rate, and each drive's temperature, limits and free space.

![Details window storage page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/storage.png)

**Network:** traffic history and every adapter's link speed and addresses.

![Details window network page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/network.png)

**System:** motherboard, BIOS and Windows.

![Details window system page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/system.png)

**Sensors:** every reading in a collapsible tree, with its current value, low and high.

![Details window sensors page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/sensors.gif)

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

`scripts\package.ps1` builds the installer, the portable zip and the winget manifests into `dist\`. It needs [Inno Setup](https://jrsoftware.org/isinfo.php) 6.3 or later. Also see the [manual testing checklist](https://github.com/oMaN-Rod/Pulsar/blob/main/docs/testing.md) and the [release guide](https://github.com/oMaN-Rod/Pulsar/blob/main/docs/releasing.md).

## License

[GPL-3.0-or-later](https://github.com/oMaN-Rod/Pulsar/blob/main/LICENSE). The settings window is built with [Slint](https://slint.dev), used under its GPLv3 option.
