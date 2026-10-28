use pulsar_core::config::{
    ColorMode, Config, DisplayMode, GraphStyle, LabelStyle, MAX_POPUP_PROCESSES, PopupBackground,
    PopupDetail, Position, RateUnit, TemperatureUnit,
};
use pulsar_core::metric::ItemKind;
use pulsar_core::tr;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HMENU, MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED,
    MF_POPUP, MF_SEPARATOR, MF_STRING, PostMessageW, SW_SHOWNORMAL, SetForegroundWindow,
    TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
};
use windows::core::{HSTRING, PCSTR, PCWSTR, w};

use crate::names::{
    COLOR_MODES, DETAILS, STYLES, color_mode_name, detail_name, item_name, style_name,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Mode(DisplayMode),
    Settings,
    Details,
    About,
    TaskManager,
    Exit,
    Toggle(Toggle),
    Edit(Edit),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggle {
    ShortLabels,
    Icons,
    HoverPopup,
    HideInFullscreen,
    Floating,
    LockPosition,
}

/// A settings change made from the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    PopupDetail(PopupDetail),
    PopupGraph,
    PopupTranslucent,
    PopupProcesses(u8),
    Temperature(TemperatureUnit),
    Network(RateUnit),
    UploadOnTop,
    IntervalMs(u32),
    ShowItem(ItemKind),
    Item(ItemKind, ItemEdit),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemEdit {
    /// `None` shows text only.
    Style(Option<GraphStyle>),
    Color(ColorMode),
    /// `None` follows the popup setting for every item.
    Popup(Option<PopupDetail>),
    Move {
        earlier: bool,
    },
    Hide,
}

pub fn apply_toggle(c: &mut Config, t: Toggle) {
    let d = &mut c.display;
    match t {
        Toggle::ShortLabels => {
            d.labels = if d.labels == LabelStyle::Short {
                LabelStyle::Full
            } else {
                LabelStyle::Short
            }
        }
        Toggle::Icons => {
            d.icons = !d.icons;
            if !d.icons && d.labels == LabelStyle::None {
                d.labels = LabelStyle::Full;
            }
        }
        Toggle::HoverPopup => d.hover_popup = !d.hover_popup,
        Toggle::HideInFullscreen => d.hide_in_fullscreen = !d.hide_in_fullscreen,
        Toggle::Floating => {
            d.position = if d.position == Position::Floating {
                Position::Right
            } else {
                Position::Floating
            }
        }
        Toggle::LockPosition => d.lock_position = !d.lock_position,
    }
}

pub fn apply_edit(c: &mut Config, e: Edit) {
    match e {
        Edit::PopupDetail(detail) => c.popup.detail = detail,
        Edit::PopupGraph => c.popup.graph = !c.popup.graph,
        Edit::PopupTranslucent => {
            c.popup.background = match c.popup.background {
                PopupBackground::Solid => PopupBackground::Translucent,
                PopupBackground::Translucent => PopupBackground::Solid,
            }
        }
        Edit::PopupProcesses(n) => c.popup.processes = n.min(MAX_POPUP_PROCESSES),
        Edit::Temperature(unit) => c.units.temperature = unit,
        Edit::Network(unit) => c.units.network = unit,
        Edit::UploadOnTop => c.display.upload_on_top = !c.display.upload_on_top,
        Edit::IntervalMs(ms) => c.general.sample_interval_ms = ms,
        Edit::ShowItem(kind) => {
            if let Some(item) = c.item_mut(kind) {
                item.enabled = !item.enabled;
            }
        }
        Edit::Item(kind, ItemEdit::Move { earlier }) => c.move_item(kind, earlier),
        Edit::Item(kind, edit) => {
            let Some(item) = c.item_mut(kind) else {
                return;
            };
            match edit {
                ItemEdit::Style(Some(style)) => {
                    item.graph = true;
                    item.style = style;
                }
                ItemEdit::Style(None) => item.graph = false,
                ItemEdit::Color(mode) => item.color_mode = mode,
                ItemEdit::Popup(detail) => item.popup = detail,
                ItemEdit::Hide => item.enabled = false,
                ItemEdit::Move { .. } => {}
            }
        }
    }
}

const ID_GRAPHS: usize = 1;
const ID_TEXT: usize = 2;
const ID_TASK_MANAGER: usize = 3;
const ID_EXIT: usize = 4;
const ID_SETTINGS: usize = 5;
const ID_ABOUT: usize = 6;
const ID_SHORT_LABELS: usize = 7;
const ID_HOVER: usize = 8;
const ID_HIDE_FULLSCREEN: usize = 9;
const ID_FLOATING: usize = 10;
const ID_LOCK: usize = 11;
const ID_ICONS: usize = 12;
const ID_DETAILS: usize = 13;
/// Plus the index into `DETAILS`.
const ID_POPUP_DETAIL: usize = 20;
const ID_POPUP_GRAPH: usize = 25;
const ID_POPUP_TRANSLUCENT: usize = 26;
/// Plus the index into `PROCESS_COUNTS`.
const ID_PROCESSES: usize = 30;
const ID_CELSIUS: usize = 40;
const ID_FAHRENHEIT: usize = 41;
const ID_BYTES: usize = 42;
const ID_BITS: usize = 43;
const ID_UPLOAD_ON_TOP: usize = 44;
/// Plus the index into `INTERVALS_MS`.
const ID_INTERVAL: usize = 50;
/// Plus the index into `ItemKind::ALL`.
const ID_SHOW_ITEM: usize = 100;
/// Plus `ITEM_IDS` times the item's index into `ItemKind::ALL`, plus one
/// of the offsets below.
const ID_ITEM: usize = 1000;
const ITEM_IDS: usize = 32;
const ITEM_STYLE: usize = 0;
const ITEM_COLOR: usize = 8;
const ITEM_POPUP_DEFAULT: usize = 12;
const ITEM_POPUP: usize = 13;
const ITEM_EARLIER: usize = 20;
const ITEM_LATER: usize = 21;
const ITEM_HIDE: usize = 22;

const PROCESS_COUNTS: [u8; 6] = [0, 3, 5, 8, 10, 15];
const INTERVALS_MS: [u32; 4] = [500, 1000, 2000, 5000];

fn kind_at(index: usize) -> Option<ItemKind> {
    ItemKind::ALL.get(index).copied()
}

fn kind_index(kind: ItemKind) -> usize {
    ItemKind::ALL.iter().position(|&k| k == kind).unwrap_or(0)
}

fn item_id(kind: ItemKind, offset: usize) -> usize {
    ID_ITEM + kind_index(kind) * ITEM_IDS + offset
}

fn item_command(id: usize) -> Option<Command> {
    let kind = kind_at((id - ID_ITEM) / ITEM_IDS)?;
    let offset = (id - ID_ITEM) % ITEM_IDS;
    let edit = match offset {
        o if o < ITEM_STYLE + STYLES.len() => ItemEdit::Style(STYLES[o - ITEM_STYLE]),
        o if (ITEM_COLOR..ITEM_COLOR + COLOR_MODES.len()).contains(&o) => {
            ItemEdit::Color(COLOR_MODES[o - ITEM_COLOR])
        }
        ITEM_POPUP_DEFAULT => ItemEdit::Popup(None),
        o if (ITEM_POPUP..ITEM_POPUP + DETAILS.len()).contains(&o) => {
            ItemEdit::Popup(Some(DETAILS[o - ITEM_POPUP]))
        }
        ITEM_EARLIER => ItemEdit::Move { earlier: true },
        ITEM_LATER => ItemEdit::Move { earlier: false },
        ITEM_HIDE => ItemEdit::Hide,
        _ => return None,
    };
    Some(Command::Edit(Edit::Item(kind, edit)))
}

pub fn command_for(id: usize) -> Option<Command> {
    let edit = |e| Some(Command::Edit(e));
    let in_range = |base: usize, len: usize| (base..base + len).contains(&id);
    match id {
        ID_GRAPHS => Some(Command::Mode(DisplayMode::Graph)),
        ID_TEXT => Some(Command::Mode(DisplayMode::Text)),
        ID_SETTINGS => Some(Command::Settings),
        ID_DETAILS => Some(Command::Details),
        ID_ABOUT => Some(Command::About),
        ID_SHORT_LABELS => Some(Command::Toggle(Toggle::ShortLabels)),
        ID_HOVER => Some(Command::Toggle(Toggle::HoverPopup)),
        ID_HIDE_FULLSCREEN => Some(Command::Toggle(Toggle::HideInFullscreen)),
        ID_FLOATING => Some(Command::Toggle(Toggle::Floating)),
        ID_LOCK => Some(Command::Toggle(Toggle::LockPosition)),
        ID_ICONS => Some(Command::Toggle(Toggle::Icons)),
        ID_TASK_MANAGER => Some(Command::TaskManager),
        ID_EXIT => Some(Command::Exit),
        ID_POPUP_GRAPH => edit(Edit::PopupGraph),
        ID_POPUP_TRANSLUCENT => edit(Edit::PopupTranslucent),
        ID_CELSIUS => edit(Edit::Temperature(TemperatureUnit::Celsius)),
        ID_FAHRENHEIT => edit(Edit::Temperature(TemperatureUnit::Fahrenheit)),
        ID_BYTES => edit(Edit::Network(RateUnit::Bytes)),
        ID_BITS => edit(Edit::Network(RateUnit::Bits)),
        ID_UPLOAD_ON_TOP => edit(Edit::UploadOnTop),
        _ if in_range(ID_POPUP_DETAIL, DETAILS.len()) => {
            edit(Edit::PopupDetail(DETAILS[id - ID_POPUP_DETAIL]))
        }
        _ if in_range(ID_PROCESSES, PROCESS_COUNTS.len()) => {
            edit(Edit::PopupProcesses(PROCESS_COUNTS[id - ID_PROCESSES]))
        }
        _ if in_range(ID_INTERVAL, INTERVALS_MS.len()) => {
            edit(Edit::IntervalMs(INTERVALS_MS[id - ID_INTERVAL]))
        }
        _ if in_range(ID_SHOW_ITEM, ItemKind::ALL.len()) => {
            edit(Edit::ShowItem(kind_at(id - ID_SHOW_ITEM)?))
        }
        _ if in_range(ID_ITEM, ItemKind::ALL.len() * ITEM_IDS) => item_command(id),
        _ => None,
    }
}

/// A native popup menu that destroys itself and its submenus when dropped.
struct Menu(HMENU);

impl Menu {
    fn new() -> Option<Self> {
        unsafe { CreatePopupMenu() }.ok().map(Self)
    }

    fn item(&self, id: usize, text: &str, checked: bool, enabled: bool) {
        let mut flags = MF_STRING;
        if checked {
            flags |= MF_CHECKED;
        }
        if !enabled {
            flags |= MF_GRAYED;
        }
        self.append(flags, id, text);
    }

    fn separator(&self) {
        unsafe {
            let _ = AppendMenuW(self.0, MF_SEPARATOR, 0, PCWSTR::null());
        }
    }

    /// Hands `sub` to this menu, which then owns and destroys it.
    fn submenu(&self, sub: Menu, text: &str) {
        self.append(MF_POPUP, sub.0.0 as usize, text);
        std::mem::forget(sub);
    }

    fn append(&self, flags: MENU_ITEM_FLAGS, id: usize, text: &str) {
        unsafe {
            let _ = AppendMenuW(self.0, flags, id, &HSTRING::from(text));
        }
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyMenu(self.0);
        }
    }
}

