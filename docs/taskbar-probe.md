# Recording an overflowed taskbar

Pulsar works out how much of the taskbar is free from what UI Automation says about the taskbar's buttons. It has only been tested against a taskbar with room to spare, so what Windows reports once the taskbar overflows is still an assumption. These recordings settle that, and become test data.

`scripts\record-taskbar.ps1` is read-only: it reads window and UI Automation properties and the registry. It clicks nothing, moves nothing and starts nothing. App names and window titles are replaced with placeholders.

## Recording

Close Pulsar first, so that its own window is not part of the picture.

1. Put the taskbar in the state to record. To overflow it, open windows of different apps until the `...` button appears, keeping the apps' buttons on the taskbar.
2. Without clicking anything on the taskbar, run:

   ```
   pwsh -File scripts\record-taskbar.ps1 -Label overflow-centred -Samples 5
   ```

   The file is saved in `target\taskbar-fixtures`, named after the label.
3. Open the JSON and check that nothing in it is private before sharing it.
4. Note what you saw on screen: where the `...` button is, how many buttons show, and the gap left next to the tray.

## What to record

Use one label per row, recorded twice: with the taskbar nearly full, then overflowed.

| Label | Taskbar state |
|---|---|
| `overflow-centred` | Centred, labels combined (the default) |
| `overflow-left` | Left aligned |
| `overflow-labels` | *Combine taskbar buttons* set to *Never* |
| `overflow-small` | Smaller taskbar buttons, on 24H2 or later |
| `overflow-widgets-left` | Centred with Widgets on, so its button is at the left |
| `overflow-widgets-right` | Left aligned with Widgets on, so its button is at the right |
| `overflow-dpi150` | Display scaling at 150% (sign out and back in first) |
| `overflow-secondary` | A second monitor with the taskbar on all displays, overflowing there |
| `overflow-flyout` | The `...` flyout open, without clicking anything in it |
| `shell-restyled` | Any taskbar restyler or replacement you want Pulsar to handle |
| `vertical` | A taskbar made vertical by a third-party tool, if you use one |
| `empty-after-restart` | Run the script at once after `start explorer`, with `-Samples 10` |

## What each recording confirms

| Assumption in the code | How to check it in the recording |
|---|---|
| `TaskbarFrame` is below the island, in `Shell_TrayWnd` → `DesktopWindowContentBridge` → `InputSite`, on every taskbar | `windows` has those classes; `uia` has `id: TaskbarFrame` under `InputSite`. Also check `overflow-secondary` and `shell-restyled` |
| The island's bridge class is one of the two in `taskbar\space.rs` | The class under `windows`, on every build |
| Buttons in the overflow flyout are no longer under `TaskbarFrame`, or are marked `offscreen` | Count the `Appid:` nodes with `offscreen: false` against the buttons you counted on screen |
| The `...` button is under `TaskbarFrame` with a real rectangle, so it counts as occupied | Find a node at the position where you saw it |
| No overflowed button keeps a rectangle over the free space | No `offscreen: false` node overlaps the gap you saw |
| `TaskbarFrameRepeater` does not cover more than the buttons | Its `rect` ends where the last visible button ends. If it spans the free space, the free space reads as occupied |
| Tray items have the id `SystemTrayIcon`, directly under `InputSite` | On the primary and `overflow-secondary` |
| Rectangles are physical pixels on the bar, at any scaling | `overflow-dpi150`: compare the node rectangles to `rect` and to the screen |
| `TaskbarAl` decides centred or left, with a missing value meaning centred | `settings` against what you see |
| A centred taskbar still has a gap each side as it fills, until one runs out | `overflow-centred` near full, and what happens to the centring at overflow |
| A taskbar in the middle of restarting shows up as missing or empty, not as a full bar | `empty-after-restart` |
| Some gap is left next to the tray at overflow | The difference between the last button and the tray; if this is near zero, *When no item fits* is what users will see |

A recording shows the raw tree, where the buttons are children of `TaskbarFrameRepeater`. Pulsar reads the control view, which leaves that group out, so a button counts as long as it is somewhere under `TaskbarFrame`.

If any of these fails, start with `rects_from` and `Uia::read` in `crates\pulsar-monitor\src\taskbar\space.rs`, and `free_gaps` in `placement.rs`. The rectangles in a recording can be pasted into `free_gaps` tests as they are.

Two more things to decide from the recordings: whether a pill (a compact button holding the items) is worth building, and whether the *Fit to the taskbar* default should stay on.
