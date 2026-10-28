# Manual testing checklist

These checks cover what unit tests cannot: the real taskbar, the installer, and the network. Run them before every release on Windows 11, with the build from `scripts\package.ps1`.

## Overlay and taskbar

| Check | Expected |
|---|---|
| Restart Explorer: `taskkill /f /im explorer.exe`, then `start explorer` | The overlay and tray icon come back within a few seconds, in the same place |
| Position: *Next to the tray* and *Left edge*, plus an offset | Placement follows each setting; the overlay never covers the tray or the Start button |
| Taskbar auto-hide on | The overlay hides and shows with the taskbar |
| Display scaling 100%, 150%, 200% (sign out between) | Text stays sharp and fits; nothing is clipped |
| A second monitor, with *Every taskbar* on, then unplug it | One overlay per taskbar; unplugging leaves the primary one working |
| A fullscreen game or video | The overlay fades out after a moment and returns as soon as fullscreen ends |
| Task View, Alt+Tab, Start, Search | The overlay does not hide |
| Light and dark Windows mode | Colours follow the taskbar theme |
| Small taskbar icons (if available) or a custom taskbar such as TranslucentTB | Still readable; the tile and panel backgrounds help on translucent taskbars |
| Graph and text mode; labels Full, Short and None, with and without icons | Every combination lays out without overlap |
| Graph mode with *Single-line graphs* on, on a short taskbar if available | Icon, label and value share one centred line in every tile; network shows ↓ and ↑ side by side |
| Floating: drag it, lock it, restart Pulsar, then *Reset position* | It moves and stays locked, reopens where it was left, and reset centres it above the taskbar |

## Items

| Check | Expected |
|---|---|
| Hover each item | Its popup shows a graph and details, and updates live |
| GPU temperature on | TEMP roughly matches Task Manager; on a machine without a sensor it shows "—" and the popup explains why |
| Disks: *Each drive*, then *Used space* | One cell per ticked drive; values match Explorer |
| Network: choose one adapter, then unplug it | Only that adapter is counted; unplugged shows "—" with the reason |
| Ping on, with a bad host | The latency cell shows "—" and the popup names the error |

| CPU temperature on, without the sensor service | The cell shows "—" and the popup says the sensor service is not installed |
| Hover CPU and CPU temperature with the sensor service running | CPU lists temperature, power, hottest core and thermal throttling; CPU temperature adds a bar per physical core (Intel) |
| Hover GPU with the sensor service running, on an NVIDIA RTX card | *Hot spot* (RTX 50) and *Memory temperature* (cards with memory sensors) appear and roughly match HWiNFO; other cards show neither row |
| CPU temperature on, with the sensor service and PawnIO | A plausible temperature that rises under load; *CPU power* shows watts that rise with it; stopping the *Pulsar Sensors* service turns it to "—" within a few seconds |

## Installer

| Check | Expected |
|---|---|
| Fresh install | No admin prompt; installs to `%LOCALAPPDATA%\Programs\Pulsar`; a Start Menu entry; autostart as chosen |
| Settings → General on a fresh install with autostart ticked | *Start with Windows* is on |
| Fresh install with *Install the Pulsar sensor service* and *Also install PawnIO* ticked | winget installs PawnIO, one administrator prompt registers the service, `pulsar-sensors.exe` is in `%ProgramFiles%\Pulsar`, and the *Pulsar Sensors* service is running |
| The same, declining the administrator prompt | A message says CPU temperature will show a dash; the rest of the install is unaffected |
| Run the installer again and untick the sensor service | One administrator prompt; the service and `%ProgramFiles%\Pulsar` are gone |
| Uninstall with the sensor service installed | One administrator prompt; the service and `%ProgramFiles%\Pulsar` are gone |
| Install again over a running Pulsar and Settings | Both close, no autostart checkbox is offered, Pulsar restarts, and autostart is unchanged |
| Uninstall, answering **No** to removing settings | Both close; the program folder and the `Run` value are gone; `%APPDATA%\Pulsar` remains |
| Uninstall, answering **Yes** | `%APPDATA%\Pulsar` and `%LOCALAPPDATA%\Pulsar` are gone too |
| Silent upgrade: `pulsar-<v>-setup.exe /VERYSILENT` while Pulsar runs | Pulsar is replaced and running again afterwards |

## Diagnostics and updates

| Check | Expected |
|---|---|
| Crash offer: exit Pulsar, then put `crash-20260101-000000-pulsar.txt` (any text) and a `crash.pending` containing that file name in the logs folder, and start Pulsar | A "Pulsar closed unexpectedly" notification appears; clicking it opens the report |
| Delete `%LOCALAPPDATA%\Pulsar\update.toml` and start Pulsar | About a minute later, `pulsar.log` shows the update check result; no notification while you have the newest version |
| Settings → General → *Tray icon* off, then on | The icon leaves and returns at once; while off, the overlay's right-click menu and a second launch still reach Settings |
| *Tray icon* off, then the crash offer check above | The icon appears with the notification and goes once it is clicked or times out |
| Settings → About | Version and icon shown; the Logs, GitHub and issue buttons open the right places |

## Install routes

| Check | Expected |
|---|---|
| `cargo install --path crates\pulsar-monitor --root %TEMP%\pulsar-cargo` | `bin\pulsar.exe`, `bin\pulsar-settings.exe` and `bin\pulsar-details.exe` side by side, and they run like the installed copy |
| `winget validate --manifest dist\winget\oMaN-Rod.Pulsar\<version>` | `Manifest validation succeeded.` |

## Languages

| Check | Expected |
|---|---|
| *Language* set to each language in turn, with Pulsar running | The taskbar labels, menu and popups switch at once; Settings switches when the choice saves; Details on its next opening |
| *Windows default* with the Windows display language set to Spanish, then Chinese (Traditional) | Spanish, then English |
| Chinese: Settings, Details, a popup and the taskbar | No missing or Japanese-style glyphs |
| Settings open in Spanish or Chinese, then run the installer | The installer closes the settings window before installing |
| The installer started on Spanish and Chinese Windows | Its pages, including *CPU temperature*, are in that language |

## Footprint

After 5 minutes idle with Settings closed, check that Pulsar uses **< 15 MB** private working set (Task Manager → Details → *Memory (private working set)*) and **< 1%** CPU.
