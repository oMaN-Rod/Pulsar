import {
  ITEM_KEYS,
  isItemKey,
  META,
  parseToml,
  type ItemKey,
  type TomlTable,
  type TomlValue,
} from './theme.ts';

export interface Choice<T extends string = string> {
  id: T;
  name: string;
}

export type GraphStyle =
  | 'none'
  | 'area'
  | 'line'
  | 'bars'
  | 'meter'
  | 'segments'
  | 'spark'
  | 'ring'
  | 'dot_matrix'
  | 'heat_strip'
  | 'bullet'
  | 'range_rail'
  | 'tint'
  | 'bubble'
  | 'cores'
  | 'heat'
  | 'core_types'
  | 'combined'
  | 'mirror_bars'
  | 'uptime'
  | 'battery_cell';

export const GRAPH_STYLES: (Choice<GraphStyle> & { only?: ItemKey[] })[] = [
  { id: 'area', name: 'Area' },
  { id: 'line', name: 'Line' },
  { id: 'bars', name: 'Bars' },
  { id: 'meter', name: 'Meter' },
  { id: 'segments', name: 'Segments' },
  { id: 'spark', name: 'Sparkline' },
  { id: 'ring', name: 'Ring' },
  { id: 'dot_matrix', name: 'Dot matrix' },
  { id: 'heat_strip', name: 'Heat strip' },
  { id: 'bullet', name: 'Bullet' },
  { id: 'range_rail', name: 'Range rail' },
  { id: 'tint', name: 'Tint' },
  { id: 'bubble', name: 'Bubble' },
  { id: 'cores', name: 'Cores', only: ['cpu'] },
  { id: 'heat', name: 'Core heat', only: ['cpu'] },
  { id: 'core_types', name: 'Core types', only: ['cpu'] },
  { id: 'combined', name: 'Bars with temperature', only: ['cpu', 'gpu'] },
  { id: 'mirror_bars', name: 'Mirror bars', only: ['network'] },
  { id: 'uptime', name: 'Uptime', only: ['ping'] },
  { id: 'battery_cell', name: 'Battery cell', only: ['battery'] },
];

export const stylesFor = (key: unknown) =>
  GRAPH_STYLES.filter((s) => !s.only || (s.only as unknown[]).includes(key));

export type WidgetStyle = 'sparklines' | 'tiles' | 'meters' | 'dials' | 'core_wall' | 'cards';

export const WIDGET_STYLES: Choice<WidgetStyle>[] = [
  { id: 'sparklines', name: 'Sparklines' },
  { id: 'tiles', name: 'Tiles' },
  { id: 'meters', name: 'Meters' },
  { id: 'dials', name: 'Dials' },
  { id: 'cards', name: 'Device cards' },
  { id: 'core_wall', name: 'Core wall' },
];

export type WidgetLayout =
  'auto' | 'dashboard' | 'lanes' | 'tiles' | 'meters' | 'dials' | 'strip' | 'rings' | 'focus';

export const WIDGET_LAYOUTS: Choice<WidgetLayout>[] = [
  { id: 'auto', name: 'Automatic' },
  { id: 'dashboard', name: 'Dashboard' },
  { id: 'lanes', name: 'Lanes' },
  { id: 'tiles', name: 'Tiles' },
  { id: 'meters', name: 'Meters' },
  { id: 'dials', name: 'Dials' },
  { id: 'strip', name: 'Strip' },
  { id: 'rings', name: 'Rings' },
  { id: 'focus', name: 'Focus' },
];

/** Popups saved before materials were "translucent", now Acrylic. */
function cleanPopup(
  pop: Omit<Profile['popup'], 'background' | 'glassTint'> & {
    background: unknown;
    glassTint: unknown;
  },
  base: Profile['popup'],
): Profile['popup'] {
  const background = pop.background === 'translucent' ? 'acrylic' : pop.background;
  return {
    ...pop,
    background: pick(background, ids(WIDGET_MATERIALS), base.background),
    glassTint: whole(pop.glassTint, 0, 100, base.glassTint),
  };
}

export type WidgetMaterial = 'solid' | 'glass' | 'liquid' | 'acrylic' | 'clear';

export const WIDGET_MATERIALS: Choice<WidgetMaterial>[] = [
  { id: 'solid', name: 'Solid' },
  { id: 'glass', name: 'Glass' },
  { id: 'liquid', name: 'Liquid glass' },
  { id: 'acrylic', name: 'Acrylic' },
  { id: 'clear', name: 'Clear' },
];

export type ScreenEdge = 'left' | 'top' | 'right' | 'bottom';
export const SCREEN_EDGES: ScreenEdge[] = ['left', 'top', 'right', 'bottom'];
export type PushStrength = 'light' | 'medium' | 'firm';
export type OverFullscreen = 'never' | 'hotkey' | 'hotkey_and_edge';
export type FocusTransition = 'none' | 'fade' | 'slide' | 'slide_up';
export type StripShape = 'rectangle' | 'pill' | 'chips';

export const SHAPE_RADII = [0, 24] as const;
export const ROW_RADII = [0, 30] as const;
export const HIDE_DELAYS_MS = [0, 5000] as const;
export const FOCUS_INTERVALS_S = [2, 120] as const;
export const FOCUS_TRANSITIONS_MS = [100, 1500] as const;
export const WIDGET_WIDTHS = [220, 600] as const;
export const WIDGET_CHOSEN_WIDTHS = [100, 2400] as const;
export const WIDGET_CHOSEN_HEIGHTS = [32, 1600] as const;

