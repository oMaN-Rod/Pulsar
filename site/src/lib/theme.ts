import { darkenTo, isHex } from './color.ts';

export type ItemKey =
  | 'cpu'
  | 'cpu_temp'
  | 'cpu_power'
  | 'cpu_clock'
  | 'ram'
  | 'chipset_temp'
  | 'gpu'
  | 'gpu_temp'
  | 'gpu_memory'
  | 'gpu_power'
  | 'disk'
  | 'disk_temp'
  | 'network'
  | 'ping'
  | 'battery'
  | 'fps';

export type Unit = 'percent' | 'celsius' | 'millis';

export interface ItemMeta {
  key: ItemKey;
  name: string;
  short: string;
  letter: string;
  unit?: Unit;
}

export const ITEMS: ItemMeta[] = [
  { key: 'cpu', name: 'CPU', short: 'CPU', letter: 'C', unit: 'percent' },
  { key: 'cpu_temp', name: 'CPU temperature', short: 'CPU°', letter: 'H', unit: 'celsius' },
  { key: 'cpu_power', name: 'CPU power', short: 'PWR', letter: 'W' },
  { key: 'cpu_clock', name: 'CPU clock', short: 'CLK', letter: 'F' },
  { key: 'ram', name: 'Memory', short: 'RAM', letter: 'M', unit: 'percent' },
  { key: 'chipset_temp', name: 'Chipset temperature', short: 'PCH°', letter: 'B', unit: 'celsius' },
  { key: 'gpu', name: 'GPU', short: 'GPU', letter: 'G', unit: 'percent' },
  { key: 'gpu_temp', name: 'GPU temperature', short: 'TEMP', letter: 'T', unit: 'celsius' },
  { key: 'gpu_memory', name: 'GPU memory', short: 'VRAM', letter: 'V', unit: 'percent' },
  { key: 'gpu_power', name: 'GPU power', short: 'GPWR', letter: 'Q' },
  { key: 'disk', name: 'Disk', short: 'DSK', letter: 'D', unit: 'percent' },
  { key: 'disk_temp', name: 'Disk temperature', short: 'DSK°', letter: 'S', unit: 'celsius' },
  { key: 'network', name: 'Network', short: 'NET', letter: '↓' },
  { key: 'ping', name: 'Ping', short: 'PING', letter: 'P', unit: 'millis' },
  { key: 'battery', name: 'Battery', short: 'BAT', letter: 'E', unit: 'percent' },
  { key: 'fps', name: 'Frame rate', short: 'FPS', letter: 'R' },
];

export const ITEM_KEYS: ItemKey[] = ITEMS.map((i) => i.key);

export const META = Object.fromEntries(ITEMS.map((i) => [i.key, i])) as Record<ItemKey, ItemMeta>;

export const isItemKey = (k: unknown): k is ItemKey => (ITEM_KEYS as unknown[]).includes(k);

export type ItemColors = Partial<Record<ItemKey, string>>;

export interface Variant {
  text?: string;
  label?: string;
  tile?: string;
  tileOpacity?: number;
  panel?: string;
  panelOpacity?: number;
  items: ItemColors;
}

export interface Theme {
  name: string;
  author: string | null;
  basedOn: string | null;
  colorLabels: boolean;
  dark: Variant;
  light: Variant | null;
}

interface Filled {
  text: string;
  label: string;
  tile: string;
  tileOpacity: number;
  panel: string;
  panelOpacity: number;
  items: ItemColors;
}

export interface Resolved extends Omit<Filled, 'items'> {
  colorLabels: boolean;
  items: Record<ItemKey, string>;
  up: string;
  derived: boolean;
}

/** Pulsar's own item colors, used where a theme sets none. */
export const DEFAULT_ITEMS: Record<ItemKey, string> = {
  cpu: '#4CC2FF',
  ram: '#B186F6',
  disk: '#6CCB5F',
  network: '#FFB900',
  gpu: '#FF6FB5',
  gpu_temp: '#FF8C42',
  gpu_memory: '#E87BDE',
  gpu_power: '#FFB347',
  cpu_temp: '#FF5A5F',
  cpu_power: '#F5D04A',
  cpu_clock: '#8EA8FF',
  chipset_temp: '#FF9E7A',
  disk_temp: '#B5E36B',
  ping: '#2ED5C4',
  battery: '#7ED321',
  fps: '#D96BFF',
};

