<script lang="ts">
  import { tick } from 'svelte';
  import { app } from '../../state.svelte.ts';
  import { cloneTheme, MAX_CREDIT, slug } from '../../theme.ts';
  import Dialog from './Dialog.svelte';
  import DialogHead from './DialogHead.svelte';

  let name = $state('');
  let author = $state('');
  let nameInput: HTMLInputElement;

  function submit(e: SubmitEvent) {
    e.preventDefault();
    const nextName = name.trim();
    const nextAuthor = author.replace(/\s+/g, ' ').trim() || null;
    if (!nextName) return;
    if (nextName !== app.theme.name || nextAuthor !== (app.theme.author ?? null)) {
      app.theme = { ...cloneTheme($state.snapshot(app.theme)), name: nextName, author: nextAuthor };
      app.source = { kind: 'custom', id: null };
    }
    app.dialog = null;
  }
</script>

<Dialog
  name="rename"
  labelledby="rename-title"
  onopen={() => {
    name = app.theme.name;
    author = app.theme.author ?? '';
  }}
  onshown={() => tick().then(() => nameInput.select())}
>
  <form class="dialog-form" onsubmit={submit}>
    <DialogHead id="rename-title" title="Theme details" />
    <label class="field">
      <span class="field-name">Name</span>
      <input
        type="text"
        class="text-input"
        required
        maxlength="60"
        spellcheck="false"
        autocomplete="off"
        bind:value={name}
        bind:this={nameInput}
      />
    </label>
    <!-- prettier-ignore -->
    <p class="note">Exports as <code>{slug(name.trim() || 'theme')}.toml</code>.</p>
    <label class="field">
      <span class="field-name">Author</span>
      <input
        type="text"
        class="text-input"
        maxlength={MAX_CREDIT}
        spellcheck="false"
        autocomplete="nickname"
        placeholder="Your name or GitHub username"
        bind:value={author}
      />
    </label>
    <p class="note based-on" hidden={!app.theme.basedOn}>
      Based on {app.theme.basedOn}. That stays in the file as credit.
    </p>
    <div class="actions">
      <button type="submit" class="primary">Save</button>
      <button type="button" class="quiet" onclick={() => (app.dialog = null)}>Cancel</button>
    </div>
  </form>
</Dialog>
