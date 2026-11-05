<script lang="ts" generics="T extends string">
  let {
    name,
    value,
    options,
    onchange,
  }: { name: string; value: T; options: [T, string][]; onchange: (value: T) => void } = $props();
</script>

<div class="field">
  <span class="field-name">{name}</span>
  <div class="seg small" role="group" aria-label={name}>
    {#each options as [v, n] (v)}
      <button type="button" aria-pressed={value === v} onclick={() => onchange(v)}>{n}</button>
    {/each}
  </div>
</div>