/** Upload's own color while a theme leaves the network color alone. */
const DEFAULT_UP = '#F7630C';

const WINDOWS: Record<'dark' | 'light', Omit<Filled, 'items'>> = {
  dark: {
    text: '#FFFFFF',
    label: '#B8B8B8',
    tile: '#FFFFFF',
    tileOpacity: 8,
    panel: '#202020',
    panelOpacity: 0,
  },
  light: {
    text: '#1A1A1A',
    label: '#5C5C5C',
    tile: '#000000',
    tileOpacity: 6,
    panel: '#F3F3F3',
    panelOpacity: 0,
  },
};

/** A dark variant this opaque also serves a light taskbar unchanged. */
const OWN_BACKGROUND_OPACITY = 50;
const LIGHT_ITEM_CONTRAST = 3;
export const MAX_FILE_BYTES = 64 * 1024;
export const MAX_CREDIT = 80;

const VARIANT_KEYS = ['text', 'label', 'tile', 'tile_opacity', 'panel', 'panel_opacity', 'items'];

export function windowsTheme(): Theme {
  return {
    name: 'Windows default',
    author: null,
    basedOn: null,
    colorLabels: false,
    dark: { items: {} },
    light: null,
  };
}

export function resolve(theme: Theme, lightTaskbar: boolean): Resolved {
  const dark = fill(theme.dark, WINDOWS.dark);
  let v: Filled = dark;
  if (lightTaskbar) {
    if (theme.light) v = fill(theme.light, WINDOWS.light);
    else if (dark.panelOpacity < OWN_BACKGROUND_OPACITY) {
      const items: ItemColors = {};
      for (const [k, c] of Object.entries(dark.items) as [ItemKey, string][])
        items[k] = darkenTo(c, WINDOWS.light.panel, LIGHT_ITEM_CONTRAST);
      v = { ...WINDOWS.light, items };
    }
  }
  return {
    ...v,
    colorLabels: theme.colorLabels,
    items: { ...DEFAULT_ITEMS, ...v.items },
    up: v.items.network ?? DEFAULT_UP,
    derived: lightTaskbar && !theme.light && dark.panelOpacity < OWN_BACKGROUND_OPACITY,
  };
}

function fill(variant: Partial<Variant>, base: Omit<Filled, 'items'>): Filled {
  const hex = (c: unknown, fallback: string) => (isHex(c) ? c : fallback);
  const share = (n: unknown, fallback: number) =>
    Number.isInteger(n) && (n as number) >= 0 && (n as number) <= 100 ? (n as number) : fallback;
  return {
    text: hex(variant.text, base.text),
    label: hex(variant.label, base.label),
    tile: hex(variant.tile, base.tile),
    tileOpacity: share(variant.tileOpacity, base.tileOpacity),
    panel: hex(variant.panel, base.panel),
    panelOpacity: share(variant.panelOpacity, base.panelOpacity),
    items: Object.fromEntries(
      Object.entries(variant.items ?? {}).filter(([k, c]) => isItemKey(k) && isHex(c)),
    ),
  };
}

/** Same rule as Pulsar's file names: lowercase words joined by dashes. */
export function slug(name: string, fallback = 'theme'): string {
  let out = '';
  for (const c of name.toLowerCase()) {
    if (/[a-z0-9]/.test(c)) out += c;
    else if (out && !out.endsWith('-')) out += '-';
  }
  out = out.replace(/-+$/, '').slice(0, 48);
  return out || fallback;
}

export function validId(id: unknown): boolean {
  return /^[a-z0-9-]{1,64}$/.test(String(id));
}

export type TomlValue = string | number | boolean | TomlValue[] | TomlTable | TomlTable[];
export interface TomlTable {
  [key: string]: TomlValue | undefined;
}

const UNSAFE_KEYS = ['__proto__', 'constructor', 'prototype'];

/**
 * Parses the TOML subset theme files use: tables, and keys holding strings,
 * integers or booleans.
 */