/// Style, colour, popup and position for the item that was right-clicked.
fn item_menu(config: &Config, kind: ItemKind) -> Option<Menu> {
    let item = config.item(kind)?;
    let menu = Menu::new()?;
    let graphs = config.display.mode == DisplayMode::Graph;
    let current = item.graph.then_some(item.style);
    for (i, style) in STYLES.into_iter().enumerate() {
        let id = item_id(kind, ITEM_STYLE + i);
        menu.item(id, style_name(style), current == style, graphs);
    }
    menu.separator();
    for (i, mode) in COLOR_MODES.into_iter().enumerate() {
        let id = item_id(kind, ITEM_COLOR + i);
        menu.item(id, color_mode_name(mode), item.color_mode == mode, true);
    }
    menu.separator();
    let popup = Menu::new()?;
    let everywhere = tr!("Same as other items ({})", detail_name(config.popup.detail));
    popup.item(
        item_id(kind, ITEM_POPUP_DEFAULT),
        &everywhere,
        item.popup.is_none(),
        true,
    );
    for (i, detail) in DETAILS.into_iter().enumerate() {
        let id = item_id(kind, ITEM_POPUP + i);
        popup.item(id, detail_name(detail), item.popup == Some(detail), true);
    }
    menu.submenu(popup, tr!("Popup detail"));
    menu.separator();
    let enabled = config.enabled_items();
    let at = enabled.iter().position(|&k| k == kind);
    let first = at == Some(0);
    let last = at.is_some_and(|i| i + 1 == enabled.len());
    menu.item(
        item_id(kind, ITEM_EARLIER),
        tr!("Swap with item on the left"),
        false,
        !first,
    );
    menu.item(
        item_id(kind, ITEM_LATER),
        tr!("Swap with item on the right"),
        false,
        !last,
    );
    menu.item(item_id(kind, ITEM_HIDE), tr!("Hide"), false, true);
    Some(menu)
}

