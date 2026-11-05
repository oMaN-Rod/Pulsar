import { rgba } from './color.ts';
import type { ItemSettings, LoadLevel, LoadUnit, Profile } from './profile.ts';
import { graph, type Sim } from './sim.ts';
import { META, type ItemKey, type Resolved } from './theme.ts';

export const TILE_H = 40;

/** The longest value Pulsar reserves room for, per item. */
const WIDEST: Record<ItemKey, string> = {
  cpu: '100%',
  cpu_temp: '199°C',
  cpu_power: '999 W',
  cpu_clock: '9.99 GHz',
  ram: '100%',
  chipset_temp: '199°C',
  gpu: '100%',
  gpu_temp: '199°C',
  gpu_memory: '100%',
  gpu_power: '999 W',
  disk: '100%',
  disk_temp: '199°C',
  network: '99.9 MB/s',
  ping: '999 ms',
  battery: '100%',
  fps: '9999 fps',
};
const WIDEST_BYTES: Partial<Record<ItemKey, string>> = { ram: '99.9 GB', gpu_memory: '99.9 GB' };

const UNDER = ['spark', 'segments', 'heat_strip', 'bullet', 'range_rail', 'core_types', 'uptime'];

const LOAD = {
  dark: ['#6CCB5F', '#FFD33D', '#FF8C1A', '#FF4D4F'],
  light: ['#107C10', '#A88000', '#C85A00', '#C42B1C'],
};
const LOAD_KEYS: LoadLevel[] = ['low', 'moderate', 'high', 'critical'];
const ACCENT = { dark: '#4CC2FF', light: '#0067C0' };

type Load = Profile['load'];

export interface Run {
  text?: string;
  icon?: ItemKey;
  color?: string;
}

export interface Shape {
  tag: 'rect' | 'circle' | 'path' | 'polyline';
  attrs: Record<string, string | number>;
}

export interface TextSpan {
  style: string;
  small?: boolean;
  runs: Run[];
}

export interface Tile {
  w: number;
  h: number;
  bg: string | null;
  shapes: Shape[];
  spans: TextSpan[];
}

function band(unit: LoadUnit, raw: number, load: Load) {
  const [m, h, c] = load.thresholds[unit];
  return raw >= c ? 3 : raw >= h ? 2 : raw >= m ? 1 : 0;
}

export function loadColorBand(i: number, light: boolean, load: Load | null | undefined): string {
  return load?.colors[LOAD_KEYS[i]] ?? (light ? LOAD.light : LOAD.dark)[i];
}

export function loadColor(v01: number, light: boolean, load: Load): string {
  return loadColorBand(v01 >= 0.9 ? 3 : v01 >= 0.75 ? 2 : v01 >= 0.5 ? 1 : 0, light, load);
}

/** The load color for item `k`'s reading `raw`; `null` for readings without a scale. */
export function loadOf(k: ItemKey, raw: number, p: Profile, light: boolean): string | null {
  const unit = META[k].unit;
  return unit ? loadColorBand(band(unit, raw, p.load), light, p.load) : null;
}

export function graphColor(
  k: ItemKey,
  v: Resolved,
  sim: Sim,
  p: Profile,
  item: ItemSettings,
  light: boolean,
): string {
  if (item.colorMode === 'load') return loadOf(k, sim.raw(k), p, light) ?? v.items[k];
  if (item.colorMode === 'accent' || p.accentGraphs) return light ? ACCENT.light : ACCENT.dark;
  return v.items[k];
}

export function fontCss(font: Profile['font']): string {
  const family = font.family ? `'${font.family.replace(/'/g, '')}', ` : '';
  return `${family}'Segoe UI Variable Text', 'Segoe UI', system-ui, sans-serif`;
}

/** Pulsar's font size in pixels at 96 DPI. */
export const fontPx = (font: Profile['font']) => (font.size * 96) / 72;

let measurer: CanvasRenderingContext2D | null | undefined;
function textWidth(s: string, px: number, family: string, weight: number) {
  if (measurer === undefined)
    measurer =
      typeof document === 'undefined' ? null : document.createElement('canvas').getContext('2d');
  if (!measurer) return s.length * px * 0.56;
  measurer.font = `${weight} ${px}px ${family}`;
  return measurer.measureText(s).width;
}

