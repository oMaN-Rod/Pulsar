import { describe, expect, it } from 'vitest';
import {
  cleanLabel,
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
  p.textGaps = { item: 20, label: 0, row: 3 };
  p.overrides = {
    tile: '#102030',
    tileOpacity: 0,
    panel: '#405060',
    panelOpacity: 80,
    label: '#AAAAAA',
    value: '#FFFFFF',
    labelLight: '#333333',
    valueLight: null,
  };
  p.units = { temperature: 'fahrenheit', network: 'bits', memory: 'bytes' };
  p.network = {
    show: 'download',
    uploadOnTop: true,
    splitTiles: true,
    connectionDot: true,
    downLabel: 'In',
    upLabel: 'Out',
    popupShow: 'upload',
    minDownBps: 10240,
    minUpBps: 1000,
  };
  p.fit = { enabled: false, removeFrom: 'left', noRoom: 'tray', otherSide: false };
  p.load.thresholds.celsius = [60, 75, 95];
  p.load.colors.high = '#ABCDEF';
  p.disk.value = 'used';
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
    color: '#12AB34',
    label: 'Proc',
    shortLabel: 'P',
    popupWidth: 420,
    popupGraphHeight: 120,
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
    expect({ ...back.profile, source: null }).toEqual(p);
    expect(parseToml(profileToToml(back.profile, 'nord', true))).toEqual(parseToml(text));
  });

  it("keeps an imported file's settings the site leaves alone", () => {
    const doc = parseToml(
      [
        'name = "Desk"',
        '[display]',
        'position = "floating"',
        'theme = "nord"',
        'mode = "graph"',
        '[ping]',
        'host = "9.9.9.9"',
        'interval_ms = 2000',
        '[disk]',
        'drives = ["C", "D"]',
        '[network]',
        'show = "both"',
        'adapters = ["Ethernet"]',
        '[[items]]',
        'kind = "cpu"',
        'enabled = true',
        'color = "#FF0000"',
        'label = "Processor 12345"',
        'popup_width = 9000',
        '[[items]]',
        'kind = "warp"',
        'enabled = true',
      ].join('\n'),
    );
    const { profile } = profileFromToml(doc, 'x');
    profile.mode = 'text';
    profile.items.cpu.trend = true;
    const out = parseToml(profileToToml(profile, 'nord', false));
    const display = out.display as Record<string, unknown>;
    expect(display.position).toBe('floating');
    expect(display.mode).toBe('text');
    expect(out.ping).toEqual({ host: '9.9.9.9', interval_ms: 2000 });
    expect(out.disk).toEqual({ drives: ['C', 'D'] });
    expect((out.network as Record<string, unknown>).adapters).toEqual(['Ethernet']);
    const items = out.items as Record<string, unknown>[];
    const cpu = items.find((t) => t.kind === 'cpu');
    expect(cpu?.color).toBe('#FF0000');
    expect(profile.items.cpu.label).toBe('Processor 12');
    expect(profile.items.cpu.popupWidth).toBe(800);
    expect(cpu?.trend).toBe(true);
    expect(items.some((t) => t.kind === 'warp')).toBe(true);
  });

  it('writes whole items for kinds an imported file lacks', () => {
    const doc = parseToml('[[items]]\nkind = "cpu"\nenabled = true');
    const { profile } = profileFromToml(doc, 'x');
    expect(parseToml(profileToToml(profile, 'windows', false)).items).toEqual(doc.items);
    profile.items.ram.taskbar = true;
    const items = parseToml(profileToToml(profile, 'windows', false)).items as Record<
      string,
      unknown
    >[];
    expect(items.map((t) => t.kind)).toEqual(['cpu', 'ram']);
    expect(items[1]).toMatchObject({ kind: 'ram', enabled: true, graph: true });
  });

  it('keeps the rest of a table the site stops writing', () => {
    const doc = parseToml(
      [
        '[disk]',
        'value = "used"',
        'drives = ["C"]',
        '[display.load_colors]',
        'high = "#ABCDEF"',
      ].join('\n'),
    );
    const { profile } = profileFromToml(doc, 'x');
    profile.disk.value = 'activity';
    profile.load.colors.high = null;
    const out = parseToml(profileToToml(profile, 'windows', false));
    expect(out.disk).toEqual({ drives: ['C'] });
    expect((out.display as Record<string, unknown>).load_colors).toBeUndefined();
  });

  it('adds no items to an imported file without any', () => {
    const { profile } = profileFromToml(parseToml('[display]\ntheme = "nord"'), 'x');
    expect(parseToml(profileToToml(profile, 'nord', false)).items).toBeUndefined();
  });

  it('applies a theme change to an imported file', () => {
    const doc = parseToml('[display]\ntheme = "nord"');
    const { profile } = profileFromToml(doc, 'x');
    const out = parseToml(profileToToml(profile, 'windows', false));
    expect((out.display as Record<string, unknown>).theme).toBeUndefined();
  });

  it('reads and writes a floating overlay', () => {
    const p = defaultProfile();
    p.position = 'floating';
    const back = profileFromToml(parseToml(profileToToml(p, 'windows', false)), 'x');
    expect(back.profile.position).toBe('floating');
  });

  it('writes network settings only when they differ from Pulsar', () => {
    const text = profileToToml(defaultProfile(), 'windows', false);
    expect(parseToml(text).network).toEqual({ show: 'both' });
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

describe('cleanLabel', () => {
  it('cleans a label as Pulsar does', () => {
    expect(cleanLabel(' CPU ')).toBe('CPU');
    expect(cleanLabel('a	b⁦c')).toBe('abc');
    expect(cleanLabel('Processor 12345')).toBe('Processor 12');
    expect(cleanLabel('Processor   x')).toBe('Processor');
    expect(cleanLabel('👨‍👩‍👧'.repeat(13))).toBe('👨‍👩‍👧'.repeat(12));
    expect(cleanLabel('')).toBeNull();
    expect(cleanLabel(3)).toBeNull();
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
    saved.position = 'top';
    saved.disk = { value: 'free' };
    saved.textGaps = { item: 99, label: 'wide' };
    saved.overrides = { tile: 'blue', tileOpacity: 150, panelOpacity: 2.5, value: '#abcdef' };
    saved.network = {
      show: 'sideways',
      splitTiles: 'yes',
      downLabel: 7,
      upLabel: ' Up ',
      popupShow: 'download',
      minDownBps: -5,
      minUpBps: 2.5,
    };
    saved.items.ram = {
      ...saved.items.ram,
      color: 'red',
      label: '  Memory‮all of it  ',
      shortLabel: '   ',
      popupWidth: 5000,
      popupGraphHeight: 1.5,
    };
    const p = mergeProfile(saved);
    expect(p.fit).toEqual(defaultProfile().fit);
    expect(p.items.fps).toEqual(defaultProfile().items.fps);
    expect(p.order[0]).toBe('ram');
    expect(p.order).toHaveLength(ITEM_KEYS.length);
    expect(p.font.family).toBe('Evil  body  x');
    expect(p.load.colors.low).toBeNull();
    expect(p.load.colors.high).toBe('#ABCDEF');
    expect(p.position).toBe('right');
    expect(p.disk.value).toBe('activity');
    expect(p.textGaps).toEqual({ item: 40, label: 4, row: 0 });
    expect(p.overrides).toEqual({
      ...defaultProfile().overrides,
      tileOpacity: 100,
      value: '#ABCDEF',
    });
    expect(p.items.ram).toMatchObject({
      color: null,
      label: 'Memoryall of',
      shortLabel: null,
      popupWidth: 800,
      popupGraphHeight: null,
    });
    expect(p.items.cpu.color).toBe('#12AB34');
    expect(p.network).toEqual({
      ...defaultProfile().network,
      upLabel: 'Up',
      popupShow: 'download',
    });
  });

  it('returns the defaults for nothing usable', () => {
    expect(mergeProfile(null)).toEqual(defaultProfile());
    expect(mergeProfile({ name: 'x' })).toEqual(defaultProfile());
  });
});
