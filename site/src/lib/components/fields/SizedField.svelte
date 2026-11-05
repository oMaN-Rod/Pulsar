<script lang="ts">
  import RangeField from './RangeField.svelte';

  let {
    name,
    autoName,
    value = $bindable(),
    min,
    max,
    fallback,
    unit = ' px',
  }: {
    name: string;
    autoName: string;
    value: number | null;
    min: number;
    max: number;
    fallback: number;
    unit?: string;
  } = $props();
</script>

<div class="field sized">
  <label class="check">
    <input
      type="checkbox"
      checked={value === null}
      onchange={(e) => (value = e.currentTarget.checked ? null : fallback)}
    />
    <span>{autoName}</span>
  </label>
  <RangeField
    {name}
    value={value ?? fallback}
    {min}
    {max}
    output={value === null ? 'auto' : `${value}${unit}`}
    disabled={value === null}
    oninput={(v) => (value = v)}
  />
</div>
