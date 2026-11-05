// Writes site/static/catalog.json from Pulsar's built-in themes and the community folder.
import { readFile, readdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { themeFromToml, validId } from '../src/lib/theme.ts';
import { builtinThemes } from './builtin.mjs';

const site = join(dirname(fileURLToPath(import.meta.url)), '..');
const builtinDir = join(site, '..', 'crates', 'pulsar-core', 'themes');
const communityDir = join(site, 'community');

const names = await builtinThemes();

const builtin = [];
for (const [id, name] of names) {
  if (id === 'windows') {
    builtin.push({ id, name, toml: '' });
    continue;
  }
  const toml = await readFile(join(builtinDir, `${id}.toml`), 'utf8');
  themeFromToml(toml, { strict: true });
  builtin.push({ id, name, toml });
}

const index = JSON.parse(await readFile(join(communityDir, 'index.json'), 'utf8'));
const files = new Set((await readdir(communityDir)).filter((f) => f.endsWith('.toml')));
const community = [];
for (const entry of index) {
  if (!validId(entry.id) || names.has(entry.id)) throw new Error(`bad community id ${entry.id}`);
  const file = `${entry.id}.toml`;
  if (!files.delete(file)) throw new Error(`community/${file} is listed but missing`);
  const toml = await readFile(join(communityDir, file), 'utf8');
  const theme = themeFromToml(toml, { strict: true });
  community.push({ ...entry, name: theme.name, author: entry.author ?? theme.author, toml });
}
if (files.size) throw new Error(`not in community/index.json: ${[...files].join(', ')}`);
community.sort(
  (a, b) => (b.added ?? '').localeCompare(a.added ?? '') || a.name.localeCompare(b.name),
);

await writeFile(join(site, 'static', 'catalog.json'), JSON.stringify({ builtin, community }));
console.log(`catalog.json: ${builtin.length} built-in, ${community.length} community`);
