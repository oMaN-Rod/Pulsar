<script lang="ts">
  import { untrack, type Snippet } from 'svelte';
  import { app, type DialogName } from '../../state.svelte.ts';

  let {
    name,
    labelledby,
    onopen,
    onshown,
    children,
  }: {
    name: DialogName;
    labelledby: string;
    onopen?: () => void;
    onshown?: () => void;
    children: Snippet;
  } = $props();

  let el: HTMLDialogElement;

  $effect(() => {
    const open = app.dialog === name;
    untrack(() => {
      if (open && !el.open) {
        onopen?.();
        el.showModal();
        onshown?.();
      } else if (!open && el.open) el.close();
    });
  });
</script>

<dialog
  class="dialog"
  aria-labelledby={labelledby}
  bind:this={el}
  onclose={() => {
    if (app.dialog === name) app.dialog = null;
  }}
  onclick={(e) => {
    if (e.target === el) el.close();
  }}
>
  {@render children()}
</dialog>
