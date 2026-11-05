<script lang="ts">
  import { app } from '../../state.svelte.ts';

  let { id, title }: { id: string; title: string } = $props();
</script>

<div class="dialog-head">
  <h2 {id}>{title}</h2>
  <button type="button" class="dialog-close" aria-label="Close" onclick={() => (app.dialog = null)}>
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
      <path
        d="m4 4 8 8M12 4l-8 8"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linecap="round"
      />
    </svg>
  </button>
</div>