export function parseToml(text: string): TomlTable {
  const root: TomlTable = Object.create(null);
  let table = root;
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const at = i;
    const fail = (why: string): never => {
      throw new Error(`Line ${at + 1}: ${why}`);
    };
    const name = (key: string) =>
      UNSAFE_KEYS.includes(key) ? fail(`"${key}" can't be used as a name`) : key;
    let line = stripComment(lines[i]).trim();
    if (!line) continue;
    const list = line.match(/^\[\[\s*([A-Za-z0-9_-]+)\s*\]\]$/);
    if (list) {
      const key = name(list[1]);
      root[key] ??= [];
      const tables = root[key];
      if (!Array.isArray(tables)) fail(`"${key}" is not a list of tables`);
      table = Object.create(null);
      (tables as TomlTable[]).push(table);
      continue;
    }
    const header = line.match(/^\[\s*([A-Za-z0-9_.-]+)\s*\]$/);
    if (header) {
      table = root;
      for (const part of header[1].split('.')) {
        if (!part) fail('empty table name');
        name(part);
        table[part] ??= Object.create(null);
        const next = table[part];
        if (typeof next !== 'object' || Array.isArray(next)) fail(`"${part}" is not a table`);
        table = next as TomlTable;
      }
      continue;
    }
    while (/=\s*\[/.test(line) && depth(line) > 0 && i + 1 < lines.length)
      line += ` ${stripComment(lines[++i]).trim()}`;
    const kv = line.match(/^("(?:[^"\\]|\\.)*"|[A-Za-z0-9_-]+)\s*=\s*(.+)$/);
    if (!kv) fail('expected key = value');
    const [, rawKey, rawValue] = kv as RegExpMatchArray;
    const key = name(rawKey.startsWith('"') ? JSON.parse(rawKey) : rawKey);
    if (Object.hasOwn(table, key)) fail(`"${key}" is set twice`);
    table[key] = parseValue(rawValue.trim(), fail);
  }
  return root;
}

/** How many array brackets are still open at the end of `line`. */
function depth(line: string): number {
  let open = 0;
  let quote: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === '\\' && quote === '"') i++;
      else if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === '[') open++;
    else if (c === ']') open--;
  }
  return open;
}

function stripComment(line: string): string {
  let quote: string | null = null;
  for (let i = 0; i < line.length; i++) {
    const c = line[i];
    if (quote) {
      if (c === '\\' && quote === '"') i++;
      else if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === '#') return line.slice(0, i);
  }
  return line;
}

function parseValue(v: string, fail: (why: string) => never): TomlValue {
  if (/^"(?:[^"\\]|\\.)*"$/.test(v)) return JSON.parse(v);
  if (/^'[^']*'$/.test(v)) return v.slice(1, -1);
  if (/^[+-]?\d+$/.test(v)) return Number(v);
  if (/^[+-]?\d+\.\d+$/.test(v)) return Number(v);
  if (v === 'true' || v === 'false') return v === 'true';
  if (v.startsWith('[') && v.endsWith(']')) {
    const inner = v.slice(1, -1).trim();
    return inner ? splitList(inner).map((item) => parseValue(item.trim(), fail)) : [];
  }
  return fail(`unsupported value ${v}`);
}

function splitList(text: string): string[] {
  const items: string[] = [];
  let quote: string | null = null;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quote) {
      if (c === '\\' && quote === '"') i++;
      else if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === ',') {
      items.push(text.slice(start, i));
      start = i + 1;
    }
  }
  const last = text.slice(start);
  if (last.trim()) items.push(last);
  return items;
}

const isTable = (v: unknown): v is TomlTable =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

/**
 * Reads a theme file the way Pulsar does. With `strict`, anything Pulsar
 * would quietly ignore is an error, which keeps shared themes clean.
 */