const PAD = 4;
const MIN_TILE = 44;
const GRAPH_SCALE = 0.85;
const coresWidth = (n: number) => Math.min(120, Math.max(36, n * 3 - 1));
const heatRows = (h: number) => Math.min(4, Math.max(1, Math.floor((h - 8) / 10)));
const heatWidth = (n: number, h: number) =>
  Math.min(120, Math.max(24, Math.ceil(n / heatRows(h)) * 10 - 1));

function trendOf(hist: number[]) {
  if (hist.length < 6) return null;
  const earlier = hist.slice(-31, -1);
  const change = (hist.at(-1) as number) - earlier.reduce((a, b) => a + b, 0) / earlier.length;
  return change > 0.05 ? 'up' : change < -0.05 ? 'down' : 'flat';
}

export function networkRows(sim: Sim, p: Profile, c: string, up: string): Run[][] {
  const t = sim.net();
  const rows: Run[][] = [];
  if (p.network.show !== 'upload') rows.push([{ text: '↓', color: c }, { text: ` ${t.down}` }]);
  if (p.network.show !== 'download') rows.push([{ text: '↑', color: up }, { text: ` ${t.up}` }]);
  return p.network.uploadOnTop ? rows.reverse() : rows;
}

export function labelRuns(k: ItemKey, p: Profile, color: string): Run[] {
  const icon = p.icons || p.labels === 'none';
  if (p.labels === 'none') return [{ icon: k, color }];
  const text = p.labels === 'short' ? META[k].letter : META[k].short;
  return icon
    ? [
        { icon: k, color },
        { text: ` ${text}`, color },
      ]
    : [{ text, color }];
}

const rect = (attrs: Record<string, string | number>): Shape => ({ tag: 'rect', attrs });
const circle = (attrs: Record<string, string | number>): Shape => ({ tag: 'circle', attrs });
const polyline = (attrs: Record<string, string | number>): Shape => ({ tag: 'polyline', attrs });
const path = (attrs: Record<string, string | number>): Shape => ({ tag: 'path', attrs });

export interface TileContext {
  v: Resolved;
  sim: Sim;
  p: Profile;
  item: ItemSettings;
  light: boolean;
  minWidth?: number;
  fixedWidth?: number;
}