fn items_menu(config: &Config) -> Option<Menu> {
    let menu = Menu::new()?;
    for item in ItemKind::ALL {
        let id = ID_SHOW_ITEM + kind_index(item);
        menu.item(id, item_name(item), config.is_enabled(item), true);
    }
    Some(menu)
}

fn popup_menu(config: &Config) -> Option<Menu> {
    let menu = Menu::new()?;
    for (i, detail) in DETAILS.into_iter().enumerate() {
        let id = ID_POPUP_DETAIL + i;
        menu.item(id, detail_name(detail), config.popup.detail == detail, true);
    }
    menu.separator();
    menu.item(ID_POPUP_GRAPH, tr!("Graph"), config.popup.graph, true);
    let translucent = config.popup.background == PopupBackground::Translucent;
    menu.item(
        ID_POPUP_TRANSLUCENT,
        tr!("Translucent background"),
        translucent,
        true,
    );
    let processes = Menu::new()?;
    for (i, &n) in PROCESS_COUNTS.iter().enumerate() {
        let text = if n == 0 {
            tr!("None").to_string()
        } else {
            n.to_string()
        };
        processes.item(ID_PROCESSES + i, &text, config.popup.processes == n, true);
    }
    menu.submenu(processes, tr!("Top processes"));
    menu.separator();
    menu.item(
        ID_HOVER,
        tr!("Details on hover"),
        config.display.hover_popup,
        true,
    );
    Some(menu)
}

