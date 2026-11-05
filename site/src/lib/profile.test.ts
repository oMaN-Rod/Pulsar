import { describe, expect, it } from 'vitest';
import {
  defaultProfile,
  mergeProfile,
  moveKey,
  profileFromToml,
  profileToToml,
  taskbarKeys,
  widgetKeys,
  type Profile,
} from './profile.ts';
import { ITEM_KEYS, parseToml } from './theme.ts';

function custom(): Profile {
  const p = defaultProfile();
  p.name = 'Desk';
  p.mode = 'text';
  p.labels = 'short';
  p.position = 'left';
  p.font = { family: 'Cascadia Code', size: 10.5, bold: true };
  p.units = { temperature: 'fahrenheit', network: 'bits', memory: 'bytes' };
  p.network = { show: 'download', uploadOnTop: true };
  p.fit = { enabled: false, removeFrom: 'left', noRoom: 'tray', otherSide: false };
  p.load.thresholds.celsius = [60, 75, 95];
  p.load.colors.high = '#ABCDEF';
  p.popupDetail = 'detailed';
  p.popup = { width: 360, graphHeight: 90, processes: 3, background: 'solid', graph: false };
  p.widget = {
    enabled: true,
    style: 'tiles',
    detail: 'standard',
    width: 320,
    opacity: 70,
    background: '#101010',
  };
  p.items.cpu = {
    ...p.items.cpu,
    style: 'cores',
    trend: true,
    colorMode: 'load',
    popup: 'simple',
    tileWidth: 80,
    tileHeight: 30,
    widgetStyle: 'core_wall',
    widgetDetail: 'detailed',
  };
  p.items.ping.taskbar = true;
  p.items.ping.style = 'uptime';
  p.items.disk.widget = false;
  moveKey(p, 'taskbar', 'ping', 0);
  moveKey(p, 'widget', 'network', 0);
  return p;
}

describe('moveKey', () => {
  it('moves among the shown keys and keeps hidden ones in place', () => {
    const p = defaultProfile();
    expect(taskbarKeys(p)).toEqual(['cpu', 'ram', 'gpu', 'disk', 'network']);
    moveKey(p, 'taskbar', 'cpu', 2);
    expect(taskbarKeys(p)).toEqual(['ram', 'gpu', 'cpu', 'disk', 'network']);
    expect(p.order.slice(0, 3)).toEqual(['cpu_temp', 'cpu_power', 'cpu_clock']);
    moveKey(p, 'taskbar', 'network', 0);
    expect(taskbarKeys(p)).toEqual(['network', 'ram', 'gpu', 'cpu', 'disk']);
    moveKey(p, 'taskbar', 'network', 99);
    expect(taskbarKeys(p)).toEqual(['ram', 'gpu', 'cpu', 'disk', 'network']);
    expect([...p.order].sort()).toEqual([...ITEM_KEYS].sort());
  });

  it('gives the widget its own order on first move', () => {
    const p = defaultProfile();
    moveKey(p, 'widget', 'disk', 0);
    expect(widgetKeys(p)).toEqual(['disk', 'cpu', 'ram', 'gpu', 'network']);
    expect(taskbarKeys(p)).toEqual(['cpu', 'ram', 'gpu', 'disk', 'network']);
    expect(p.widgetOrder).toHaveLength(ITEM_KEYS.length);
  });
});

describe('profile files', () => {
  it('round-trips through profileToToml', () => {
    const p = custom();
    const text = profileToToml(p, 'nord', true);
    const back = profileFromToml(parseToml(text), 'fallback');
    expect(back.themeId).toBe('nord');
    expect(back.colorLabels).toBe(true);
    expect(back.profile).toEqual(p);
    expect(profileToToml(back.profile, 'nord', true)).toBe(text);
  });

  it('leaves the theme out for Windows default', () => {
    const text = profileToToml(defaultProfile(), 'windows', false);
    expect(text).not.toContain('theme =');
    expect(profileFromToml(parseToml(text), 'x').themeId).toBe('windows');
  });

  it('keeps defaults for what it does not understand', () => {
    const doc = parseToml(
      '[display]\nmode = "fancy"\nfont_size_pt = 40.0\n[popup]\nwidth = 5\n[[items]]\nkind = "cpu"\nstyle = "uptime"\n[[items]]\nkind = "warp"',
    );
    const { profile } = profileFromToml(doc, 'stem');
    expect(profile.name).toBe('stem');
    expect(profile.mode).toBe('graph');
    expect(profile.font.size).toBe(14);
    expect(profile.popup.width).toBe(220);
    expect(profile.items.cpu.style).toBe('area');
    expect(taskbarKeys(profile)).toEqual(['cpu']);
  });
});

describe('mergeProfile', () => {
  it('fills what an older saved profile lacks', () => {
    const saved = JSON.parse(JSON.stringify(custom()));
    delete saved.fit;
    delete saved.items.fps;
    saved.order = ['ram', 'bogus', 'ram'];
    saved.font.family = 'Evil"; } body { x';
    saved.load.colors.low = 'red';
    const p = mergeProfile(saved);
    expect(p.fit).toEqual(defaultProfile().fit);
    expect(p.items.fps).toEqual(defaultProfile().items.fps);
    expect(p.order[0]).toBe('ram');
    expect(p.order).toHaveLength(ITEM_KEYS.length);
    expect(p.font.family).toBe('Evil  body  x');
    expect(p.load.colors.low).toBeNull();
    expect(p.load.colors.high).toBe('#ABCDEF');
  });

  it('returns the defaults for nothing usable', () => {
    expect(mergeProfile(null)).toEqual(defaultProfile());
    expect(mergeProfile({ name: 'x' })).toEqual(defaultProfile());
  });
});
