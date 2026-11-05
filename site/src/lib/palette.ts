import { hex, toHsl } from './color.ts';

export function loadImage(file: File): Promise<{ url: string; img: HTMLImageElement }> {
  return new Promise((resolve, reject) => {
    if (!file.type.startsWith('image/')) {
      reject(new Error('That file is not an image.'));
      return;
    }
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => resolve({ url, img });
    img.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error('The image could not be read.'));
    };
    img.src = url;
  });
}

/**
 * The image's most characteristic colors, most vivid first: pixels are
 * grouped into coarse color buckets, and each bucket scores by how much of
 * the image it covers and how saturated it is.
 */
export function extractPalette(img: HTMLImageElement, count = 6): string[] {
  const size = 72;
  const canvas = document.createElement('canvas');
  canvas.width = size;
  canvas.height = Math.max(1, Math.round((size * img.naturalHeight) / img.naturalWidth));
  const ctx = canvas.getContext('2d', { willReadFrequently: true });
  if (!ctx) return [];
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height);
  const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);

  const buckets = new Map<number, { n: number; r: number; g: number; b: number }>();
  for (let i = 0; i < data.length; i += 4) {
    if (data[i + 3] < 128) continue;
    const key = ((data[i] >> 4) << 8) | ((data[i + 1] >> 4) << 4) | (data[i + 2] >> 4);
    const b = buckets.get(key) ?? { n: 0, r: 0, g: 0, b: 0 };
    b.n++;
    b.r += data[i];
    b.g += data[i + 1];
    b.b += data[i + 2];
    buckets.set(key, b);
  }

  const candidates = [...buckets.values()]
    .map((b) => {
      const c = hex([b.r / b.n, b.g / b.n, b.b / b.n]);
      const [h, s, l] = toHsl(c);
      const balance = 1 - Math.abs(l - 55) / 55;
      return { c, h, s, l, score: Math.sqrt(b.n) * (0.25 + (s / 100) * balance) };
    })
    .sort((a, b) => b.score - a.score);

  const picked: typeof candidates = [];
  for (const cand of candidates) {
    const near = picked.some((p) => {
      const dh = Math.min(Math.abs(p.h - cand.h), 360 - Math.abs(p.h - cand.h));
      return dh < 18 && Math.abs(p.l - cand.l) < 18;
    });
    if (!near) picked.push(cand);
    if (picked.length === count) break;
  }
  return picked
    .sort((a, b) => b.s * (1 - Math.abs(b.l - 55) / 55) - a.s * (1 - Math.abs(a.l - 55) / 55))
    .map((p) => p.c);
}
