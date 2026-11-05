import type { Catalog, CatalogEntry } from './community.ts';
import type { OrbitSettings } from './generate.ts';
import { extractPalette, loadImage } from './palette.ts';
import { defaultProfile, mergeProfile, type Profile } from './profile.ts';
import { createSim, setUnits, type Sim } from './sim.ts';
import {
  cloneTheme,
  isItemKey,
  MAX_CREDIT,
  themeFromToml,
  themeToToml,
  windowsTheme,
  type ItemKey,
  type Theme,
} from './theme.ts';

const KEY = 'pulsar-themes';

export type SourceKind = 'builtin' | 'community' | 'custom';
export type Section = 'colors' | 'items' | 'display' | 'popups' | 'widget';
export type DialogName = 'rename' | 'export' | 'import' | 'share';
export type Fit = 'full' | 'shrunk' | 'hide' | 'tray' | 'float';

export interface TaskbarSettings {
  align: 'left' | 'center';
  edge: 'bottom' | 'left' | 'right';
  transparent: boolean;
  closed: string[];
}

export interface Orbit extends OrbitSettings {
  palette: string[];
}

const reducedMotion =
  typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;

export const app = $state({
  light: false,
  live: !reducedMotion,
  wallpaper: null as { url: string; name: string } | null,
  catalog: { builtin: [], community: [] } as Catalog,
  source: { kind: 'builtin', id: 'aurora' } as { kind: SourceKind; id: string | null },
  tab: 'builtin' as 'builtin' | 'community',
  theme: windowsTheme(),
  selected: 'cpu' as ItemKey,
  section: 'colors' as Section,
  profile: defaultProfile(),
  taskbar: { align: 'left', edge: 'bottom', transparent: false, closed: [] } as TaskbarSettings,
  panelOpen: true,
  notes: true,
  themeMenu: false,
  orbit: {
    seed: '#5EA8F2',
    harmony: 'triadic',
    spread: 50,
    vivid: 78,
    bright: 68,
    palette: [],
  } as Orbit,
  toast: null as string | null,
  dialog: null as DialogName | null,
  fit: 'full' as Fit,
  tick: 0,
});

let restored = false;
try {
  const saved = JSON.parse(localStorage.getItem(KEY) ?? 'null');
  if (saved?.theme?.dark) {
    const kinds = ['builtin', 'community', 'custom'];
    Object.assign(app, {
      theme: themeFromToml(themeToToml(saved.theme)),
      source: kinds.includes(saved.source?.kind) ? saved.source : app.source,
      selected: isItemKey(saved.selected) ? saved.selected : app.selected,
      light: !!saved.light,
      profile: mergeProfile(saved.profile),
      taskbar: { ...app.taskbar, ...saved.taskbar },
      panelOpen: saved.panelOpen !== false,
      notes: saved.notes !== false,
      orbit: { ...app.orbit, ...saved.orbit, palette: [] },
    });
    restored = true;
  }
} catch {}

export const wasRestored = () => restored;

export function save() {
  const data = JSON.stringify({
    theme: app.theme,
    source: app.source,
    selected: app.selected,
    light: app.light,
    profile: app.profile,
    taskbar: app.taskbar,
    panelOpen: app.panelOpen,
    notes: app.notes,
    orbit: { ...app.orbit, palette: [] },
  });
  try {
    localStorage.setItem(KEY, data);
  } catch {}
}

const sim = createSim();

/** The simulated PC, read so that whatever uses it follows each new reading and the unit settings. */
export function readings(): Sim {
  void app.tick;
  setUnits({ ...app.profile.units });
  return sim;
}

export function step() {
  sim.step();
  app.tick++;
}

export function editProfile(change: (p: Profile) => void) {
  change(app.profile);
}

export function editTheme(change: (t: Theme) => void) {
  const theme = cloneTheme($state.snapshot(app.theme));
  change(theme);
  if (app.source.kind !== 'custom') {
    const credit = theme.author ? `${theme.name} by ${theme.author}` : theme.name;
    theme.basedOn ??= (credit.length > MAX_CREDIT ? theme.name : credit).slice(0, MAX_CREDIT);
    theme.author = null;
    theme.name = `${theme.name} (modified)`;
  }
  app.theme = theme;
  app.source = { kind: 'custom', id: null };
}

export function themeOf(entry: CatalogEntry): Theme {
  if (entry.id === 'windows' && !entry.toml) return windowsTheme();
  const theme = themeFromToml(entry.toml);
  return { ...theme, name: entry.name, author: entry.author ?? theme.author };
}

let toastTimer: ReturnType<typeof setTimeout> | undefined;
export function toast(text: string) {
  clearTimeout(toastTimer);
  app.toast = text;
  toastTimer = setTimeout(() => (app.toast = null), 2600);
}

export async function useWallpaper(file: File) {
  try {
    const { url, img } = await loadImage(file);
    const palette = extractPalette(img);
    if (app.wallpaper?.url) URL.revokeObjectURL(app.wallpaper.url);
    app.wallpaper = { url, name: file.name };
    app.orbit = { ...app.orbit, palette, seed: palette[0] ?? app.orbit.seed };
    toast('Wallpaper set; its colors are waiting in Orbit');
  } catch (err) {
    toast((err as Error).message);
  }
}

export function resetWallpaper() {
  if (app.wallpaper?.url) URL.revokeObjectURL(app.wallpaper.url);
  app.wallpaper = null;
  app.orbit = { ...app.orbit, palette: [] };
}

export function copyTheme() {
  navigator.clipboard.writeText(themeToToml($state.snapshot(app.theme))).then(
    () => toast('Theme copied'),
    () => toast('Copy is blocked here; use Save instead'),
  );
}