fn units_menu(config: &Config) -> Option<Menu> {
    let menu = Menu::new()?;
    let u = config.units;
    let celsius = u.temperature == TemperatureUnit::Celsius;
    menu.item(ID_CELSIUS, tr!("Celsius (°C)"), celsius, true);
    menu.item(ID_FAHRENHEIT, tr!("Fahrenheit (°F)"), !celsius, true);
    menu.separator();
    let bytes = u.network == RateUnit::Bytes;
    menu.item(ID_BYTES, tr!("Bytes per second (MB/s)"), bytes, true);
    menu.item(ID_BITS, tr!("Bits per second (Mbps)"), !bytes, true);
    menu.separator();
    menu.item(
        ID_UPLOAD_ON_TOP,
        tr!("Upload on top"),
        config.display.upload_on_top,
        true,
    );
    Some(menu)
}

fn interval_menu(config: &Config) -> Option<Menu> {
    let menu = Menu::new()?;
    for (i, &ms) in INTERVALS_MS.iter().enumerate() {
        let text = tr!("{} s", f64::from(ms) / 1000.0);
        let checked = config.general.sample_interval_ms == ms;
        menu.item(ID_INTERVAL + i, &text, checked, true);
    }
    Some(menu)
}

/// Shows the context menu at a screen position and returns the chosen
/// command; `item` is the overlay item under the mouse, if any. Runs a
/// modal loop: the caller must not hold app state borrowed.
/// Win32 menus are drawn dark only in an app that opts in, which takes the
/// undocumented uxtheme exports `SetPreferredAppMode` (#135) and
/// `FlushMenuThemes` (#136).
fn match_theme(dark: bool) {
    const FORCE_DARK: i32 = 2;
    const FORCE_LIGHT: i32 = 3;
    let Ok(uxtheme) = (unsafe { GetModuleHandleW(w!("uxtheme.dll")) })
        .or_else(|_| unsafe { LoadLibraryW(w!("uxtheme.dll")) })
    else {
        return;
    };
    let export = |ordinal: usize| unsafe { GetProcAddress(uxtheme, PCSTR(ordinal as *const u8)) };
    let (Some(set_mode), Some(flush)) = (export(135), export(136)) else {
        return;
    };
    unsafe {
        let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
        let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
        set_mode(if dark { FORCE_DARK } else { FORCE_LIGHT });
        flush();
    }
}

