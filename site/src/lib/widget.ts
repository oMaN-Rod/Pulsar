import { rgba } from './color.ts';
import type { Profile, WidgetLayout, WidgetMaterial, WidgetStyle } from './profile.ts';

type Widget = Profile['widget'];

export type Arrangement =
  | { kind: 'styles' }
  | { kind: 'forced'; style: WidgetStyle }
  | { kind: 'dashboard'; columns: number }
  | { kind: 'strip' }
  | { kind: 'rings' }
  | { kind: 'focus' };

const STRIP_HEIGHTS: [number, number] = [32, 52];
const RINGS_HEIGHT = 60;
const FOCUS_MIN: [number, number] = [180, 140];
const STRIP_CELL = 92;
const DASHBOARD_COLUMN = 300;
const PANEL_GAP = 8;
export const FOCUS_ASPECT = 0.8;

/** The heights a chosen layout may be dragged between, or `null` for any. */
export function heightRange(layout: WidgetLayout): readonly [number, number] | null {
  if (layout === 'strip') return STRIP_HEIGHTS;
  if (layout === 'rings') return [RINGS_HEIGHT, RINGS_HEIGHT];
  if (layout === 'focus') return [FOCUS_MIN[1], 1600];
  return null;
}

export const minWidth = (layout: WidgetLayout) => (layout === 'focus' ? FOCUS_MIN[0] : 100);

const dashboardColumns = (w: number, items: number) =>
  Math.max(
    1,
    Math.min(Math.max(1, items), Math.floor((w + PANEL_GAP) / (DASHBOARD_COLUMN + PANEL_GAP))),
  );

export function automatic(w: number, h: number, items: number): Arrangement {
  const n = Math.max(1, items);
  if (h < 72) return { kind: w >= n * STRIP_CELL + 24 ? 'strip' : 'rings' };
  if (n === 1 && w >= FOCUS_MIN[0] && h >= FOCUS_MIN[1]) return { kind: 'focus' };
  if (w >= 600 && h >= 200) return { kind: 'dashboard', columns: dashboardColumns(w, n) };
  return { kind: 'forced', style: w >= 0.8 * h ? 'tiles' : 'sparklines' };
}

export function arrange(w: Widget, items: number): Arrangement {
  const chosen = w.sizing === 'chosen';
  switch (w.layout) {
    case 'auto':
      return chosen ? automatic(w.size[0], w.size[1], items) : { kind: 'styles' };
    case 'dashboard':
      return {
        kind: 'dashboard',
        columns: chosen
          ? dashboardColumns(w.size[0], items)
          : Math.max(1, Math.min(3, Math.ceil(items / 2))),
      };
    case 'lanes':
      return { kind: 'forced', style: 'sparklines' };
    case 'tiles':
    case 'meters':
    case 'dials':
      return { kind: 'forced', style: w.layout };
    default:
      return { kind: w.layout };
  }
}

/** The widget's size in CSS pixels, or `null` where the content sets it. */
export function boxSize(
  w: Widget,
  a: Arrangement,
): { width: number | null; height: number | null } {
  if (w.sizing === 'chosen') return { width: w.size[0], height: w.size[1] };
  if (a.kind === 'focus') return { width: w.width, height: Math.round(w.width * FOCUS_ASPECT) };
  if (a.kind === 'strip') return { width: null, height: STRIP_HEIGHTS[1] };
  if (a.kind === 'rings') return { width: null, height: RINGS_HEIGHT };
  if (a.kind === 'dashboard') return { width: a.columns * w.width, height: null };
  return { width: w.width, height: null };
}

export interface Skin {
  fill: string;
  border: string;
  radius: number;
  blur: boolean;
  liquid: boolean;
}

/** `opacity` and `glassTint` are 0–100. */
export function skin(
  look: { material: WidgetMaterial; opacity: number; glassTint: number },
  base: string,
  light: boolean,
): Skin {
  const edge = (dark: number, bright: number) =>
    light ? `rgba(0, 0, 0, ${bright})` : `rgba(255, 255, 255, ${dark})`;
  const opacity = look.opacity / 100;
  const tint = look.glassTint / 100;
  const floor = 1 / 255;
  const plain = { blur: false, liquid: false };
  switch (look.material) {
    case 'glass':
      return {
        ...plain,
        fill: rgba(base, Math.max(floor, 0.85 * tint)),
        border: edge(0.22, 0.1),
        radius: 16,
        blur: true,
      };
    case 'liquid':
      return {
        fill: rgba(base, Math.max(floor, 0.7 * tint)),
        border: edge(0.3, 0.12),
        radius: 20,
        blur: true,
        liquid: true,
      };
    case 'acrylic':
      return {
        ...plain,
        fill: rgba(base, Math.max(floor, 0.8 * opacity)),
        border: edge(0.09, 0.06),
        radius: 8,
        blur: true,
      };
    case 'clear':
      return {
        ...plain,
        fill: rgba(base, Math.max(floor, 0.28 * opacity)),
        border: edge(0.1, 0.08),
        radius: 12,
      };
    default:
      return {
        ...plain,
        fill: rgba(base, Math.max(floor, opacity)),
        border: edge(0.12, 0.1),
        radius: 8,
      };
  }
}

export const rowRadius = (radius: number | null, h: number) =>
  radius === null ? h / 2 : Math.min(radius, h / 2);

/** How long the pointer must press against the edge before a docked widget opens. */
export const PUSH_MS = { light: 0, medium: 160, firm: 380 } as const;