export type Detail = 'simple' | 'standard' | 'detailed';

export const DETAILS: Choice<Detail>[] = [
  { id: 'simple', name: 'Simple' },
  { id: 'standard', name: 'Standard' },
  { id: 'detailed', name: 'Detailed' },
];

export type ColorMode = 'item' | 'accent' | 'load';

export const COLOR_MODES: Choice<ColorMode>[] = [
  { id: 'item', name: 'Item color' },
  { id: 'accent', name: 'Windows accent' },
  { id: 'load', name: 'By load' },
];

export const FONTS = [
  'Segoe UI Variable',
  'Segoe UI',
  'Bahnschrift',
  'Cascadia Code',
  'Consolas',
  'Calibri',
  'Arial',
  'Verdana',
  'Tahoma',
];

export type LoadUnit = 'percent' | 'celsius' | 'millis';

export const LOAD_UNITS: (Choice<LoadUnit> & { suffix: string; max: number })[] = [
  { id: 'percent', name: 'Percent', suffix: '%', max: 100 },
  { id: 'celsius', name: 'Temperature', suffix: '°C', max: 150 },
  { id: 'millis', name: 'Ping', suffix: 'ms', max: 5000 },
];

export type NetworkShow = 'both' | 'download' | 'upload';
const NETWORK_SHOWS: NetworkShow[] = ['both', 'download', 'upload'];
export const MAX_BPS = 1e12;

export interface Overrides {
  tile: string | null;
  tileOpacity: number | null;
  panel: string | null;
  panelOpacity: number | null;
  label: string | null;
  value: string | null;
  labelLight: string | null;
  valueLight: string | null;
}

const OVERRIDE_KEYS: [keyof Overrides, string][] = [
  ['tile', 'tile_color'],
  ['tileOpacity', 'tile_opacity'],
  ['panel', 'panel_color'],
  ['panelOpacity', 'panel_opacity'],
  ['label', 'label_color'],
  ['value', 'value_color'],
  ['labelLight', 'label_color_light'],
  ['valueLight', 'value_color_light'],
];

const isOpacity = (k: keyof Overrides) => k.endsWith('Opacity');

function cleanOverride(k: keyof Overrides, v: unknown) {
  return isOpacity(k) ? sized(v, 0, 100) : hexOrNull(v);
}

export type LoadLevel = 'low' | 'moderate' | 'high' | 'critical';

export interface ItemSettings {
  taskbar: boolean;
  widget: boolean;
  style: GraphStyle;
  graph: boolean;
  colorMode: ColorMode;
  trend: boolean;
  popup: Detail | null;
  tileWidth: number | null;
  tileHeight: number | null;
  color: string | null;
  label: string | null;
  shortLabel: string | null;
  popupWidth: number | null;
  popupGraphHeight: number | null;
  widgetStyle: WidgetStyle | null;
  widgetDetail: Detail | null;
  /** Text mode: the label above the value, in a column of its own. */
  labelAbove: boolean;
  /** Network only: the upload color, as `color` is the download one. */
  upColor: string | null;
}

/** An imported profile file and the site's own writing of it at the time, as TOML. */
export interface ProfileSource {
  file: string;
  written: string;
}

export interface Profile {
  name: string;
  /** Exports keep the file's settings the site does not show or change. */
  source: ProfileSource | null;
  items: Record<ItemKey, ItemSettings>;
  order: ItemKey[];
  widgetOrder: ItemKey[];
  fit: {
    enabled: boolean;
    removeFrom: 'left' | 'right';
    noRoom: 'hide' | 'tray' | 'float';
    otherSide: boolean;
  };
  mode: 'graph' | 'text';
  labels: 'full' | 'short' | 'none';
  icons: boolean;
  inline: boolean;
  verticalLabels: boolean;
  accentGraphs: boolean;
  position: 'left' | 'right' | 'floating';
  shape: StripShape;
  /** The corner radius of a pill or chip; `null` rounds the ends fully. */
  shapeRadius: number | null;
  floatBlur: boolean;
  font: { family: string; size: number; bold: boolean };
  textGaps: { item: number; label: number; row: number };
  /** In place of the theme's colors on the overlay; `null` keeps the theme's. */
  overrides: Overrides;
  units: {
    temperature: 'celsius' | 'fahrenheit';
    network: 'bytes' | 'bits';
    memory: 'percent' | 'bytes';
  };
  network: {
    show: NetworkShow;
    uploadOnTop: boolean;
    splitTiles: boolean;
    connectionDot: boolean;
    downLabel: string | null;
    upLabel: string | null;
    popupShow: NetworkShow;
    minDownBps: number;
    minUpBps: number;
  };
  disk: { value: 'activity' | 'used' };
  load: {
    thresholds: Record<LoadUnit, number[]>;
    colors: Record<LoadLevel, string | null>;
  };
  popupDetail: Detail;
  popup: {
    width: number;
    graphHeight: number;
    processes: number;
    background: WidgetMaterial;
    /** 0–100: how strongly the popup color tints Glass and Liquid glass. */
    glassTint: number;
    graph: boolean;
  };
  widget: {
    enabled: boolean;
    layer: 'desktop' | 'on_top';
    style: WidgetStyle;
    detail: Detail;
    width: number;
    opacity: number;
    background: string | null;
    material: WidgetMaterial;
    /** 0–100: how strongly the background color tints Glass and Liquid glass. */
    glassTint: number;
    separate: boolean;
    border: boolean;
    networkInline: boolean;
    sizing: 'content' | 'chosen';
    layout: WidgetLayout;
    /** Logical pixels, while sized by dragging. */
    size: [number, number];
    /** The corners of the Strip and Rings layouts; `null` rounds the ends fully. */
    rowRadius: number | null;
    focus: {
      cycle: boolean;
      intervalS: number;
      transition: FocusTransition;
      transitionMs: number;
    };
    /** `edge: null` leaves the widget where it was put. */
    dock: {
      edge: ScreenEdge | null;
      push: PushStrength;
      hideDelayMs: number;
      lip: boolean;
      overFullscreen: OverFullscreen;
    };
  };
}

