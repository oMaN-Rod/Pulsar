<script lang="ts">
  import { profileToToml } from '../../profile.ts';
  import { app, copyTheme, toast } from '../../state.svelte.ts';
  import { slug, themeToToml } from '../../theme.ts';
  import { download, keep } from '../../ui.ts';
  import Dialog from './Dialog.svelte';
  import DialogHead from './DialogHead.svelte';

  const themeId = $derived(
    app.source.kind === 'builtin' && app.source.id ? app.source.id : slug(app.theme.name),
  );

  function saveProfile() {
    const p = app.profile;
    const theme = $state.snapshot(app.theme);
    const id = themeId;
    download(
      `${slug(p.name, 'profile')}.toml`,
      profileToToml($state.snapshot(p), id, theme.colorLabels),
    );
    if (app.source.kind !== 'builtin') {
      setTimeout(() => download(`${id}.toml`, themeToToml(theme)), 400);
      toast('Saved the profile and its theme');
    } else toast('Profile saved');
  }
</script>

<Dialog name="export" labelledby="export-title">
  <DialogHead id="export-title" title="Export" />
  <section>
    <h3 class="eyebrow">Theme</h3>
    <!-- prettier-ignore -->
    <p class="note">
      Colors only, as <code>{themeId}.toml</code>. Save it to Pulsar's themes folder,
      %APPDATA%\Pulsar\themes, then choose it under Settings › Theme.
    </p>
    <div class="actions">
      <button
        type="button"
        class="primary"
        onclick={() =>
          download(`${slug(app.theme.name)}.toml`, themeToToml($state.snapshot(app.theme)))}
      >
        Save theme
      </button>
      <button type="button" class="quiet" onclick={copyTheme}>Copy</button>
    </div>
  </section>
  <section>
    <h3 class="eyebrow">Profile</h3>
    <p class="note">
      Everything else: items, styles, colors by load, units, popups, widget and text, pointing at
      this theme. Save it to %APPDATA%\Pulsar\profiles, then switch to it from Pulsar's tray menu.
    </p>
    <label class="field">
      <span class="field-name">Profile name</span>
      <input
        type="text"
        class="text-input"
        spellcheck="false"
        {@attach keep(app.profile.name)}
        oninput={(e) => (app.profile.name = e.currentTarget.value)}
      />
    </label>
    <div class="actions">
      <button type="button" class="primary" onclick={saveProfile}>Save profile</button>
    </div>
  </section>
</Dialog>
