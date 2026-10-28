<p align="center">
  <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/crates/pulsar-monitor/assets/pulsar-256.png" width="96" alt="Pulsar icon">
</p>

<h1 align="center">Pulsar</h1>

<p align="center">A lightweight Windows 11 taskbar monitor for CPU, memory, disk, network, GPU, temperatures and ping.</p>

<p align="center">
  <a href="https://github.com/oMaN-Rod/Pulsar/releases/latest"><img src="https://img.shields.io/github/v/release/oMaN-Rod/Pulsar?label=release" alt="Latest release"></a>
  <a href="https://github.com/oMaN-Rod/Pulsar/releases"><img src="https://img.shields.io/github/downloads/oMaN-Rod/Pulsar/total" alt="Downloads"></a>
  <a href="https://crates.io/crates/pulsar-monitor"><img src="https://img.shields.io/crates/v/pulsar-monitor" alt="crates.io"></a>
  <img src="https://img.shields.io/badge/Windows-11-0078D4" alt="Windows 11">
  <a href="https://github.com/oMaN-Rod/Pulsar/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License: GPL-3.0-or-later"></a>
</p>

<p align="center">
  <a href="#install">Install</a> ·
  <a href="#the-overlay">Overlay</a> ·
  <a href="#popups">Popups</a> ·
  <a href="#system-details-window">Details window</a> ·
  <a href="#using-pulsar">Use</a> ·
  <a href="#sensor-service">Sensor service</a> ·
  <a href="#troubleshooting">Troubleshooting</a>
</p>

---

Pulsar puts live graphs or compact text for your system's vital signs right on the Windows 11 taskbar. Rest the mouse on an item for a detailed popup, or open a full view of the whole machine.

![Pulsar beside the tray in graph mode](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/graph.gif)

## Highlights