/** Three ascending whole numbers within the unit's range, or null. */
export function thresholdsFor(unit: string, list: unknown): number[] | null {
  const u = LOAD_UNITS.find((x) => x.id === unit);
  if (!u || !Array.isArray(list) || list.length !== 3 || !list.every(Number.isFinite)) return null;
  return list.map((n: number) => Math.max(0, Math.min(u.max, Math.round(n)))).sort((a, b) => a - b);
}

const HEX = /^#[0-9a-fA-F]{6}$/;
const hexOrNull = (c: unknown) => (typeof c === 'string' && HEX.test(c) ? c.toUpperCase() : null);

/** Font names go into CSS, so only the characters font names use pass. */
export const fontName = (s: unknown) =>
  typeof s === 'string' ? s.replace(/[^\p{L}\p{N} ._-]/gu, '').slice(0, 64) : '';

const POSITIONS = ['left', 'right', 'floating'] as const;
const DISK_VALUES = ['activity', 'used'] as const;

export const MAX_LABEL_CHARS = 12;

const graphemes = (s: string) =>
  [...new Intl.Segmenter(undefined, { granularity: 'grapheme' }).segment(s)].map((g) => g.segment);

const labelChars = (s: string) => s.replace(/[\p{Cc}\u2028-\u202E\u2066-\u2069]/gu, '');

/** A label while it is typed, as Pulsar keeps it: spaces stay so the next word can follow. */
export const typedLabel = (s: string) =>
  graphemes(labelChars(s)).slice(0, MAX_LABEL_CHARS).join('');

/** A label as Pulsar stores it; `null` when nothing is left, which means the default. */
export function cleanLabel(s: unknown): string | null {
  if (typeof s !== 'string') return null;
  const label = graphemes(labelChars(s).trim()).slice(0, MAX_LABEL_CHARS).join('').trimEnd();
  return label || null;
}

const sized = (v: unknown, min: number, max: number) =>
  typeof v === 'number' && Number.isInteger(v) ? Math.max(min, Math.min(max, v)) : null;

export const POPUP_WIDTHS = [220, 800] as const;
export const TEXT_GAPS = { item: [0, 40], label: [0, 16], row: [0, 16] } as const;
export const POPUP_GRAPH_HEIGHTS = [24, 400] as const;

function cleanItem(k: ItemKey, saved: Partial<ItemSettings> | undefined): ItemSettings {
  const it = { ...defaultItem(k), ...saved };
  return {
    ...it,
    color: hexOrNull(it.color),
    labelAbove: it.labelAbove === true,
    upColor: k === 'network' ? hexOrNull(it.upColor) : null,
    label: cleanLabel(it.label),
    shortLabel: cleanLabel(it.shortLabel),
    popupWidth: sized(it.popupWidth, ...POPUP_WIDTHS),
    popupGraphHeight: sized(it.popupGraphHeight, ...POPUP_GRAPH_HEIGHTS),
  };
}

const ON_TASKBAR: ItemKey[] = ['cpu', 'ram', 'gpu', 'disk', 'network'];

export function defaultItem(k: ItemKey): ItemSettings {
  return {
    taskbar: ON_TASKBAR.includes(k),
    widget: ON_TASKBAR.includes(k),
    style: 'area',
    graph: true,
    colorMode: 'item',
    trend: false,
    popup: null,
    tileWidth: null,
    tileHeight: null,
    color: null,
    label: null,
    shortLabel: null,
    popupWidth: null,
    popupGraphHeight: null,
    widgetStyle: null,
    widgetDetail: null,
    labelAbove: false,
    upColor: null,
  };
}

export function defaultProfile(): Profile {
  return {
    name: 'My setup',
    source: null,
    items: Object.fromEntries(ITEM_KEYS.map((k) => [k, defaultItem(k)])) as Record<
      ItemKey,
      ItemSettings
    >,
    order: [...ITEM_KEYS],
    widgetOrder: [],
    fit: { enabled: true, removeFrom: 'right', noRoom: 'float', otherSide: true },
    mode: 'graph',
    labels: 'full',
    icons: false,
    inline: false,
    verticalLabels: false,
    accentGraphs: false,
    position: 'right',
    shape: 'rectangle',
    shapeRadius: null,
    floatBlur: true,
    font: { family: '', size: 9, bold: false },
    textGaps: { item: 10, label: 4, row: 0 },
    overrides: {
      tile: null,
      tileOpacity: null,
      panel: null,
      panelOpacity: null,
      label: null,
      value: null,
      labelLight: null,
      valueLight: null,
    },
    units: { temperature: 'celsius', network: 'bytes', memory: 'percent' },
    network: {
      show: 'both',
      uploadOnTop: false,
      splitTiles: false,
      connectionDot: false,
      downLabel: null,
      upLabel: null,
      popupShow: 'both',
      minDownBps: 0,
      minUpBps: 0,
    },
    disk: { value: 'activity' },
    load: {
      thresholds: { percent: [50, 75, 90], celsius: [70, 80, 90], millis: [50, 100, 200] },
      colors: { low: null, moderate: null, high: null, critical: null },
    },
    popupDetail: 'standard',
    popup: {
      width: 300,
      graphHeight: 72,
      processes: 5,
      background: 'acrylic',
      glassTint: 30,
      graph: true,
    },
    widget: {
      enabled: true,
      layer: 'desktop',
      style: 'sparklines',
      detail: 'simple',
      width: 280,
      opacity: 90,
      background: null,
      material: 'solid',
      glassTint: 0,
      separate: false,
      border: false,
      networkInline: false,
      sizing: 'content',
      layout: 'auto',
      size: [340, 420],
      rowRadius: null,
      focus: { cycle: false, intervalS: 8, transition: 'fade', transitionMs: 350 },
      dock: {
        edge: null,
        push: 'medium',
        hideDelayMs: 300,
        lip: false,
        overFullscreen: 'hotkey',
      },
    },
  };
}

