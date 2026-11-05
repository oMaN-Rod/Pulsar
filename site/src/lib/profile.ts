import {
  ITEM_KEYS,
  isItemKey,
  META,
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

export type WidgetStyle = 'sparklines' | 'tiles' | 'meters' | 'dials' | 'core_wall';

export const WIDGET_STYLES: Choice<WidgetStyle>[] = [
  { id: 'sparklines', name: 'Sparklines' },
  { id: 'tiles', name: 'Tiles' },
  { id: 'meters', name: 'Meters' },
  { id: 'dials', name: 'Dials' },
  { id: 'core_wall', name: 'Core wall' },
];

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
  widgetStyle: WidgetStyle | null;
  widgetDetail: Detail | null;
}

export interface Profile {
  name: string;
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
  position: 'left' | 'right';
  font: { family: string; size: number; bold: boolean };
  units: {
    temperature: 'celsius' | 'fahrenheit';
    network: 'bytes' | 'bits';
    memory: 'percent' | 'bytes';
  };
  network: { show: 'both' | 'download' | 'upload'; uploadOnTop: boolean };
  load: {
    thresholds: Record<LoadUnit, number[]>;
    colors: Record<LoadLevel, string | null>;
  };
  popupDetail: Detail;
  popup: {
    width: number;
    graphHeight: number;
    processes: number;
    background: 'translucent' | 'solid';
    graph: boolean;
  };
  widget: {
    enabled: boolean;
    style: WidgetStyle;
    detail: Detail;
    width: number;
    opacity: number;
    background: string | null;
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
    widgetStyle: null,
    widgetDetail: null,
  };
}

export function defaultProfile(): Profile {
  return {
    name: 'My setup',
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
    font: { family: '', size: 9, bold: false },
    units: { temperature: 'celsius', network: 'bytes', memory: 'percent' },
    network: { show: 'both', uploadOnTop: false },
    load: {
      thresholds: { percent: [50, 75, 90], celsius: [70, 80, 90], millis: [50, 100, 200] },
      colors: { low: null, moderate: null, high: null, critical: null },
    },
    popupDetail: 'standard',
    popup: { width: 300, graphHeight: 72, processes: 5, background: 'translucent', graph: true },
    widget: {
      enabled: true,
      style: 'sparklines',
      detail: 'simple',
      width: 280,
      opacity: 90,
      background: null,
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
  const widget = { ...base.widget, ...saved.widget };
  return {
    ...base,
    ...(saved as Partial<Profile>),
    font: {
      ...font,
      family: fontName(font.family),
      size: Number.isFinite(font.size) ? Math.max(7, Math.min(14, font.size)) : base.font.size,
    },
    units: { ...base.units, ...saved.units },
    network: { ...base.network, ...saved.network },
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
    popup: { ...base.popup, ...saved.popup },
    widget: { ...widget, background: hexOrNull(widget.background) },
    fit: { ...base.fit, ...saved.fit },
    order: complete(saved.order),
    widgetOrder:
      Array.isArray(saved.widgetOrder) && saved.widgetOrder.length
        ? complete(saved.widgetOrder)
        : [],
    items: Object.fromEntries(
      ITEM_KEYS.map((k) => [k, { ...defaultItem(k), ...savedItems[k] }]),
    ) as Record<ItemKey, ItemSettings>,
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
  const q = (s: string) => JSON.stringify(s);
  const lines = [`name = ${q(p.name)}`, '', '[display]'];
  const kv = (k: string, v: string | number | boolean | number[]) =>
    lines.push(
      `${k} = ${typeof v === 'string' ? q(v) : Array.isArray(v) ? `[${v.join(', ')}]` : v}`,
    );
  kv('mode', p.mode);
  if (themeId !== 'windows') kv('theme', themeId);
  kv('position', p.position);
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
  kv('show', p.network.show);
  lines.push('', '[popup]');
  kv('detail', p.popupDetail);
  kv('background', p.popup.background);
  kv('graph', p.popup.graph);
  kv('processes', p.popup.processes);
  kv('width', p.popup.width);
  kv('graph_height', p.popup.graphHeight);
  const widgetItems = widgetKeys(p);
  lines.push('', '[widget]');
  kv('enabled', p.widget.enabled && widgetItems.length > 0);
  kv('style', p.widget.style);
  kv('detail', p.widget.detail);
  kv('width', p.widget.width);
  kv('opacity', p.widget.opacity);
  if (p.widget.background) kv('background', p.widget.background);
  lines.push(`items = [${widgetItems.map(q).join(', ')}]`);
  if (p.widgetOrder.length) lines.push(`order = [${p.widgetOrder.map(q).join(', ')}]`);
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
  p.position = pick(d.position, ['left', 'right'], p.position);
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
  p.network.show = pick(table(doc.network).show, ['both', 'download', 'upload'], p.network.show);
  const pop = table(doc.popup);
  p.popupDetail = pick(pop.detail, ids(DETAILS), p.popupDetail);
  p.popup.background = pick(pop.background, ['translucent', 'solid'], p.popup.background);
  if (typeof pop.graph === 'boolean') p.popup.graph = pop.graph;
  p.popup.processes = whole(pop.processes, 0, 15, p.popup.processes);
  p.popup.width = whole(pop.width, 220, 800, p.popup.width);
  p.popup.graphHeight = whole(pop.graph_height, 24, 400, p.popup.graphHeight);
  const w = table(doc.widget);
  p.widget.enabled = w.enabled !== false;
  p.widget.style = pick(w.style, ids(WIDGET_STYLES), p.widget.style);
  p.widget.detail = pick(w.detail, ids(DETAILS), p.widget.detail);
  p.widget.width = whole(w.width, 220, 600, p.widget.width);
  p.widget.opacity = whole(w.opacity, 0, 100, p.widget.opacity);
  if (typeof w.background === 'string' && /^#[0-9a-fA-F]{6}$/.test(w.background))
    p.widget.background = w.background.toUpperCase();
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
      item.tileWidth =
        typeof it.tile_width === 'number' && Number.isInteger(it.tile_width)
          ? Math.max(16, Math.min(400, it.tile_width))
          : null;
      item.tileHeight =
        typeof it.tile_height === 'number' && Number.isInteger(it.tile_height)
          ? Math.max(16, Math.min(200, it.tile_height))
          : null;
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
  return {
    profile: p,
    themeId: typeof d.theme === 'string' ? d.theme : 'windows',
    colorLabels: d.color_labels === true,
  };
}
