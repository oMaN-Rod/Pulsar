export type Rgb = [number, number, number];

export const isHex = (s: unknown): s is string =>
  typeof s === 'string' && /^#[0-9a-fA-F]{6}$/.test(s);

export function rgb(hex: string): Rgb {
  const h = hex.slice(1);
  return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16)) as Rgb;
}

export function hex(c: number[]): string {
  return (
    '#' +
    c
      .map((v) =>
        Math.round(Math.max(0, Math.min(255, v)))
          .toString(16)
          .padStart(2, '0'),
      )
      .join('')
      .toUpperCase()
  );
}

export function mix(a: string, b: string, t: number): string {
  const x = rgb(a);
  const y = rgb(b);
  return hex(x.map((v, i) => v + (y[i] - v) * t));
}

export function rgba(h: string, a: number): string {
  const [r, g, b] = rgb(h);
  return `rgba(${r},${g},${b},${a})`;
}

export function luminance(h: string): number {
  const w = [0.2126, 0.7152, 0.0722];
  return rgb(h).reduce((sum, v, i) => {
    const c = v / 255;
    return sum + w[i] * (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  }, 0);
}

export function contrast(a: string, b: string): number {
  const x = luminance(a);
  const y = luminance(b);
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}

export function darkenTo(c: string, bg: string, min: number): string {
  let out = c;
  for (let t = 0.05; contrast(out, bg) < min && t <= 1; t += 0.05) out = mix(c, '#000000', t);
  return out;
}

export function toHsl(h: string): Rgb {
  const [r, g, b] = rgb(h).map((v) => v / 255);
  const mx = Math.max(r, g, b);
  const mn = Math.min(r, g, b);
  const d = mx - mn;
  const l = (mx + mn) / 2;
  if (!d) return [0, 0, l * 100];
  const s = d / (1 - Math.abs(2 * l - 1));
  let hue = mx === r ? ((g - b) / d) % 6 : mx === g ? (b - r) / d + 2 : (r - g) / d + 4;
  hue = (hue * 60 + 360) % 360;
  return [hue, s * 100, l * 100];
}

export function fromHsl(h: number, s: number, l: number): string {
  const hue = ((h % 360) + 360) % 360;
  const sat = Math.max(0, Math.min(100, s)) / 100;
  const lit = Math.max(0, Math.min(100, l)) / 100;
  const a = sat * Math.min(lit, 1 - lit);
  const f = (n: number) => {
    const k = (n + hue / 30) % 12;
    return lit - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
  };
  return hex([f(0) * 255, f(8) * 255, f(4) * 255]);
}

export function inkOn(bg: string): string {
  return contrast('#FFFFFF', bg) >= contrast('#0C0C0E', bg) ? '#FFFFFF' : '#0C0C0E';
}