/// `dark` draws the menu dark, to match a dark taskbar.
pub fn show(
    owner: HWND,
    x: i32,
    y: i32,
    config: &Config,
    item: Option<ItemKind>,
    dark: bool,
) -> Option<Command> {
    match_theme(dark);
    let menu = Menu::new()?;
    let display = &config.display;
    let floating = display.position == Position::Floating;
    if let Some(kind) = item
        && let Some(sub) = item_menu(config, kind)
    {
        menu.submenu(sub, item_name(kind));
        menu.separator();
    }
    menu.item(ID_DETAILS, tr!("System details…"), false, true);
    menu.separator();
    menu.item(
        ID_GRAPHS,
        tr!("Graphs"),
        display.mode == DisplayMode::Graph,
        true,
    );
    menu.item(
        ID_TEXT,
        tr!("Text only"),
        display.mode == DisplayMode::Text,
        true,
    );
    menu.separator();
    menu.submenu(items_menu(config)?, tr!("Items"));
    menu.submenu(popup_menu(config)?, tr!("Popups"));
    menu.submenu(units_menu(config)?, tr!("Units"));
    menu.submenu(interval_menu(config)?, tr!("Update interval"));
    menu.separator();
    menu.item(
        ID_SHORT_LABELS,
        tr!("Short labels"),
        display.labels == LabelStyle::Short,
        true,
    );
    menu.item(ID_ICONS, tr!("Icons"), display.icons, true);
    menu.item(
        ID_HIDE_FULLSCREEN,
        tr!("Hide in fullscreen"),
        display.hide_in_fullscreen,
        true,
    );
    menu.separator();
    menu.item(ID_FLOATING, tr!("Floating"), floating, true);
    menu.item(
        ID_LOCK,
        tr!("Lock position"),
        display.lock_position,
        floating,
    );
    menu.separator();
    menu.item(ID_SETTINGS, tr!("Settings…"), false, true);
    menu.item(ID_TASK_MANAGER, tr!("Open Task Manager"), false, true);
    menu.item(ID_ABOUT, tr!("About Pulsar"), false, true);
    menu.separator();
    menu.item(ID_EXIT, tr!("Exit Pulsar"), false, true);

    unsafe {
        // Without being foreground, the menu would not close on an outside click.
        let _ = SetForegroundWindow(owner);
        let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN;
        let id = TrackPopupMenu(menu.0, flags, x, y, None, owner, None).0 as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        command_for(id)
    }
}