export function renderTile(k: ItemKey, ctx: TileContext): Tile {
  const { v, sim, p, item, light } = ctx;
  const style = item.graph ? item.style : 'none';
  const family = fontCss(p.font);
  const weight = p.font.bold ? 700 : 400;
  const c = graphColor(k, v, sim, p, item, light);
  const up = item.colorMode === 'item' && !p.accentGraphs ? v.up : c;
  const L = v.colorLabels ? v.items[k] : v.label;
  const T = v.text;
  const net = k === 'network';
  const text = net ? '' : sim.text(k);
  const lvl = sim.level(k);
  const hist = sim.history(k);
  const narrow = !!ctx.fixedWidth;
  const H =
    narrow && (k === 'network' || UNDER.includes(style))
      ? 54
      : Math.max(16, Math.min(44, item.tileHeight ?? TILE_H));
  const label = labelRuns(k, p, L);
  const inline = !narrow && (p.inline || H < 30);
  const font = fontPx(p.font);
  const fit = (H - PAD) / 2.6;
  let px = inline ? font * GRAPH_SCALE : Math.min(font * GRAPH_SCALE, fit);
  if (inline && px * 1.33 > H) px = H / 1.33;
  const spans: TextSpan[] = [];
  const shapes: Shape[] = [];
  const span = (x: number, y: number, runs: Run[], color: string, anchor = 'start'): TextSpan => ({
    style: `${anchor === 'end' ? `right:${x}px` : `left:${x}px`};top:${y.toFixed(1)}px;color:${color};font-weight:${weight};font-size:${px.toFixed(1)}px`,
    runs,
  });
  const plain = (s: string): Run[] => [{ text: s }];

  const widestText = (
    p.units.memory === 'bytes' && WIDEST_BYTES[k] ? (WIDEST_BYTES[k] as string) : WIDEST[k]
  ).replace('199°C', p.units.temperature === 'fahrenheit' ? '390°F' : '199°C');
  const labelText = p.labels === 'short' ? META[k].letter : META[k].short;
  const vertical = p.verticalLabels && !net;
  const sizes = (size: number) => {
    const tw = (s: string) => textWidth(s, size, family, weight);
    const gap = Math.round(size * 0.3);
    const icon = p.icons || p.labels === 'none' ? size : 0;
    const lw = p.labels === 'none' ? icon : tw(labelText) + (icon ? icon + gap : 0);
    const value = tw(widestText);
    return {
      value,
      lw,
      text: net
        ? tw('↓') + gap + value
        : vertical
          ? value
          : inline
            ? lw + PAD + value
            : Math.max(lw, value),
      lead: vertical ? tw('W') + 3 : 0,
    };
  };
  const sized = sizes(inline ? px : Math.min(font, fit / GRAPH_SCALE));
  let w: number;
  if (narrow) {
    const room = (ctx.fixedWidth as number) - 6 - sized.lead;
    const need = Math.max(sized.lw, sized.value);
    if (need > room) px = Math.max(6.5, (px * room) / need);
    w = ctx.fixedWidth as number;
  } else {
    let chrome = sized.lead;
    if (style === 'ring') chrome += H - 6 + 4;
    else if (style === 'cores') chrome += coresWidth(sim.cores().length + 1) + 5;
    else if (style === 'heat') chrome += heatWidth(sim.cores().length, H) + 5;
    else if (style === 'bubble' || style === 'combined') chrome += 14;
    else if (style === 'battery_cell') chrome += 28;
    else if (UNDER.includes(style)) {
      const below = !inline && !net && H - 5 - px * 1.33 >= 5;
      chrome += below ? (vertical ? 0 : Math.min(sized.lw, sized.value) + 4) : 45;
    }
    if (item.trend) chrome += 11;
    w = Math.round(
      Math.max(sized.text + chrome + 2 * PAD, item.tileWidth ?? MIN_TILE, ctx.minWidth ?? 0),
    );
  }
  const ringD = H - 6;

  let bg: string | null = null;
  const top = H >= 30 ? 3 : (H - px) / 2 - 1;
  const bottom = H - px - 4;

  const stacked = (x0 = 4) => {
    if (net) {
      const rows = networkRows(sim, p, c, up);
      if (rows.length === 1 || H < 30) spans.push(span(x0, (H - px) / 2 - 2, rows[0], T));
      else spans.push(span(x0, top, rows[0], T), span(x0, bottom, rows[1], T));
    } else if (vertical) {
      const letters = [...(p.labels === 'short' ? META[k].letter : META[k].short)].slice(0, 4);
      const lh = Math.min(px * 0.85, (H - 4) / letters.length);
      for (const [i, ch] of letters.entries()) {
        spans.push({
          style: `left:${x0}px;top:${(2 + i * lh).toFixed(1)}px;color:${L};font-size:${(lh * 0.95).toFixed(1)}px;line-height:1`,
          runs: plain(ch),
        });
      }
      spans.push(span(x0 + sized.lead, bottom, plain(text), T));
    } else if (inline) {
      spans.push(span(x0, (H - px) / 2 - 2, [...label, { text: `  ${text}` }], T));
    } else {
      spans.push(span(x0, top, label, L), span(x0, bottom, plain(text), T));
    }
  };

  const bars = (values: number[], color: string, x0 = 0, x1 = w, y1 = H, height = H * 0.62) => {
    const n = Math.floor((x1 - x0) / 4);
    const list = values.slice(-n);
    list.forEach((val, i) => {
      const bh = Math.max(1, val * height);
      shapes.push(
        rect({
          x: (x1 - (list.length - i) * 4).toFixed(1),
          y: (y1 - bh).toFixed(1),
          width: 3,
          height: bh.toFixed(1),
          fill: color,
        }),
      );
    });
  };

  const loadAt = (i: number) => {
    const raw = sim.rawHistory(k);
    const sample = raw[raw.length - hist.length + i];
    return loadOf(k, sample, p, light) ?? loadColor(hist[i], light, p.load);
  };

  const under = (
    kind: string,
    x0: number,
    x1: number,
    y: number,
    series: number[],
    now: number,
    color: string,
  ) => {
    const span = x1 - x0;
    switch (kind) {
      case 'spark': {
        const g = graph(series, span, 10, 0);
        shapes.push(
          polyline({
            transform: `translate(${x0},${y - 2})`,
            points: g.line,
            fill: 'none',
            stroke: color,
            'stroke-width': 1.3,
          }),
        );
        break;
      }
      case 'segments': {
        const n = 10;
        const sw = (span - (n - 1) * 2) / n;
        const lit = Math.round(now * n);
        for (let i = 0; i < n; i++) {
          shapes.push(
            rect({
              x: (x0 + i * (sw + 2)).toFixed(1),
              y,
              width: sw.toFixed(1),
              height: 5,
              rx: 1,
              fill: i < lit ? color : rgba(color, 0.2),
            }),
          );
        }
        break;
      }
      case 'heat_strip': {
        const n = Math.floor(span / 4);
        const start = series.length - Math.min(n, series.length);
        series.slice(-n).forEach((val, i) => {
          shapes.push(
            rect({
              x: x0 + i * 4,
              y: y - 1,
              width: 3,
              height: 7,
              fill: net ? loadColor(val, light, p.load) : loadAt(start + i),
            }),
          );
        });
        break;
      }
      case 'bullet': {
        const l = net ? { now, high: Math.max(...series) } : lvl;
        const unit = META[k].unit;
        const marks = p.load.thresholds[unit ?? 'percent']
          .map((t) => (unit === 'percent' || !unit ? t / 100 : null))
          .filter((t) => t !== null);
        shapes.push(rect({ x: x0, y, width: span, height: 5, rx: 1, fill: rgba(color, 0.2) }));
        for (const t of marks)
          shapes.push(
            rect({ x: (x0 + span * t).toFixed(1), y, width: 1, height: 5, fill: rgba(T, 0.35) }),
          );
        shapes.push(
          rect({ x: x0, y, width: (span * l.now).toFixed(1), height: 5, rx: 1, fill: color }),
          rect({ x: (x0 + span * l.high - 1).toFixed(1), y: y - 2, width: 2, height: 9, fill: T }),
        );
        break;
      }
      case 'range_rail': {
        const l = net ? { now, low: Math.min(...series), high: Math.max(...series) } : lvl;
        shapes.push(
          rect({ x: x0, y: y + 2, width: span, height: 1, fill: rgba(T, 0.3) }),
          rect({
            x: (x0 + span * l.low).toFixed(1),
            y: y + 1,
            width: Math.max(2, span * (l.high - l.low)).toFixed(1),
            height: 3,
            rx: 1,
            fill: rgba(color, 0.55),
          }),
          rect({
            x: (x0 + span * l.now - 1).toFixed(1),
            y: y - 2,
            width: 2,
            height: 9,
            fill: color,
          }),
        );
        break;
      }
    }
  };

  const dot = loadOf(k, sim.raw(k), p, light) ?? c;
  switch (style) {
    case 'none':
      stacked();
      break;
    case 'area':
    case 'line':
    case 'bars':
    case 'combined': {
      if (style === 'bars' || style === 'combined') bars(hist, rgba(c, 0.6));
      else {
        const g = graph(hist, w, H);
        if (style === 'area') shapes.push(path({ d: g.area, fill: rgba(c, 0.38) }));
        shapes.push(
          polyline({
            points: g.line,
            fill: 'none',
            stroke: c,
            'stroke-width': style === 'line' ? 1.6 : 1.2,
          }),
        );
      }
      if (net && p.network.show === 'both')
        shapes.push(
          polyline({
            points: graph(sim.upHistory(), w, H).line,
            fill: 'none',
            stroke: up,
            'stroke-width': 1.2,
          }),
        );
      if (style === 'combined') {
        const tk = k === 'cpu' ? 'cpu_temp' : 'gpu_temp';
        shapes.push(
          circle({ cx: w - 9, cy: 9, r: 5.5, fill: loadOf(tk, sim.raw(tk), p, light) as string }),
        );
      }
      stacked();
      break;
    }
    case 'meter':
      shapes.push(
        rect({ x: 0, y: 0, width: (w * lvl.now).toFixed(1), height: H, fill: rgba(c, 0.45) }),
      );
      stacked();
      break;
    case 'dot_matrix': {
      const rows = Math.max(2, Math.floor((H * 0.6) / 4));
      const n = Math.floor(w / 4);
      const list = hist.slice(-n);
      list.forEach((val, i) => {
        const lit = Math.round(val * rows);
        const x = w - (list.length - i) * 4;
        for (let r = 0; r < lit; r++)
          shapes.push(rect({ x, y: H - 4 - r * 4, width: 3, height: 3, fill: rgba(c, 0.85) }));
      });
      stacked();
      break;
    }
    case 'tint':
      bg = rgba(dot, 0.42);
      stacked();
      break;
    case 'mirror_bars': {
      const half = H / 2;
      const n = Math.floor(w / 4);
      const mirror = (values: number[], color: string, dir: number) => {
        const list = values.slice(-n);
        list.forEach((val, i) => {
          const bh = Math.max(1, val * (half - 2));
          const y = dir < 0 ? half - bh : half;
          shapes.push(
            rect({
              x: w - (list.length - i) * 4,
              y: y.toFixed(1),
              width: 3,
              height: bh.toFixed(1),
              fill: color,
            }),
          );
        });
      };
      mirror(hist, rgba(c, 0.6), -1);
      mirror(sim.upHistory(), rgba(up, 0.6), 1);
      stacked();
      break;
    }
    case 'ring': {
      const d = narrow ? Math.min(ringD, w - 6) : ringD;
      const cx = narrow ? w - 3 - d / 2 : PAD + d / 2;
      const r = d / 2 - 2;
      const circ = 2 * Math.PI * r;
      shapes.push(
        circle({ cx, cy: H / 2, r, fill: 'none', stroke: rgba(c, 0.22), 'stroke-width': 3.5 }),
        circle({
          cx,
          cy: H / 2,
          r,
          fill: 'none',
          stroke: c,
          'stroke-width': 3.5,
          'stroke-linecap': 'round',
          'stroke-dasharray': `${(circ * lvl.now).toFixed(1)} ${circ.toFixed(1)}`,
          transform: `rotate(-90 ${cx} ${H / 2})`,
        }),
      );
      stacked(narrow ? PAD : PAD + d + 4);
      break;
    }
    case 'bubble':
      shapes.push(circle({ cx: w - PAD - 5, cy: H / 2, r: 5, fill: dot }));
      stacked();
      break;
    case 'battery_cell': {
      const x = w - PAD - 24;
      const y = (H - 12) / 2;
      shapes.push(
        rect({
          x,
          y,
          width: 21,
          height: 12,
          rx: 2,
          fill: 'none',
          stroke: rgba(T, 0.7),
          'stroke-width': 1.2,
        }),
        rect({ x: x + 21.5, y: y + 3.5, width: 2.5, height: 5, rx: 1, fill: rgba(T, 0.7) }),
        rect({ x: x + 2, y: y + 2, width: (17 * lvl.now).toFixed(1), height: 8, rx: 1, fill: c }),
      );
      stacked();
      break;
    }
    case 'cores': {
      const cores = sim.cores();
      const cw = narrow ? w - 2 * PAD : coresWidth(cores.length + 1);
      const pitch = (cw + 1) / (cores.length + 1);
      const x0 = w - PAD - cw;
      const bar = (i: number) => x0 + (i + (i >= sim.pThreads ? 1 : 0)) * pitch;
      cores.forEach((val, i) => {
        shapes.push(
          rect({
            x: bar(i).toFixed(1),
            y: PAD,
            width: (pitch - 1).toFixed(1),
            height: H - 2 * PAD,
            fill: rgba(c, 0.15),
          }),
          rect({
            x: bar(i).toFixed(1),
            y: (H - PAD - val * (H - 2 * PAD)).toFixed(1),
            width: (pitch - 1).toFixed(1),
            height: (val * (H - 2 * PAD)).toFixed(1),
            fill: c,
          }),
        );
      });
      stacked();
      break;
    }
    case 'heat': {
      const cores = sim.cores();
      const rows = heatRows(H);
      const cols = Math.ceil(cores.length / rows);
      const x0 = narrow ? PAD : w - PAD - heatWidth(cores.length, H);
      const y0 = (H - rows * 10 + 1) / 2;
      cores.forEach((val, i) => {
        shapes.push(
          rect({
            x: x0 + (i % cols) * 10,
            y: (y0 + Math.floor(i / cols) * 10).toFixed(1),
            width: 9,
            height: 9,
            rx: 1.5,
            fill: loadColorBand(band('percent', val * 100, p.load), light, p.load),
            'fill-opacity': 0.45 + val * 0.55,
          }),
        );
      });
      stacked();
      break;
    }
    default: {
      const y = H - 13;
      if (net) {
        const rows = networkRows(sim, p, c, up);
        const series = { down: [sim.history(k), c], up: [sim.upHistory(), up] } as const;
        let order: ('down' | 'up')[] =
          p.network.show === 'both'
            ? ['down', 'up']
            : [p.network.show === 'upload' ? 'up' : 'down'];
        if (p.network.uploadOnTop) order = order.reverse();
        const x0 = narrow ? PAD : Math.round(PAD + sized.text + 5);
        const rowStyle = style === 'core_types' || style === 'uptime' ? 'spark' : style;
        order.forEach((dir, i) => {
          const ty = order.length === 1 ? (H - px) / 2 - 2 : i ? bottom : top;
          spans.push(span(4, ty, rows[i], T));
          const [s, col] = series[dir];
          under(
            rowStyle,
            x0,
            w - 4,
            narrow ? ty + px + 3 : ty + px / 2,
            s,
            s.at(-1) as number,
            col,
          );
        });
        break;
      }
      if (narrow) spans.push(span(4, top, label, L), span(4, top + px + 3, plain(text), T));
      else
        spans.push(span(4, top, label, L), span(item.trend ? 14 : 4, top, plain(text), T, 'end'));
      if (style === 'core_types') {
        const cores = sim.cores();
        const avg = (a: number[]) => a.reduce((s, x) => s + x, 0) / a.length;
        const rows: [string, number][] = [
          ['P', avg(cores.slice(0, sim.pThreads))],
          ['E', avg(cores.slice(sim.pThreads))],
        ];
        rows.forEach(([n, val], i) => {
          const ry = H - 19 + i * 9;
          spans.push({ style: `left:4px;top:${ry - 3}px;color:${L}`, small: true, runs: plain(n) });
          shapes.push(
            rect({ x: 14, y: ry, width: w - 18, height: 4, rx: 1, fill: rgba(c, 0.18) }),
            rect({ x: 14, y: ry, width: ((w - 18) * val).toFixed(1), height: 4, rx: 1, fill: c }),
          );
        });
      } else if (style === 'uptime') {
        const n = Math.floor((w - 8) / 4);
        const start = hist.length - Math.min(n, hist.length);
        hist.slice(-n).forEach((val, i) => {
          const tall = val > 0.95;
          shapes.push(
            rect({
              x: 4 + i * 4,
              y: tall ? y - 4 : y,
              width: 3,
              height: tall ? 11 : 7,
              fill: loadAt(start + i),
            }),
          );
        });
      } else {
        under(style, 4, w - 4, y + 2, hist, lvl.now, c);
      }
    }
  }

  if (item.trend && !net) {
    const t = trendOf(hist);
    const x = w - 8;
    if (t === 'up') shapes.push(path({ d: `M${x - 4} 10h8l-4-6z`, fill: L }));
    else if (t === 'down') shapes.push(path({ d: `M${x - 4} 4h8l-4 6z`, fill: L }));
    else if (t === 'flat') shapes.push(rect({ x: x - 4, y: 6, width: 8, height: 2, fill: L }));
  }

  return { w, h: H, bg, shapes, spans };
}