const LAYERS = ['desktop', 'on_top'] as const;
const SIZINGS = ['content', 'chosen'] as const;
const SHAPES = ['rectangle', 'pill', 'chips'] as const;
const PUSHES = ['light', 'medium', 'firm'] as const;
const OVER_FULLSCREEN = ['never', 'hotkey', 'hotkey_and_edge'] as const;
const TRANSITIONS = ['none', 'fade', 'slide', 'slide_up'] as const;

type SavedWidget = Partial<Omit<Profile['widget'], 'focus' | 'dock'>> & {
  focus?: Partial<Profile['widget']['focus']>;
  dock?: Partial<Profile['widget']['dock']>;
};

/** `w` with every field in range, as Pulsar loads it. */
function cleanWidget(w: SavedWidget): Profile['widget'] {
  const base = defaultProfile().widget;
  const focus: Record<string, unknown> = { ...w.focus };
  const dock: Record<string, unknown> = { ...w.dock };
  const size = Array.isArray(w.size) ? w.size : base.size;
  return {
    enabled: typeof w.enabled === 'boolean' ? w.enabled : base.enabled,
    layer: pick(w.layer, LAYERS, base.layer),
    style: pick(w.style, ids(WIDGET_STYLES), base.style),
    detail: pick(w.detail, ids(DETAILS), base.detail),
    width: whole(w.width, ...WIDGET_WIDTHS, base.width),
    opacity: whole(w.opacity, 0, 100, base.opacity),
    background: hexOrNull(w.background),
    material: pick(w.material, ids(WIDGET_MATERIALS), base.material),
    glassTint: whole(w.glassTint, 0, 100, base.glassTint),
    separate: w.separate === true,
    border: w.border === true,
    networkInline: w.networkInline === true,
    sizing: pick(w.sizing, SIZINGS, base.sizing),
    layout: pick(w.layout, ids(WIDGET_LAYOUTS), base.layout),
    size: [
      whole(size[0], ...WIDGET_CHOSEN_WIDTHS, base.size[0]),
      whole(size[1], ...WIDGET_CHOSEN_HEIGHTS, base.size[1]),
    ],
    rowRadius: sized(w.rowRadius, ...ROW_RADII),
    focus: {
      cycle: focus.cycle === true,
      intervalS: whole(focus.intervalS, ...FOCUS_INTERVALS_S, base.focus.intervalS),
      transition: pick(focus.transition, TRANSITIONS, base.focus.transition),
      transitionMs: whole(focus.transitionMs, ...FOCUS_TRANSITIONS_MS, base.focus.transitionMs),
    },
    dock: {
      edge: pick(dock.edge, SCREEN_EDGES, null),
      push: pick(dock.push, PUSHES, base.dock.push),
      hideDelayMs: whole(dock.hideDelayMs, ...HIDE_DELAYS_MS, base.dock.hideDelayMs),
      lip: dock.lip === true,
      overFullscreen: pick(dock.overFullscreen, OVER_FULLSCREEN, base.dock.overFullscreen),
    },
  };
}

type Saved = {
  [K in keyof Profile]?: Profile[K] extends object ? Partial<Profile[K]> | null : unknown;
} & Record<string, unknown>;

