<script lang="ts">
  import { profileFromToml, type ProfileFile } from '../../profile.ts';
  import { app, themeOf, toast, type SourceKind } from '../../state.svelte.ts';
  import { MAX_FILE_BYTES, parseToml, slug, themeFromToml, type Theme } from '../../theme.ts';
  import Dialog from './Dialog.svelte';
  import DialogHead from './DialogHead.svelte';

  interface Result {
    name: string;
    ok: boolean;
    text: string;
  }

  let results = $state<Result[]>([]);
  let over = $state(false);

  async function importFiles(files: File[]) {
    const out: Result[] = [];
    const themes: { file: File; theme: Theme; id: string }[] = [];
    const profiles: ({ file: File } & ProfileFile)[] = [];
    for (const file of files) {
      const stem = file.name.replace(/\.toml$/i, '');
      try {
        if (file.size > MAX_FILE_BYTES * 4) throw new Error('The file is too large.');
        const text = await file.text();
        const doc = parseToml(text);
        if (doc.dark)
          themes.push({ file, theme: themeFromToml(text, { fallbackName: stem }), id: stem });
        else if (doc.display || doc.items || doc.widget)
          profiles.push({ file, ...profileFromToml(doc, stem) });
        else throw new Error("This isn't a Pulsar theme or profile.");
      } catch (err) {
        const why = (err as Error).message.split('\n')[0];
        out.push({
          name: file.name,
          ok: false,
          text: why.startsWith('Line') ? `Couldn't read it. ${why}.` : why,
        });
      }
    }
    const update: {
      theme?: Theme;
      source?: { kind: SourceKind; id: string | null };
      profile?: ProfileFile['profile'];
    } = {};
    let applied: (typeof themes)[number] | null | undefined = themes.at(-1);
    for (const t of themes) {
      Object.assign(update, { theme: t.theme, source: { kind: 'custom', id: null } });
      out.push({ name: t.file.name, ok: true, text: `Theme “${t.theme.name}”` });
    }
    for (const pr of profiles) {
      update.profile = pr.profile;
      let note = '';
      const dropped = themes.find((t) => t.id === pr.themeId || slug(t.theme.name) === pr.themeId);
      const known = (['builtin', 'community'] as const)
        .map((kind) => [kind, app.catalog[kind].find((e) => e.id === pr.themeId)] as const)
        .find(([, e]) => e);
      if (dropped) {
        applied = dropped;
        Object.assign(update, { theme: dropped.theme, source: { kind: 'custom', id: null } });
        note = ` with its theme “${dropped.theme.name}”`;
      } else if (known?.[1]) {
        applied = null;
        Object.assign(update, {
          theme: themeOf(known[1]),
          source: { kind: known[0], id: known[1].id },
        });
        note = ` with the ${known[1].name} theme`;
      } else note = `; its theme “${pr.themeId}” wasn't found, so the current one stays`;
      if (update.theme) update.theme = { ...update.theme, colorLabels: pr.colorLabels };
      out.push({ name: pr.file.name, ok: true, text: `Profile “${pr.profile.name}”${note}` });
    }
    for (const t of themes) {
      if (t === applied) continue;
      const r = out.find((x) => x.name === t.file.name);
      if (r)
        r.text += profiles.length
          ? ', not used: the profile names another theme'
          : ', not used: a later file replaced it';
    }
    if (update.theme) app.theme = update.theme;
    if (update.source) app.source = update.source;
    if (update.profile) app.profile = update.profile;
    results = out;
    const good = out.filter((r) => r.ok).length;
    if (good) toast(good === 1 ? 'Imported' : `Imported ${good} files`);
  }

  function picked(e: Event & { currentTarget: HTMLInputElement }) {
    const files = [...(e.currentTarget.files ?? [])];
    e.currentTarget.value = '';
    if (files.length) importFiles(files);
  }

  function dropped(e: DragEvent) {
    e.preventDefault();
    over = false;
    const files = [...(e.dataTransfer?.files ?? [])];
    if (files.length) importFiles(files);
  }
</script>

<Dialog name="import" labelledby="import-title" onopen={() => (results = [])}>
  <DialogHead id="import-title" title="Import" />
  <label
    class={['dropzone', over && 'over']}
    ondragover={(e) => {
      e.preventDefault();
      over = true;
    }}
    ondragleave={() => (over = false)}
    ondrop={dropped}
  >
    <input type="file" accept=".toml,text/plain" multiple class="sr-only" onchange={picked} />
    <svg viewBox="0 0 48 48" width="44" height="44" aria-hidden="true">
      <path
        d="M24 30V10M16 18l8-8 8 8"
        fill="none"
        stroke="currentColor"
        stroke-width="2.4"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
      <path
        d="M8 28v8a4 4 0 0 0 4 4h24a4 4 0 0 0 4-4v-8"
        fill="none"
        stroke="currentColor"
        stroke-width="2.4"
        stroke-linecap="round"
      />
    </svg>
    <span class="dropzone-title">Drop theme or profile files</span>
    <small>
      or click to browse. Pulsar keeps them in %APPDATA%\Pulsar\themes and
      %APPDATA%\Pulsar\profiles.
    </small>
  </label>
  <ul class="import-results" aria-live="polite">
    {#each results as r, i (i)}
      <li data-ok={r.ok}>
        <span class="import-mark" aria-hidden="true"></span>
        <span>
          <strong>{r.name}</strong>
          <br />
          {r.text}
        </span>
      </li>
    {/each}
  </ul>
  <p class="note">
    A profile switches to the theme it names when that theme is built in, shared in the gallery or
    dropped alongside it.
  </p>
</Dialog>
