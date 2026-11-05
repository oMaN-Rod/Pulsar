import { resolve, type Theme } from './theme.ts';

export function orb(theme: Theme): string {
  const v = resolve(theme, false);
  const c = [v.items.cpu, v.items.ram, v.items.gpu, v.items.network];
  return `conic-gradient(${[...c, c[0]].join(',')})`;
}

export function download(name: string, text: string, type = 'text/plain') {
  const url = URL.createObjectURL(new Blob([text], { type }));
  const a = Object.assign(document.createElement('a'), { href: url, download: name });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export const rangeFill = (value: number, min: number, max: number) =>
  `${((value - min) / (max - min)) * 100}%`;

/** Keeps a field's value in step with `value`, except while someone is typing in it. */
export function keep(value: string | number) {
  return (node: HTMLInputElement) => {
    if (document.activeElement !== node) node.value = String(value);
    const refresh = () => (node.value = String(value));
    node.addEventListener('blur', refresh);
    return () => node.removeEventListener('blur', refresh);
  };
}