/** Fills in what an older saved profile lacks. */
export function mergeProfile(input: unknown): Profile {
  const base = defaultProfile();
  const saved = input as Saved | null | undefined;
  if (!saved?.items) return base;
  const savedItems = saved.items as Partial<Record<ItemKey, Partial<ItemSettings>>>;
  const font = { ...base.font, ...saved.font };
  const net = { ...base.network, ...saved.network };
  const source = saved.source as ProfileSource | null | undefined;
  return {
    ...base,
    ...(saved as Partial<Profile>),
    source: typeof source?.file === 'string' && typeof source.written === 'string' ? source : null,
    position: pick(saved.position, POSITIONS, base.position),
    shape: pick(saved.shape, SHAPES, base.shape),
    shapeRadius: sized(saved.shapeRadius, ...SHAPE_RADII),
    floatBlur: saved.floatBlur !== false,
    font: {
      ...font,
      family: fontName(font.family),
      size: Number.isFinite(font.size) ? Math.max(7, Math.min(14, font.size)) : base.font.size,
    },
    textGaps: {
      item: sized(saved.textGaps?.item, ...TEXT_GAPS.item) ?? base.textGaps.item,
      label: sized(saved.textGaps?.label, ...TEXT_GAPS.label) ?? base.textGaps.label,
      row: sized(saved.textGaps?.row, ...TEXT_GAPS.row) ?? base.textGaps.row,
    },
    overrides: Object.fromEntries(
      OVERRIDE_KEYS.map(([k]) => [k, cleanOverride(k, saved.overrides?.[k])]),
    ) as unknown as Overrides,
    units: { ...base.units, ...saved.units },
    network: {
      show: pick(net.show, NETWORK_SHOWS, base.network.show),
      uploadOnTop: net.uploadOnTop === true,
      splitTiles: net.splitTiles === true,
      connectionDot: net.connectionDot === true,
      downLabel: cleanLabel(net.downLabel),
      upLabel: cleanLabel(net.upLabel),
      popupShow: pick(net.popupShow, NETWORK_SHOWS, base.network.popupShow),
      minDownBps: sized(net.minDownBps, 0, MAX_BPS) ?? 0,
      minUpBps: sized(net.minUpBps, 0, MAX_BPS) ?? 0,
    },
    load: {
      thresholds: Object.fromEntries(
        LOAD_UNITS.map((u) => [
          u.id,
          thresholdsFor(u.id, saved.load?.thresholds?.[u.id]) ?? base.load.thresholds[u.id],
        ]),
      ) as Profile['load']['thresholds'],
      colors: Object.fromEntries(
        (Object.keys(base.load.colors) as LoadLevel[]).map((k) => [
          k,
          hexOrNull(saved.load?.colors?.[k]),
        ]),
      ) as Profile['load']['colors'],
    },
    disk: { value: pick(saved.disk?.value, DISK_VALUES, base.disk.value) },
    popup: cleanPopup({ ...base.popup, ...saved.popup }, base.popup),
    widget: cleanWidget((saved.widget ?? {}) as SavedWidget),
    fit: { ...base.fit, ...saved.fit },
    order: complete(saved.order),
    widgetOrder:
      Array.isArray(saved.widgetOrder) && saved.widgetOrder.length
        ? complete(saved.widgetOrder)
        : [],
    items: Object.fromEntries(ITEM_KEYS.map((k) => [k, cleanItem(k, savedItems[k])])) as Record<
      ItemKey,
      ItemSettings
    >,
  };
}

/** `list` with unknown keys dropped and any missing ones added at the end. */
function complete(list: unknown): ItemKey[] {
  const known = (Array.isArray(list) ? list : []).filter(
    (k, i, a): k is ItemKey => isItemKey(k) && a.indexOf(k) === i,
  );
  return [...known, ...ITEM_KEYS.filter((k) => !known.includes(k))];
}

export const taskbarKeys = (p: Profile) => p.order.filter((k) => p.items[k].taskbar);

export const widgetKeys = (p: Profile) =>
  (p.widgetOrder.length ? p.widgetOrder : p.order).filter((k) => p.items[k].widget);

/** Moves `key` to `index` among the keys shown on `where`, keeping the hidden ones in place. */
export function moveKey(p: Profile, where: 'taskbar' | 'widget', key: ItemKey, index: number) {
  if (where === 'widget' && !p.widgetOrder.length) p.widgetOrder = [...p.order];
  const list = where === 'widget' ? p.widgetOrder : p.order;
  const shown = list.filter((k) => p.items[k][where] && k !== key);
  const target = Math.max(0, Math.min(shown.length, index));
  const without = list.filter((k) => k !== key);
  const at =
    target < shown.length
      ? without.indexOf(shown[target])
      : without.indexOf(shown.at(-1) as ItemKey) + 1;
  without.splice(shown.length ? at : without.length, 0, key);
  list.splice(0, list.length, ...without);
}

export function profileToToml(p: Profile, themeId: string, colorLabels: boolean): string {
  const text = siteToml(p, themeId, colorLabels);
  if (!p.source) return text;
  const { file, written } = p.source;
  return tomlToString(applyChanges(parseToml(file), parseToml(written), parseToml(text)));
}