/// Opens a file, folder or URL with its default handler.
pub fn shell_open(target: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

pub fn open_task_manager() {
    shell_open("taskmgr.exe");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_map_to_commands() {
        assert_eq!(command_for(ID_TEXT), Some(Command::Mode(DisplayMode::Text)));
        assert_eq!(command_for(ID_EXIT), Some(Command::Exit));
        assert_eq!(command_for(ID_SETTINGS), Some(Command::Settings));
        assert_eq!(command_for(ID_ABOUT), Some(Command::About));
        assert_eq!(command_for(ID_DETAILS), Some(Command::Details));
        assert_eq!(command_for(0), None, "menu dismissed");
    }

    #[test]
    fn toggle_ids_map_to_commands() {
        assert_eq!(
            command_for(ID_SHORT_LABELS),
            Some(Command::Toggle(Toggle::ShortLabels))
        );
        assert_eq!(
            command_for(ID_FLOATING),
            Some(Command::Toggle(Toggle::Floating))
        );
        assert_eq!(
            command_for(ID_LOCK),
            Some(Command::Toggle(Toggle::LockPosition))
        );
    }

    #[test]
    fn every_item_command_round_trips_through_its_id() {
        for kind in ItemKind::ALL {
            for (i, style) in STYLES.into_iter().enumerate() {
                assert_eq!(
                    command_for(item_id(kind, ITEM_STYLE + i)),
                    Some(Command::Edit(Edit::Item(kind, ItemEdit::Style(style))))
                );
            }
            for (i, mode) in COLOR_MODES.into_iter().enumerate() {
                assert_eq!(
                    command_for(item_id(kind, ITEM_COLOR + i)),
                    Some(Command::Edit(Edit::Item(kind, ItemEdit::Color(mode))))
                );
            }
            assert_eq!(
                command_for(item_id(kind, ITEM_POPUP_DEFAULT)),
                Some(Command::Edit(Edit::Item(kind, ItemEdit::Popup(None))))
            );
            for (i, detail) in DETAILS.into_iter().enumerate() {
                assert_eq!(
                    command_for(item_id(kind, ITEM_POPUP + i)),
                    Some(Command::Edit(Edit::Item(
                        kind,
                        ItemEdit::Popup(Some(detail))
                    )))
                );
            }
            assert_eq!(
                command_for(item_id(kind, ITEM_HIDE)),
                Some(Command::Edit(Edit::Item(kind, ItemEdit::Hide)))
            );
            assert_eq!(
                command_for(ID_SHOW_ITEM + kind_index(kind)),
                Some(Command::Edit(Edit::ShowItem(kind)))
            );
        }
        assert_eq!(command_for(item_id(ItemKind::Cpu, ITEM_HIDE + 1)), None);
        assert!(STYLES.len() <= ITEM_COLOR && ITEM_POPUP + DETAILS.len() <= ITEM_EARLIER);
        assert!(ID_SHOW_ITEM + ItemKind::ALL.len() <= ID_ITEM);
    }

    #[test]
    fn global_edit_ids_map_to_their_values() {
        assert_eq!(
            command_for(ID_POPUP_DETAIL + 2),
            Some(Command::Edit(Edit::PopupDetail(PopupDetail::Detailed)))
        );
        assert_eq!(
            command_for(ID_PROCESSES),
            Some(Command::Edit(Edit::PopupProcesses(0)))
        );
        assert_eq!(
            command_for(ID_INTERVAL + 3),
            Some(Command::Edit(Edit::IntervalMs(5000)))
        );
        assert_eq!(
            command_for(ID_BITS),
            Some(Command::Edit(Edit::Network(RateUnit::Bits)))
        );
        assert_eq!(command_for(ID_INTERVAL + INTERVALS_MS.len()), None);
    }

    #[test]
    fn edits_change_the_config() {
        let mut c = Config::default();
        apply_edit(&mut c, Edit::Item(ItemKind::Cpu, ItemEdit::Style(None)));
        assert!(!c.item(ItemKind::Cpu).unwrap().graph);
        let bars = Edit::Item(ItemKind::Cpu, ItemEdit::Style(Some(GraphStyle::Bars)));
        apply_edit(&mut c, bars);
        let cpu = c.item(ItemKind::Cpu).unwrap();
        assert!(cpu.graph && cpu.style == GraphStyle::Bars);
        apply_edit(
            &mut c,
            Edit::Item(ItemKind::Cpu, ItemEdit::Color(ColorMode::Load)),
        );
        apply_edit(
            &mut c,
            Edit::Item(ItemKind::Cpu, ItemEdit::Popup(Some(PopupDetail::Simple))),
        );
        assert_eq!(c.popup_detail(ItemKind::Cpu), PopupDetail::Simple);
        assert_eq!(c.item(ItemKind::Cpu).unwrap().color_mode, ColorMode::Load);
        apply_edit(&mut c, Edit::Item(ItemKind::Cpu, ItemEdit::Hide));
        assert!(!c.is_enabled(ItemKind::Cpu));
        apply_edit(&mut c, Edit::ShowItem(ItemKind::Cpu));
        assert!(c.is_enabled(ItemKind::Cpu));
        apply_edit(&mut c, Edit::Temperature(TemperatureUnit::Fahrenheit));
        apply_edit(&mut c, Edit::UploadOnTop);
        apply_edit(&mut c, Edit::PopupProcesses(200));
        apply_edit(&mut c, Edit::PopupTranslucent);
        assert_eq!(c.popup.background, PopupBackground::Solid);
        assert_eq!(
            command_for(ID_POPUP_TRANSLUCENT),
            Some(Command::Edit(Edit::PopupTranslucent))
        );
        assert_eq!(c.units.temperature, TemperatureUnit::Fahrenheit);
        assert!(c.display.upload_on_top);
        assert_eq!(c.popup.processes, MAX_POPUP_PROCESSES);
    }

    #[test]
    fn toggles_flip_the_config() {
        let mut c = Config::default();
        apply_toggle(&mut c, Toggle::Floating);
        assert_eq!(c.display.position, Position::Floating);
        apply_toggle(&mut c, Toggle::Floating);
        assert_eq!(c.display.position, Position::Right);
        apply_toggle(&mut c, Toggle::HideInFullscreen);
        assert!(!c.display.hide_in_fullscreen);
        apply_toggle(&mut c, Toggle::ShortLabels);
        assert_eq!(c.display.labels, LabelStyle::Short);
        apply_toggle(&mut c, Toggle::ShortLabels);
        assert_eq!(c.display.labels, LabelStyle::Full);
    }

    #[test]
    fn icon_toggle_restores_labels() {
        let mut c = Config::default();
        apply_toggle(&mut c, Toggle::Icons);
        assert!(c.display.icons);
        c.display.labels = LabelStyle::None;
        apply_toggle(&mut c, Toggle::Icons);
        assert!(!c.display.icons);
        assert_eq!(
            c.display.labels,
            LabelStyle::Full,
            "a cell never ends up with no label at all"
        );
        assert_eq!(command_for(ID_ICONS), Some(Command::Toggle(Toggle::Icons)));
    }
}
