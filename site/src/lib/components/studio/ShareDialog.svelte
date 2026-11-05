<script lang="ts">
  import { submitUrl } from '../../community.ts';
  import { app, copyTheme } from '../../state.svelte.ts';
  import Dialog from './Dialog.svelte';
  import DialogHead from './DialogHead.svelte';
</script>

<Dialog name="share" labelledby="share-title">
  <DialogHead id="share-title" title="Share" />
  <p class="share-lede">
    Add <strong>{app.theme.name}</strong>
    to the community gallery.
  </p>
  <p class="note">
    {app.theme.author
      ? `Credited to ${app.theme.author}. Change it with the pencil beside the name.`
      : 'No author set, so the gallery will credit your GitHub username. Set one with the pencil beside the name.'}
  </p>
  <ol class="steps">
    <li>The form opens on GitHub with the theme already filled in.</li>
    <li>A check replies on the issue within a minute.</li>
    <li>Once a maintainer approves it, it shows under Community for everyone.</li>
  </ol>
  <div class="actions">
    <a
      class="primary link-primary"
      target="_blank"
      rel="noopener"
      href={submitUrl($state.snapshot(app.theme))}
    >
      Open the submission form
    </a>
    <button type="button" class="quiet" onclick={copyTheme}>Copy the theme file</button>
  </div>
  <p class="note">
    You'll need a GitHub account. The theme is shared under the project's license, GPL-3.0.
  </p>
</Dialog>