function siteToml(p: Profile, themeId: string, colorLabels: boolean): string {
  const q = (s: string) => JSON.stringify(s);
  const lines = [`name = ${q(p.name)}`, '', '[display]'];
  const kv = (k: string, v: string | number | boolean | number[]) =>
    lines.push(
      `${k} = ${typeof v === 'string' ? q(v) : Array.isArray(v) ? `[${v.join(', ')}]` : v}`,
    );
  kv('mode', p.mode);
  if (themeId !== 'windows') kv('theme', themeId);
  kv('position', p.position);
  if (p.shape !== 'rectangle') kv('shape', p.shape);
  if (p.shapeRadius !== null) kv('shape_radius', p.shapeRadius);
  if (!p.floatBlur) kv('float_blur', false);
  kv('labels', p.labels);
  kv('icons', p.icons || p.labels === 'none');
  kv('graph_inline', p.inline);
  kv('vertical_labels', p.verticalLabels);
  kv('color_labels', colorLabels);
  kv('accent_graphs', p.accentGraphs);
  kv('upload_on_top', p.network.uploadOnTop);
  kv('fit_to_taskbar', p.fit.enabled);
  kv('fit_remove_from', p.fit.removeFrom);
  kv('fit_no_room', p.fit.noRoom);
  kv('fit_other_side', p.fit.otherSide);
  if (p.font.family && p.font.family !== FONTS[0]) kv('font_family', p.font.family);
  kv('font_bold', p.font.bold);
  lines.push(`font_size_pt = ${Number(p.font.size).toFixed(1)}`);
  for (const [k, key] of OVERRIDE_KEYS) {
    const v = p.overrides[k];
    if (v !== null) kv(key, v);
  }
  const gaps = defaultProfile().textGaps;
  for (const k of ['item', 'label', 'row'] as const)
    if (p.textGaps[k] !== gaps[k]) kv(`text_${k}_gap`, p.textGaps[k]);
  lines.push('', '[display.load_thresholds]');
  for (const u of LOAD_UNITS) kv(u.id, p.load.thresholds[u.id]);
  const colors = Object.entries(p.load.colors).filter(([, c]) => c) as [string, string][];
  if (colors.length) {
    lines.push('', '[display.load_colors]');
    for (const [k, c] of colors) kv(k, c);
  }
  lines.push('', '[units]');
  for (const [k, v] of Object.entries(p.units)) kv(k, v);
  lines.push('', '[network]');
  const n = p.network;
  kv('show', n.show);
  if (n.splitTiles) kv('split_tiles', true);
  if (n.connectionDot) kv('connection_dot', true);
  const down = cleanLabel(n.downLabel);
  const up = cleanLabel(n.upLabel);
  if (down) kv('down_label', down);
  if (up) kv('up_label', up);
  if (n.popupShow !== 'both') kv('popup_show', n.popupShow);
  if (n.minDownBps) kv('min_down_bps', n.minDownBps);
  if (n.minUpBps) kv('min_up_bps', n.minUpBps);
  if (p.disk.value !== 'activity') lines.push('', '[disk]', `value = ${q(p.disk.value)}`);
  lines.push('', '[popup]');
  kv('detail', p.popupDetail);
  kv('background', p.popup.background);
  kv('glass_tint', p.popup.glassTint);
  kv('graph', p.popup.graph);
  kv('processes', p.popup.processes);
  kv('width', p.popup.width);
  kv('graph_height', p.popup.graphHeight);
  const widgetItems = widgetKeys(p);
  const w = p.widget;
  lines.push('', '[widget]');
  kv('enabled', w.enabled && widgetItems.length > 0);
  if (w.layer !== 'desktop') kv('layer', w.layer);
  kv('style', w.style);
  kv('detail', w.detail);
  kv('width', w.width);
  kv('opacity', w.opacity);
  if (w.background) kv('background', w.background);
  kv('material', w.material);
  if (w.glassTint) kv('glass_tint', w.glassTint);
  if (w.separate) kv('separate', true);
  if (w.border) kv('border', true);
  if (w.networkInline) kv('network_inline', true);
  kv('sizing', w.sizing);
  kv('layout', w.layout);
  kv('size', w.size);
  if (w.rowRadius !== null) kv('row_radius', w.rowRadius);
  if (w.focus.cycle) kv('focus_cycle', true);
  kv('focus_interval_s', w.focus.intervalS);
  kv('focus_transition', w.focus.transition);
  kv('focus_transition_ms', w.focus.transitionMs);
  lines.push(`items = [${widgetItems.map(q).join(', ')}]`);
  if (p.widgetOrder.length) lines.push(`order = [${p.widgetOrder.map(q).join(', ')}]`);
  if (w.dock.edge) {
    lines.push('', '[widget.dock]');
    kv('edge', w.dock.edge);
    kv('push', w.dock.push);
    kv('hide_delay_ms', w.dock.hideDelayMs);
    kv('lip', w.dock.lip);
    kv('over_fullscreen', w.dock.overFullscreen);
  }
  for (const k of p.order) {
    const it = p.items[k];
    lines.push('', '[[items]]');
    kv('kind', k);
    kv('enabled', it.taskbar);
    kv('graph', it.graph);
    kv('style', it.style);
    kv('color_mode', it.colorMode);
    kv('trend', it.trend);
    if (it.popup) kv('popup', it.popup);
    if (it.tileWidth) kv('tile_width', it.tileWidth);
    if (it.tileHeight) kv('tile_height', it.tileHeight);
    if (it.color) kv('color', it.color);
    if (k === 'network' && it.upColor) kv('up_color', it.upColor);
    if (it.labelAbove) kv('label_above', true);
    const label = cleanLabel(it.label);
    const shortLabel = cleanLabel(it.shortLabel);
    if (label) kv('label', label);
    if (shortLabel) kv('short_label', shortLabel);
    if (it.popupWidth) kv('popup_width', it.popupWidth);
    if (it.popupGraphHeight) kv('popup_graph_height', it.popupGraphHeight);
    if (it.widgetStyle) kv('widget_style', it.widgetStyle);
    if (it.widgetDetail) kv('widget_detail', it.widgetDetail);
  }
  return `${lines.join('\n')}\n`;
}

export const itemName = (k: ItemKey) => META[k].name;

const pick = <T extends string, F>(value: unknown, allowed: readonly T[], fallback: F): T | F =>
  (allowed as readonly unknown[]).includes(value) ? (value as T) : fallback;
const ids = <T extends string>(list: Choice<T>[]) => list.map((x) => x.id);
const whole = (value: unknown, min: number, max: number, fallback: number) =>
  typeof value === 'number' && Number.isInteger(value)
    ? Math.max(min, Math.min(max, value))
    : fallback;
const table = (v: TomlValue | undefined): TomlTable =>
  typeof v === 'object' && v !== null && !Array.isArray(v) ? v : {};

export interface ProfileFile {
  profile: Profile;
  themeId: string;
  colorLabels: boolean;
}

/**
 * Reads a Pulsar profile file. Anything missing or not understood keeps the
 * default, as Pulsar does. Returns the profile and the theme id it names.
 */
