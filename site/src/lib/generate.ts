import { fromHsl, toHsl } from './color.ts';
import { ITEM_KEYS, type ItemColors, type Theme } from './theme.ts';

export type HarmonyId = 'analogous' | 'complementary' | 'triadic' | 'tonal';

export interface Harmony {
  id: HarmonyId;
  name: string;
  title: string;
  anchors: (h: number, sp: number) => number[];
  hue: (h: number, i: number, sp: number) => number;
}

export const HARMONIES: Harmony[] = [
  {
    id: 'analogous',
    name: 'Analogous',
    title: 'Drift',
    anchors: (h, sp) => [h - sp, h + sp],
    hue: (h, i, sp) => h + (i / 15 - 0.5) * 2 * sp,
  },
  {
    id: 'complementary',
    name: 'Complementary',
    title: 'Counterpoint',
    anchors: (h) => [h, h + 180],
    hue: (h, i, sp) => (i % 2 ? h + 180 : h) + (((i * 11) % 7) - 3) * (sp / 9),
  },
  {
    id: 'triadic',
    name: 'Triadic',
    title: 'Prism',
    anchors: (h) => [h, h + 120, h + 240],
    hue: (h, i, sp) => h + 120 * (i % 3) + (((i * 7) % 5) - 2) * (sp / 8),
  },
  {
    id: 'tonal',
    name: 'Tonal',
    title: 'Wash',
    anchors: (h) => [h],
    hue: (h, i, sp) => h + (((i * 5) % 3) - 1) * (sp / 10),
  },
];

export function harmonyOf(id: string): Harmony {
  return HARMONIES.find((x) => x.id === id) ?? HARMONIES[2];
}

export interface OrbitSettings {
  seed: string;
  harmony: string;
  spread: number;
  vivid: number;
  bright: number;
}

export function generateTheme({ seed, harmony, spread, vivid, bright }: OrbitSettings): Theme {
  const H = harmonyOf(harmony);
  const h = toHsl(seed)[0];
  const sp = 10 + spread * 0.7;
  const items: ItemColors = {};
  ITEM_KEYS.forEach((k, i) => {
    const wobble = H.id === 'tonal' ? ((i * 13) % 30) - 12 : ((i * 7) % 9) - 4;
    items[k] = fromHsl(H.hue(h, i, sp), vivid, Math.max(40, Math.min(88, bright + wobble)));
  });
  return {
    name: H.title,
    author: null,
    basedOn: null,
    colorLabels: false,
    dark: {
      text: fromHsl(h, 30, 97),
      label: fromHsl(h, 22, 72),
      tile: fromHsl(h, 45, 13),
      tileOpacity: 40,
      panel: fromHsl(h, 35, 8),
      panelOpacity: 0,
      items,
    },
    light: null,
  };
}
