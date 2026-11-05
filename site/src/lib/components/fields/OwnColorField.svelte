<script lang="ts">
  import { keep } from '../../ui.ts';

  let {
    name,
    themeName,
    hint,
    value = $bindable(),
    fallback,
  }: {
    name: string;
    themeName: string;
    hint?: string;
    value: string | null;
    fallback: string;
  } = $props();

  const shown = $derived(value ?? fallback);
</script>

<div class="field">
  <span class="field-name">{name}</span>
  <label class="check">
    <input
      type="checkbox"
      checked={!value}
      onchange={(e) => (value = e.currentTarget.checked ? null : fallback.toUpperCase())}
    />
    <span>{themeName}</span>
  </label>
  <label class={['surface', !value && 'muted']}>
    <input
      type="color"
      {@attach keep(shown.toLowerCase())}
      oninput={(e) => (value = e.currentTarget.value.toUpperCase())}
    />
    <span class="surface-text">
      <span>Own color</span>
      {#if hint}<small>{hint}</small>{/if}
    </span>
    <code>{value ? shown : `${shown} theme`}</code>
  </label>
</div>
