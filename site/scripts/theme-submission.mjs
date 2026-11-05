// Reads a theme submission issue from the GitHub event.
//   check: prints a comment saying whether the theme can be accepted
//   add:   writes the theme into site/community and lists it
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { slug, themeFromToml, themeToToml } from '../src/lib/theme.ts';
import { builtinThemes } from './builtin.mjs';

const MAX_NAME = 48;
const site = join(dirname(fileURLToPath(import.meta.url)), '..');
const communityDir = join(site, 'community');
const mode = process.argv[2];
const event = JSON.parse(await readFile(process.env.GITHUB_EVENT_PATH, 'utf8'));
const issue = event.issue;

function field(body, heading) {
  const m = body.match(
    new RegExp(`^###\\s+${heading}\\s*\\n([\\s\\S]*?)(?=^###\\s|$(?![\\s\\S]))`, 'm'),
  );
  const value = m?.[1].trim() ?? '';
  return value === '_No response_' ? '' : value;
}

function unfence(text) {
  const m = text.match(/^```[a-z]*\s*\n([\s\S]*?)\n```\s*$/i);
  return m ? m[1] : text;
}

async function review() {
  const body = issue.body ?? '';
  const name = field(body, 'Theme name').replace(/\s+/g, ' ');
  const author = field(body, 'Credit as').replace(/\s+/g, ' ').slice(0, 40) || issue.user.login;
  const problems = [];
  let theme = null;
  if (!name) problems.push('The theme needs a name.');
  if (name.length > MAX_NAME) problems.push(`The name is longer than ${MAX_NAME} characters.`);
  try {
    theme = themeFromToml(unfence(field(body, 'Theme file')), { strict: true });
  } catch (err) {
    problems.push(...err.message.split('\n'));
  }
  const id = slug(name);
  const catalogIds = JSON.parse(await readFile(join(communityDir, 'index.json'), 'utf8')).map(
    (e) => e.id,
  );
  const builtin = await builtinThemes();
  if (name && (builtin.has(id) || catalogIds.includes(id)))
    problems.push(`A theme called "${name}" is already listed; choose another name.`);
  return { name, author, id, theme: theme && { ...theme, name, author }, problems };
}

const result = await review();

// Keeps names and keys from the issue from mentioning anyone in the reply.
const quiet = (text) => text.replace(/@/g, '@​');

if (mode === 'check') {
  console.log(
    quiet(
      result.problems.length
        ? `Thanks for sharing a theme! It can't be listed yet:\n\n${result.problems.map((p) => `- ${p}`).join('\n')}\n\nEdit the issue to fix this and it will be checked again.`
        : `Thanks for sharing **${result.name}**! The file checks out. A maintainer will take a look and add it to the gallery.`,
    ),
  );
} else if (mode === 'add') {
  if (result.problems.length) {
    console.error(result.problems.join('\n'));
    process.exit(1);
  }
  await writeFile(join(communityDir, `${result.id}.toml`), themeToToml(result.theme));
  const index = JSON.parse(await readFile(join(communityDir, 'index.json'), 'utf8'));
  index.push({
    id: result.id,
    author: result.author,
    issue: issue.number,
    added: new Date().toISOString().slice(0, 10),
  });
  await writeFile(join(communityDir, 'index.json'), `${JSON.stringify(index, null, 2)}\n`);
  const out = process.env.GITHUB_OUTPUT;
  if (out)
    await writeFile(out, `id=${result.id}\nname=${result.name.replace(/[\r\n]/g, ' ')}\n`, {
      flag: 'a',
    });
  console.log(`added ${result.id}`);
} else {
  console.error('usage: theme-submission.mjs check|add');
  process.exit(2);
}
