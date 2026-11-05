import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

export const repo = join(dirname(fileURLToPath(import.meta.url)), '..', '..');

/** Pulsar's built-in theme ids and names, in its order, read from theme.rs. */
export async function builtinThemes() {
  const source = await readFile(join(repo, 'crates', 'pulsar-core', 'src', 'theme.rs'), 'utf8');
  const names = new Map(
    [...source.matchAll(/\(\s*(?:"([a-z0-9-]+)"|DEFAULT_ID),\s*crate::tr_noop!\("([^"]+)"\)/g)].map(
      (m) => [m[1] ?? 'windows', m[2]],
    ),
  );
  if (!names.has('windows')) throw new Error('could not find the built-in theme names in theme.rs');
  return names;
}