- **Graphs or text**, with a graph style, colour and popup detail chosen per item.
- **Hover popups** at three levels of detail, with a translucent background that blurs what is behind them.
- **A system details window**: processor, memory modules, graphics, drives, network, motherboard and every live sensor.
- **GPU monitoring for any vendor**, read the way Task Manager does, with no drivers or vendor SDKs.
- **CPU temperature and power, if you opt in**, through an optional [sensor service](#sensor-service).
- **Light**: about 10 MB of memory and well under 1% CPU. No admin rights, no drivers and no background services unless you add the sensor service.
- **No telemetry.** See [Privacy](#privacy).

## Install

Pulsar needs Windows 11.

| Method | How |
|---|---|
| **winget** | `winget install oMaN-Rod.Pulsar` |
| **Installer** | Download `pulsar-<version>-setup.exe` from [Releases](https://github.com/oMaN-Rod/Pulsar/releases). It installs for your user only, adds a Start Menu entry, and can start Pulsar when you sign in. |
| **Portable** | Download the zip from [Releases](https://github.com/oMaN-Rod/Pulsar/releases), extract it anywhere and run `pulsar.exe`. |
| **Cargo** | `cargo install --locked pulsar-monitor`, then run `pulsar`. |

> [!WARNING]
> Releases are not code-signed yet, so SmartScreen may warn that the installer is from an unknown publisher. Choose **More info → Run anyway**. Signing through the SignPath Foundation is planned.

<details>
<summary><b>More about installing with Cargo</b></summary>

`cargo install` puts `pulsar.exe`, `pulsar-settings.exe`, `pulsar-details.exe` and `pulsar-sensors.exe` in `%USERPROFILE%\.cargo\bin`. Building needs Rust 1.92+, the MSVC build tools and the Windows SDK.

To remove it, turn off *Start with Windows* in Settings, exit Pulsar, then run `cargo uninstall pulsar-monitor`. If you set up the [sensor service](#sensor-service), remove it first.

</details>

## The overlay

Pulsar sits beside the tray or at the left edge of the taskbar, nudged by an offset if you like, or floats anywhere on screen. It survives Explorer restarts, taskbar moves, auto-hide, DPI changes and multiple monitors, and fades out while a fullscreen game or video is in front.

### Graphs or text

| | |
|---|---|
| **Graphs** | ![Graph mode](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-graph.png) |
| **Graphs with icons** | ![Graph mode with icons](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-graph-icons.png) |
| **One-letter labels** | ![Graph mode with one-letter labels](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-graph-short.png) |
| **Single-line graphs**, for short taskbars | ![Single-line graphs](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-graph-inline.png) |
| **Text** | ![Text mode](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-text.png) |
| **Text, one-letter labels** | ![Text mode with one-letter labels](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-text-short.png) |
| **Text, icons alone** | ![Text mode with icons alone](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-text-icons.png) |

### A style and colour per item

Each item draws an area graph, a line, bars, a level meter, or just its text. It can use its own colour, your Windows accent colour, or a colour that turns green, yellow, orange and red as its load rises.

| | |
|---|---|
| **Area** | ![Area graphs](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-graph.png) |
| **Line** | ![Line graphs](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-style-line.png) |
| **Bars** | ![Bar graphs](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-style-bars.png) |
| **Level meter** | ![Level meters](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-style-meter.png) |
| **Mixed**: area, line, meter, bars and text only | ![A different style per item](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-style-mixed.png) |
| **Colour by load** | ![Graphs coloured by load](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-color-load.png) |
| **Colour by load, text** | ![Text coloured by load](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-text-load.png) |

> [!TIP]
> Right-click an item on the taskbar to change its style, colour and popup detail, swap it with its neighbour, or hide it.

### Every item

CPU, CPU temperature, CPU power, CPU clock, memory, chipset temperature, GPU, GPU temperature, disk, drive temperature, network, ping and battery. Show any of them, in any order.

![Every item at once](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/overlay-all.png)

### Floating

Detach Pulsar from the taskbar, drag it anywhere and lock it in place, here with a panel background.

![Floating mode](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/floating.gif)

### Make it yours

- theme presets
- label, value and item colours, with separate label and value colours for a light taskbar
- tile and panel backgrounds, which help on TranslucentTB and similar
- any installed font, bold optional
- temperatures in °C or °F, network rates in bytes or bits per second, and upload shown above download if you prefer
- each drive as its own item, showing its activity or used space, and one network adapter or all physical ones

## Popups

Rest the mouse on an item to see its graph and details. Choose how much each popup shows, for all items or per item:

| Simple | Standard | Detailed |
|:---:|:---:|:---:|
| <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-simple.png" width="250" alt="Simple CPU popup"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-standard.png" width="250" alt="Standard CPU popup"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-detailed.png" width="250" alt="Detailed CPU popup"> |
| The value and its graph | The main readings and top processes | Everything Pulsar reads, plus the low, average and high of the graph |

Popups are translucent by default, blurring what is behind them, or solid if you prefer:

| Translucent | Solid |
|:---:|:---:|
| <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-standard.png" width="250" alt="Translucent popup"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-solid.png" width="250" alt="Solid popup"> |

<details>
<summary><b>What each popup shows</b></summary>

| Item | Standard | Detailed adds |
|---|---|---|
| **CPU** | clock speed, per-core load, top processes, and with the sensor service temperature, power, hottest core and thermal throttling | process, thread and handle counts, and up time |
| **CPU temperature** | temperature, power, hottest core, thermal throttling | per-core temperatures |
| **CPU power** | temperature, power, hottest core, thermal throttling | cores, graphics and memory power, TDP, sustained and burst limits, and per-core power |
| **CPU clock** | average and fastest core | per-core clocks |
| **Memory** | in use, available, committed, top processes | in use, modified, standby and free as a bar, and the kernel pools |
| **Chipset temperature** | chipset, CPU, GPU and hottest drive | |
| **GPU** | temperature, dedicated and shared memory, top processes | engines, fan, power, memory clock, and hot spot and memory temperature where the card has them |
| **GPU temperature** | fan, power, memory clock, hot spot, memory temperature | |
| **Disk** | read and write rates, chosen drives, top processes by I/O | free space per drive |
| **Drive temperature** | each drive, by letter and model | |
| **Network** | traffic per adapter | totals since Pulsar started |
| **Ping** | host, min/avg/max, packet loss | jitter |
| **Battery** | state, time remaining | charge rate, energy, capacity when new, health, charge cycles, voltage |
| **Frame rate** | application, frame time, 1% and 0.1% lows, how often its monitor's picture changed | min/avg/max |

Every Detailed popup except Network also shows the low, average and high of its graph.

</details>

<details>
<summary><b>Every popup, Detailed</b></summary>

| | | |
|:---:|:---:|:---:|
| <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-detailed.png" width="250" alt="CPU"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-temp-detailed.png" width="250" alt="CPU temperature"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-power-detailed.png" width="250" alt="CPU power"> |
| CPU | CPU temperature | CPU power |
| <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-cpu-clock-detailed.png" width="250" alt="CPU clock"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-ram-detailed.png" width="250" alt="Memory"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-chipset-temp-detailed.png" width="250" alt="Chipset temperature"> |
| CPU clock | Memory | Chipset temperature |
| <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-gpu-detailed.png" width="250" alt="GPU"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-gpu-temp-detailed.png" width="250" alt="GPU temperature"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-disk-detailed.png" width="250" alt="Disk"> |
| GPU | GPU temperature | Disk |
| <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-disk-temp-detailed.png" width="250" alt="Drive temperature"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-network-detailed.png" width="250" alt="Network"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/popup-ping-detailed.png" width="250" alt="Ping"> |
| Drive temperature | Network | Ping |

</details>

## System details window

<kbd>Ctrl</kbd>+click the overlay, or choose *System details…* from its menu, for a CPU-Z-style view of the whole machine. It opens on the clicked item's page, with the hover graphs and their full history.

| Page | What it shows |
|---|---|
| **Overview** | a summary of the machine, live graphs for every item, and temperatures, power and up time |
| **Processor** | per-core load, clock and temperature as a matrix, strips, rings or a graph per thread, with power, voltage, caches and instruction sets |
| **Memory** | usage, commit, timings and each module |
| **Graphics** | utilisation, temperature, engines, graphics memory and every adapter with its PCIe link |
| **Storage** | activity, transfer rate, and each drive's temperature, limits and free space |
| **Network** | traffic and latency history, and every adapter's link speed and addresses |
| **System** | motherboard, chipset, BIOS and Windows |
| **Sensors** | every reading in a collapsible tree, with its current value, low and high |

![Details window overview](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/details-0.png)

<details>
<summary><b>More pages</b></summary>

**Processor**

![Details window processor page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/processor.gif)

**Memory**

![Details window memory page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/details-2.png)

**Graphics**

![Details window graphics page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/details-3.png)

**Storage**

![Details window storage page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/details-4.png)

**Network**

![Details window network page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/network.png)

**System**

![Details window system page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/details-6.png)

**Sensors**

![Details window sensors page](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/sensors.gif)

</details>

## Using Pulsar

| Where | Action | What happens |
|---|---|---|
| Overlay | Rest the mouse on an item | its popup |
| Overlay | Left-click | opens Task Manager |
| Overlay | <kbd>Ctrl</kbd>+click | opens the system details window on that item's page |
| Overlay | Right-click | the menu, with the clicked item's own options at the top |
| Overlay | Drag, in floating mode | moves it |
| Tray icon | Left-click | opens Settings |
| Tray icon | Right-click | the menu |
| Start menu | Launch Pulsar while it runs | opens Settings |

The tray icon can be turned off in *Settings → General*. It then appears only while a notification is showing, and the overlay's right-click menu still reaches Settings.

### The menu

Switch between graphs and text, show or hide items, and set popup detail, units, the update interval, labels, icons and floating mode, all without opening Settings.

| The clicked item's options | Popups |
|:---:|:---:|
| <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/menu-cpu.png" width="400" alt="Menu with the CPU item's options"> | <img src="https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/menu-popups.png" width="400" alt="Menu with popup options"> |

### Settings

Every option, applied as you change it.

<details>
<summary><b>Settings pages</b></summary>

**General:** language, start with Windows, update checks, the tray icon, and how often Pulsar samples

![Settings, General](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/settings-general.png)

**Display:** mode, labels, fonts, colours, popups, units, position and backgrounds

![Settings, Display](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/settings-display.png)

**Items:** which items show, their order and colour, and each item's graph style, colouring and popup detail

![Settings, Items](https://raw.githubusercontent.com/oMaN-Rod/Pulsar/main/docs/images/settings-items.png)

</details>

## Sensor service

Everything Pulsar shows by default is read without administrator rights. A few readings need kernel access, which Pulsar does not have and does not ask for by default. Two optional pieces provide them, and both need administrator rights:

- **[PawnIO](https://pawnio.eu)** ([source](https://github.com/namazso/PawnIO)), an open-source signed driver by namazso. It only runs modules signed by its author, and the ones Pulsar uses can read a fixed list of registers.
- **The Pulsar sensor service** (`pulsar-sensors.exe`), which reads temperatures and power draw through PawnIO once a second and publishes those numbers. It runs as a Windows service from `%ProgramFiles%\Pulsar`. Pulsar itself stays unelevated. The only input the service takes is a yes-or-no request for frame rates.

The *Frame rate* item also comes from the service but does not need PawnIO. While the item is on, the service listens to the present events that DXGI, Direct3D 9 and the graphics kernel already log through Event Tracing for Windows, which only an administrator may do. It never touches the game: nothing is loaded into it or read from its memory, which is what anti-cheat software looks for. By default the item shows the rate of the app in front: how often it hands a frame to Windows, counting the GPU process that browsers and apps built on them draw from. An app that is not drawing shows 0 fps. The popup also shows how often the picture changed on that app's monitor. That is not the sum of the apps on it: Windows combines them into one picture per refresh, so it never exceeds the refresh rate, except for a game in fullscreen with V-Sync off. To watch a game on one monitor while working on another, choose that monitor under *Settings → Sensors → Frame rate*. The trace stops a few seconds after the item is turned off.

### Setting it up

1. Install the pieces:
   - **Installer:** tick the boxes on its *CPU temperature* page. Run the installer again to add or remove them later.
   - **winget:** `winget install` runs the installer silently, which never asks for administrator rights and so skips that page. Run `winget install -i oMaN-Rod.Pulsar` to see it, or follow the next step using `%LOCALAPPDATA%\Programs\Pulsar\pulsar-sensors.exe`.
   - **Portable zip or Cargo:** install PawnIO (`winget install namazso.PawnIO`), then run `pulsar-sensors.exe --install` from an administrator terminal.
2. Turn on *CPU temperature*, *CPU power*, *Chipset temperature* or *Frame rate* in *Settings → Items*.

> [!NOTE]
> The service runs from its own copy of `pulsar-sensors.exe`. The installer's wizard refreshes that copy, but a silent upgrade (`winget upgrade`) and `cargo install` do not, so run `pulsar-sensors.exe --install` again after those if Pulsar says the service needs updating.

To remove the service, run `pulsar-sensors.exe --uninstall` from an administrator terminal. Uninstalling Pulsar from Windows Settings or winget does this for you; `cargo uninstall` and deleting the portable folder do not.

Without the service, the *CPU temperature*, *CPU power*, *Chipset temperature* and *Frame rate* items show "—" and their popup says what is missing; the other readings are simply left out.

<details>
<summary><b>What needs the sensor service</b></summary>

| Reading | Built in | With the sensor service |
|---|:---:|---|
| CPU usage, clock speed, per-core load, top processes | ✓ | |
| Memory, disk, network and ping | ✓ | |
| Drive temperature, for drives that report one | ✓ | |
| GPU board maker and PCIe link, chipset model (details window) | ✓ | |
| GPU usage, VRAM, temperature, fan and power | ✓ | |
| CPU temperature | | Intel, and AMD Ryzen (Zen and later) |
| CPU power | | Intel, and AMD Ryzen (Zen and later) |
| Chipset temperature | | Intel, chipsets from the 8 series on |
| TDP and power limits (details window, CPU power popup) | | Intel |
| Multiplier range and ring clock (details window) | | Intel |
| Memory timings and clocks (details window) | | Intel 12th–14th gen; AMD Ryzen without the controller clock |
| Hottest core (popups) | | Intel |
| Per-core temperature (details window, CPU temperature popup) | | Intel |
| Per-core clock (details window, CPU clock popup) | | Intel, and AMD Ryzen |
| Per-core voltage (details window) | | Intel |
| Per-core power (details window, CPU power popup) | | AMD Ryzen |
| Cores, integrated graphics and memory power (details window, CPU power popup) | | Intel; cores only on AMD Ryzen |
| Thermal throttling (popups) | | Intel |
| GPU hot spot (popups) | | NVIDIA RTX 50 series |
| GPU memory temperature (popups) | | NVIDIA RTX 30 series and later |
| Frame rate | | Any CPU, without PawnIO |

The GPU rows appear only where the card has those sensors, and they need one of the supported CPUs.

</details>

## Languages

Pulsar is available in English, Spanish (Español) and Simplified Chinese (简体中文). It follows the Windows display language; pick another under *Settings → General → Language*. To add or improve a translation, see [docs/translating.md](https://github.com/oMaN-Rod/Pulsar/blob/main/docs/translating.md).

## Settings and files

| What | Where |
|---|---|
| Settings, applied live | `%APPDATA%\Pulsar\config.toml` |
| Logs, crash reports and diagnostics | `%LOCALAPPDATA%\Pulsar\logs` |
| A settings file that could not be read | backed up as `config.toml.bak`, and defaults are used |

## Privacy

Pulsar makes only two kinds of network request:

- **Ping:** pings the host you choose. Ping is off until you enable the Ping item.
- **Update check:** asks GitHub's releases API about new versions once a day. Turn it off in *Settings → General*. Nothing is ever downloaded or installed automatically.

There is no telemetry.

## Troubleshooting

- **Something looks wrong:** open *Settings → About → Logs* and look at `pulsar.log`.
- **Pulsar crashed:** it writes a `crash-*.txt` report to the logs folder and offers to open it the next time it starts.
- **A reading is missing or wrong:** *Settings → About → Diagnostics → Save report*, or `pulsar --diag [file]`, writes every reading Pulsar can take to `diagnostics.txt` in the logs folder.

Then [open an issue](https://github.com/oMaN-Rod/Pulsar/issues/new/choose) with the log and any crash or diagnostics report attached.

> [!IMPORTANT]
> The diagnostics report lists process and network adapter names. Read it before sharing.

## Build from source

```
cargo build --release
cargo test --workspace
```

| Script | What it does |
|---|---|
| `scripts\package.ps1` | builds the installer, the portable zip and the winget manifests into `dist\`; needs [Inno Setup](https://jrsoftware.org/isinfo.php) 6.3 or later |
| `scripts\screenshots.ps1` | captures the overlay, popups, menus, Settings and the details window to `target\screenshots\`; `-Publish` refreshes the images in `docs\images` |

Also see the [manual testing checklist](https://github.com/oMaN-Rod/Pulsar/blob/main/docs/testing.md) and the [release guide](https://github.com/oMaN-Rod/Pulsar/blob/main/docs/releasing.md).

## License

[GPL-3.0-or-later](https://github.com/oMaN-Rod/Pulsar/blob/main/LICENSE). The settings window is built with [Slint](https://slint.dev), used under its GPLv3 option.
