import { themeToToml, type Theme } from './theme.ts';

export const REPO = 'oMaN-Rod/Pulsar';

export interface CatalogEntry {
  id: string;
  name: string;
  toml: string;
  author?: string | null;
  issue?: number;
  added?: string;
}

export interface Catalog {
  builtin: CatalogEntry[];
  community: CatalogEntry[];
}

export function submitUrl(theme: Theme): string {
  const q = new URLSearchParams({
    template: 'theme.yml',
    title: `Theme: ${theme.name}`,
    'theme-name': theme.name,
    credit: theme.author ?? '',
    toml: themeToToml(theme),
  });
  return `https://github.com/${REPO}/issues/new?${q}`;
}

export async function loadCatalog(url: string): Promise<Catalog> {
  const res = await fetch(url, { cache: 'no-cache' });
  if (!res.ok) throw new Error(`catalog.json: ${res.status}`);
  return res.json();
}