export function themeFromToml(
  text: string,
  { strict = false, fallbackName = 'Imported theme' } = {},
): Theme {
  if (new TextEncoder().encode(text).length > MAX_FILE_BYTES)
    throw new Error('The file is larger than 64 KB.');
  const doc = parseToml(text);
  const problems: string[] = [];
  const known = (obj: TomlTable, keys: string[], where: string) => {
    if (!strict) return;
    for (const k of Object.keys(obj))
      if (!keys.includes(k)) problems.push(`Unknown key "${k}" in ${where}.`);
  };
  known(doc, ['name', 'author', 'based_on', 'color_labels', 'dark', 'light'], 'the file');
  if (doc.name !== undefined && typeof doc.name !== 'string') problems.push('name must be text.');
  const credit = (key: string) => {
    const v = doc[key];
    if (v === undefined) return null;
    if (typeof v !== 'string' || v.length > MAX_CREDIT) {
      problems.push(`${key} must be text of up to ${MAX_CREDIT} characters.`);
      return null;
    }
    return v.replace(/\s+/g, ' ').trim() || null;
  };
  const author = credit('author');
  const basedOn = credit('based_on');
  if (doc.color_labels !== undefined && typeof doc.color_labels !== 'boolean')
    problems.push('color_labels must be true or false.');
  if (!isTable(doc.dark)) problems.push('The file needs a [dark] table.');
  const variant = (t: TomlValue | undefined, where: string): Variant | null => {
    if (t === undefined) return null;
    if (!isTable(t)) {
      problems.push(`${where} must be a table.`);
      return null;
    }
    known(t, VARIANT_KEYS, `[${where}]`);
    const out: Variant = { items: {} };
    for (const k of ['text', 'label', 'tile', 'panel'] as const) {
      const c = t[k];
      if (c === undefined) continue;
      if (isHex(c)) out[k] = c.toUpperCase();
      else problems.push(`${where}.${k} must look like "#RRGGBB".`);
    }
    for (const [k, prop] of [
      ['tile_opacity', 'tileOpacity'],
      ['panel_opacity', 'panelOpacity'],
    ] as const) {
      const n = t[k];
      if (n === undefined) continue;
      if (typeof n === 'number' && Number.isInteger(n) && n >= 0 && n <= 255)
        out[prop] = Math.min(n, 100);
      else problems.push(`${where}.${k} must be a whole number from 0 to 100.`);
    }
    if (t.items !== undefined) {
      if (!isTable(t.items)) problems.push(`[${where}.items] must be a table.`);
      else {
        known(t.items, ITEM_KEYS, `[${where}.items]`);
        for (const [k, c] of Object.entries(t.items)) {
          if (!isItemKey(k)) continue;
          if (isHex(c)) out.items[k] = c.toUpperCase();
          else problems.push(`${where}.items.${k} must look like "#RRGGBB".`);
        }
      }
    }
    return out;
  };
  const dark = variant(doc.dark, 'dark') ?? { items: {} };
  const light = variant(doc.light, 'light');
  if (strict && typeof doc.name === 'string' && !doc.name.trim()) problems.push('name is empty.');
  if (problems.length) throw new Error(problems.join('\n'));
  return {
    name: (typeof doc.name === 'string' && doc.name.trim()) || fallbackName,
    author,
    basedOn,
    colorLabels: doc.color_labels === true,
    dark,
    light,
  };
}

export function themeToToml(theme: Theme): string {
  const q = (s: string) => JSON.stringify(s);
  const lines = [`name = ${q(theme.name)}`];
  if (theme.author) lines.push(`author = ${q(theme.author)}`);
  if (theme.basedOn) lines.push(`based_on = ${q(theme.basedOn)}`);
  if (theme.colorLabels) lines.push('color_labels = true');
  const variant = (title: string, v: Variant) => {
    lines.push('', `[${title}]`);
    if (v.text) lines.push(`text = ${q(v.text)}`);
    if (v.label) lines.push(`label = ${q(v.label)}`);
    if (v.tile) lines.push(`tile = ${q(v.tile)}`);
    if (v.tileOpacity !== undefined) lines.push(`tile_opacity = ${v.tileOpacity}`);
    if (v.panel) lines.push(`panel = ${q(v.panel)}`);
    if (v.panelOpacity !== undefined) lines.push(`panel_opacity = ${v.panelOpacity}`);
    const items = ITEM_KEYS.filter((k) => v.items[k]);
    if (items.length) {
      lines.push('', `[${title}.items]`);
      for (const k of items) lines.push(`${k} = ${q(v.items[k] as string)}`);
    }
  };
  variant('dark', theme.dark);
  if (theme.light) variant('light', theme.light);
  return lines.join('\n') + '\n';
}

export function cloneTheme(t: Theme): Theme {
  return {
    name: t.name,
    author: t.author ?? null,
    basedOn: t.basedOn ?? null,
    colorLabels: t.colorLabels,
    dark: { ...t.dark, items: { ...t.dark.items } },
    light: t.light ? { ...t.light, items: { ...t.light.items } } : null,
  };
}