export interface TextCell {
  key: ItemKey;
  name: string;
  label: Run[];
  labelColor: string;
  valueColor: string;
  value: string;
}

export type TextColumn = { kind: 'cells'; cells: TextCell[] } | { kind: 'network'; rows: Run[][] };

/**
 * Text mode: no tiles, items in columns of two with the label before the
 * value, network in a column of its own.
 */
export function textColumns(keys: ItemKey[], ctx: Omit<TileContext, 'item'>): TextColumn[] {
  const { v, sim, p, light } = ctx;
  const cell = (k: ItemKey): TextCell => {
    const item = p.items[k];
    const L = v.colorLabels ? v.items[k] : v.label;
    return {
      key: k,
      name: META[k].name,
      label: labelRuns(k, p, L),
      labelColor: L,
      valueColor: item.colorMode === 'load' ? (loadOf(k, sim.raw(k), p, light) ?? v.text) : v.text,
      value: sim.text(k),
    };
  };
  const columns: TextColumn[] = [];
  let pending: ItemKey[] = [];
  const flush = () => {
    if (pending.length) columns.push({ kind: 'cells', cells: pending.map(cell) });
    pending = [];
  };
  for (const k of keys) {
    if (k === 'network') {
      flush();
      const c = graphColor(k, v, sim, p, p.items[k], light);
      columns.push({ kind: 'network', rows: networkRows(sim, p, c, v.up) });
      continue;
    }
    pending.push(k);
    if (pending.length === 2) flush();
  }
  flush();
  return columns;
}