export function profileFromToml(doc: TomlTable, fallbackName: string): ProfileFile {
  const p = defaultProfile();
  const d = table(doc.display);
  p.name = typeof doc.name === 'string' && doc.name.trim() ? doc.name.trim() : fallbackName;
  p.mode = pick(d.mode, ['graph', 'text'], p.mode);
  p.position = pick(d.position, POSITIONS, p.position);
  p.shape = pick(d.shape, SHAPES, p.shape);
  p.shapeRadius = sized(d.shape_radius, ...SHAPE_RADII);
  p.floatBlur = d.float_blur !== false;
  p.labels = pick(d.labels, ['full', 'short', 'none'], p.labels);
  p.icons = d.icons === true;
  p.inline = d.graph_inline === true;
  p.verticalLabels = d.vertical_labels === true;
  p.accentGraphs = d.accent_graphs === true;
  p.network.uploadOnTop = d.upload_on_top === true;
  if (typeof d.fit_to_taskbar === 'boolean') p.fit.enabled = d.fit_to_taskbar;
  p.fit.removeFrom = pick(d.fit_remove_from, ['left', 'right'], p.fit.removeFrom);
  p.fit.noRoom = pick(d.fit_no_room, ['hide', 'tray', 'float'], p.fit.noRoom);
  if (typeof d.fit_other_side === 'boolean') p.fit.otherSide = d.fit_other_side;
  if (typeof d.font_family === 'string')
    p.font.family = d.font_family === FONTS[0] ? '' : fontName(d.font_family);
  p.font.bold = d.font_bold === true;
  if (typeof d.font_size_pt === 'number')
    p.font.size = Math.max(7, Math.min(14, Math.round(d.font_size_pt * 2) / 2));
  p.overrides = Object.fromEntries(
    OVERRIDE_KEYS.map(([k, key]) => [k, cleanOverride(k, d[key])]),
  ) as unknown as Overrides;
  for (const k of ['item', 'label', 'row'] as const) {
    const [min, max] = TEXT_GAPS[k];
    p.textGaps[k] = sized(d[`text_${k}_gap`], min, max) ?? p.textGaps[k];
  }
  const thresholds = table(d.load_thresholds);
  for (const u of LOAD_UNITS) {
    const t = thresholds[u.id];
    const ok = Array.isArray(t) && t.every(Number.isInteger) && thresholdsFor(u.id, t);
    if (ok) p.load.thresholds[u.id] = ok;
  }
  const loadColors = table(d.load_colors);
  for (const k of Object.keys(p.load.colors) as LoadLevel[]) {
    const c = loadColors[k];
    if (typeof c === 'string' && /^#[0-9a-fA-F]{6}$/.test(c)) p.load.colors[k] = c.toUpperCase();
  }
  const u = table(doc.units);
  p.units.temperature = pick(u.temperature, ['celsius', 'fahrenheit'], p.units.temperature);
  p.units.network = pick(u.network, ['bytes', 'bits'], p.units.network);
  p.units.memory = pick(u.memory, ['percent', 'bytes'], p.units.memory);
  const n = table(doc.network);
  p.network.show = pick(n.show, NETWORK_SHOWS, p.network.show);
  p.network.splitTiles = n.split_tiles === true;
  p.network.connectionDot = n.connection_dot === true;
  p.network.downLabel = cleanLabel(n.down_label);
  p.network.upLabel = cleanLabel(n.up_label);
  p.network.popupShow = pick(n.popup_show, NETWORK_SHOWS, p.network.popupShow);
  p.network.minDownBps = sized(n.min_down_bps, 0, MAX_BPS) ?? 0;
  p.network.minUpBps = sized(n.min_up_bps, 0, MAX_BPS) ?? 0;
  p.disk.value = pick(table(doc.disk).value, DISK_VALUES, p.disk.value);
  const pop = table(doc.popup);
  p.popupDetail = pick(pop.detail, ids(DETAILS), p.popupDetail);
  p.popup = cleanPopup(
    { ...p.popup, background: pop.background, glassTint: pop.glass_tint },
    p.popup,
  );
  if (typeof pop.graph === 'boolean') p.popup.graph = pop.graph;
  p.popup.processes = whole(pop.processes, 0, 15, p.popup.processes);
  p.popup.width = whole(pop.width, 220, 800, p.popup.width);
  p.popup.graphHeight = whole(pop.graph_height, 24, 400, p.popup.graphHeight);
  const w = table(doc.widget);
  const dock = table(w.dock);
  p.widget = cleanWidget({
    enabled: w.enabled !== false,
    layer: w.layer,
    style: w.style,
    detail: w.detail,
    width: w.width,
    opacity: w.opacity,
    background: w.background,
    material: w.material,
    glassTint: w.glass_tint,
    separate: w.separate,
    border: w.border,
    networkInline: w.network_inline,
    sizing: w.sizing,
    layout: w.layout,
    size: w.size,
    rowRadius: w.row_radius,
    focus: {
      cycle: w.focus_cycle,
      intervalS: w.focus_interval_s,
      transition: w.focus_transition,
      transitionMs: w.focus_transition_ms,
    },
    dock: {
      edge: dock.edge,
      push: dock.push,
      hideDelayMs: dock.hide_delay_ms,
      lip: dock.lip,
      overFullscreen: dock.over_fullscreen,
    },
  } as SavedWidget);
  if (Array.isArray(doc.items)) {
    const entries = doc.items.map((it) => table(it as TomlValue));
    p.order = complete(entries.map((it) => it.kind));
    for (const k of ITEM_KEYS) p.items[k].taskbar = false;
    for (const it of entries) {
      if (!isItemKey(it.kind)) continue;
      const item = p.items[it.kind];
      item.taskbar = it.enabled !== false;
      item.graph = it.graph !== false;
      item.style = pick(it.style, ids(stylesFor(it.kind)), 'area');
      item.colorMode = pick(it.color_mode, ids(COLOR_MODES), 'item');
      item.trend = it.trend === true;
      item.popup = pick(it.popup, ids(DETAILS), null);
      item.tileWidth = sized(it.tile_width, 16, 400);
      item.tileHeight = sized(it.tile_height, 16, 200);
      item.color = hexOrNull(it.color);
      item.upColor = it.kind === 'network' ? hexOrNull(it.up_color) : null;
      item.labelAbove = it.label_above === true;
      item.label = cleanLabel(it.label);
      item.shortLabel = cleanLabel(it.short_label);
      item.popupWidth = sized(it.popup_width, ...POPUP_WIDTHS);
      item.popupGraphHeight = sized(it.popup_graph_height, ...POPUP_GRAPH_HEIGHTS);
      item.widgetStyle = pick(it.widget_style, ids(WIDGET_STYLES), null);
      item.widgetDetail = pick(it.widget_detail, ids(DETAILS), null);
    }
  }
  const onWidget: unknown[] =
    Array.isArray(w.items) && w.items.length
      ? w.items
      : ITEM_KEYS.filter((k) => p.items[k].taskbar);
  for (const k of ITEM_KEYS) p.items[k].widget = onWidget.includes(k);
  if (Array.isArray(w.order) && w.order.length) p.widgetOrder = complete(w.order);
  const themeId = typeof d.theme === 'string' ? d.theme : 'windows';
  const colorLabels = d.color_labels === true;
  p.source = { file: tomlToString(doc), written: siteToml(p, themeId, colorLabels) };
  return { profile: p, themeId, colorLabels };
}

const isTable = (v: unknown): v is TomlTable =>
  typeof v === 'object' && v !== null && !Array.isArray(v);
const isTableArray = (v: unknown): v is TomlTable[] =>
  Array.isArray(v) && v.length > 0 && v.every(isTable);
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** `doc` with whatever differs between `before` and `after` applied to it. */
function applyChanges(doc: TomlTable, before: TomlTable, after: TomlTable): TomlTable {
  const out: TomlTable = { ...doc };
  for (const k of new Set([...Object.keys(before), ...Object.keys(after)])) {
    const b = before[k];
    const a = after[k];
    if (k === 'items' && isTableArray(a)) {
      const items = mergeItems(
        isTableArray(doc.items) ? doc.items : [],
        isTableArray(b) ? b : [],
        a,
      );
      if (items.length || doc.items !== undefined) out.items = items;
    } else if (isTable(a) || (a === undefined && isTable(b))) {
      const merged = applyChanges(
        isTable(doc[k]) ? doc[k] : {},
        isTable(b) ? b : {},
        isTable(a) ? a : {},
      );
      if (isTable(a) || Object.keys(merged).length) out[k] = merged;
      else delete out[k];
    } else if (same(a, b)) {
      continue;
    } else if (a === undefined) {
      delete out[k];
    } else {
      out[k] = a;
    }
  }
  return out;
}

/**
 * Items matched by kind; the file's order stays unless the site's changed.
 * Items the file lacks stay out unless changed, and then go in whole.
 */
function mergeItems(doc: TomlTable[], before: TomlTable[], after: TomlTable[]): TomlTable[] {
  const kindIn = (list: TomlTable[], kind: TomlValue | undefined) =>
    list.find((t) => t.kind === kind);
  const merged = after.flatMap((a) => {
    const own = kindIn(doc, a.kind);
    const was = kindIn(before, a.kind);
    if (own) return [applyChanges(own, was ?? {}, a)];
    return was && same(a, was) ? [] : [a];
  });
  const added = merged.filter((m) => !kindIn(doc, m.kind));
  if (
    same(
      after.map((t) => t.kind),
      before.map((t) => t.kind),
    )
  )
    return [...doc.map((t) => kindIn(merged, t.kind) ?? t), ...added];
  return [...merged, ...doc.filter((t) => !kindIn(after, t.kind))];
}

function tomlToString(doc: TomlTable): string {
  const key = (k: string) => (/^[A-Za-z0-9_-]+$/.test(k) ? k : JSON.stringify(k));
  const value = (v: TomlValue): string => {
    if (typeof v === 'string') return JSON.stringify(v);
    if (Array.isArray(v)) return `[${v.map(value).join(', ')}]`;
    if (isTable(v))
      return `{ ${Object.entries(v)
        .filter((e): e is [string, TomlValue] => e[1] !== undefined)
        .map(([k, x]) => `${key(k)} = ${value(x)}`)
        .join(', ')} }`;
    return String(v);
  };
  const lines: string[] = [];
  const write = (t: TomlTable, path: string[]) => {
    const nested: [string, TomlTable | TomlTable[]][] = [];
    for (const [k, v] of Object.entries(t)) {
      if (v === undefined) continue;
      if (isTable(v) || isTableArray(v)) nested.push([k, v]);
      else lines.push(`${key(k)} = ${value(v)}`);
    }
    for (const [k, v] of nested) {
      const name = [...path, k].map(key).join('.');
      for (const table of isTableArray(v) ? v : [v]) {
        lines.push('', isTableArray(v) ? `[[${name}]]` : `[${name}]`);
        write(table, [...path, k]);
      }
    }
  };
  write(doc, []);
  return `${lines.join('\n').replace(/^\n+/, '')}\n`;
}
