import { describe, expect, it } from 'vitest';
import { parseToml, slug, themeFromToml, themeToToml, validId } from './theme.ts';

const NORD = `name = "Nord"
author = "Pulsar"
based_on = "Nord by Arctic Ice Studio"

[dark]
text = "#ECEFF4"
label = "#a5b1c2"
panel = "#2E3440"
panel_opacity = 85

[dark.items]
cpu = "#88C0D0"
network = "#EBCB8B" # trailing comment
`;

describe('parseToml', () => {
  it('reads tables, values, lists and comments', () => {
    const doc = parseToml(
      `a = "x # not a comment"\nb = 3\nc = true\nd = [1, "two", 'three']\n[t.u]\nv = -2\n[[items]]\nkind = "cpu"\n[[items]]\nkind = "ram"`,
    );
    expect(doc.a).toBe('x # not a comment');
    expect(doc.b).toBe(3);
    expect(doc.c).toBe(true);
    expect(doc.d).toEqual([1, 'two', 'three']);
    expect(doc.t).toEqual({ u: { v: -2 } });
    expect(doc.items).toEqual([{ kind: 'cpu' }, { kind: 'ram' }]);
  });

  it('joins lists that span lines', () => {
    expect(parseToml('items = [\n  "cpu",\n  "ram", # note\n]').items).toEqual(['cpu', 'ram']);
  });

  it('builds objects without a prototype', () => {
    expect(Object.getPrototypeOf(parseToml('[dark]\ntext = "#FFFFFF"'))).toBeNull();
  });

  it.each([
    ['__proto__ = 1', '"__proto__" can\'t be used as a name'],
    ['"__proto__" = 1', '"__proto__" can\'t be used as a name'],
    ['[__proto__]', '"__proto__" can\'t be used as a name'],
    ['[dark.constructor]', '"constructor" can\'t be used as a name'],
    ['[[prototype]]', '"prototype" can\'t be used as a name'],
  ])('rejects the unsafe key in %s', (text, why) => {
    expect(() => parseToml(text)).toThrow(`Line 1: ${why}`);
    expect(({} as Record<string, unknown>).x).toBeUndefined();
  });

  it('rejects duplicates and unsupported values', () => {
    expect(() => parseToml('a = 1\na = 2')).toThrow('Line 2: "a" is set twice');
    expect(() => parseToml('a = 1979-05-27')).toThrow('Line 1: unsupported value');
    expect(() => parseToml('just words')).toThrow('Line 1: expected key = value');
    expect(() => parseToml('a = 1\n[a]')).toThrow('Line 2: "a" is not a table');
  });
});

describe('themeFromToml', () => {
  it('reads a theme and uppercases colors', () => {
    const t = themeFromToml(NORD, { strict: true });
    expect(t.name).toBe('Nord');
    expect(t.author).toBe('Pulsar');
    expect(t.basedOn).toBe('Nord by Arctic Ice Studio');
    expect(t.dark.label).toBe('#A5B1C2');
    expect(t.dark.panelOpacity).toBe(85);
    expect(t.dark.items).toEqual({ cpu: '#88C0D0', network: '#EBCB8B' });
    expect(t.light).toBeNull();
  });

  it('ignores unknown keys unless strict', () => {
    const text = 'name = "X"\nextra = 1\n[dark]\nglow = "#FFFFFF"\n[dark.items]\nfan = "#FFFFFF"';
    expect(themeFromToml(text).name).toBe('X');
    expect(() => themeFromToml(text, { strict: true })).toThrow(
      [
        'Unknown key "extra" in the file.',
        'Unknown key "glow" in [dark].',
        'Unknown key "fan" in [dark.items].',
      ].join('\n'),
    );
  });

  it('reports every problem', () => {
    const text =
      'name = 3\nauthor = 7\ncolor_labels = "yes"\n[dark]\ntext = "white"\ntile_opacity = 300\n[dark.items]\ncpu = "#12345"';
    expect(() => themeFromToml(text)).toThrow(
      [
        'name must be text.',
        'author must be text of up to 80 characters.',
        'color_labels must be true or false.',
        'dark.text must look like "#RRGGBB".',
        'dark.tile_opacity must be a whole number from 0 to 100.',
        'dark.items.cpu must look like "#RRGGBB".',
      ].join('\n'),
    );
  });

  it('needs a dark table and, when strict, a name', () => {
    expect(() => themeFromToml('name = "X"')).toThrow('The file needs a [dark] table.');
    expect(() => themeFromToml('name = " "\n[dark]', { strict: true })).toThrow('name is empty.');
    expect(themeFromToml('[dark]', { fallbackName: 'stem' }).name).toBe('stem');
  });

  it('caps opacity at 100 and refuses large files', () => {
    expect(themeFromToml('[dark]\npanel_opacity = 200').dark.panelOpacity).toBe(100);
    expect(() => themeFromToml(`[dark]\n#${'x'.repeat(64 * 1024)}`)).toThrow(
      'The file is larger than 64 KB.',
    );
  });

  it('round-trips through themeToToml', () => {
    const t = themeFromToml(NORD, { strict: true });
    const light = {
      ...t,
      colorLabels: true,
      light: { text: '#111111', tileOpacity: 0, items: { ram: '#ABCDEF' } },
    };
    for (const theme of [t, light]) {
      const text = themeToToml(theme);
      expect(themeFromToml(text, { strict: true })).toEqual(theme);
      expect(themeToToml(themeFromToml(text))).toBe(text);
    }
  });
});

describe('ids', () => {
  it('slugs names like Pulsar', () => {
    expect(slug('Nord (modified)')).toBe('nord-modified');
    expect(slug('  ')).toBe('theme');
    expect(slug('Ünïcode!', 'profile')).toBe('n-code');
  });

  it('validates ids', () => {
    expect(validId('tokyo-night')).toBe(true);
    expect(validId('Tokyo')).toBe(false);
    expect(validId('')).toBe(false);
  });
});
