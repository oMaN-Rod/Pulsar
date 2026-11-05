<script lang="ts">
  let {
    name,
    value,
    options,
    disabled = false,
    onchange,
  }: {
    name: string;
    value: string;
    options: [string, string][];
    disabled?: boolean;
    onchange: (value: string) => void;
  } = $props();
</script>

<label class="field select">
  <span class="field-name">{name}</span>
  <select {disabled} onchange={(e) => onchange(e.currentTarget.value)}>
    {#each options as [v, n] (v)}
      <option value={v} selected={v === value}>{n}</option>
    {/each}
  </select>
</label>
