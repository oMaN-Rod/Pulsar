<script lang="ts">
  import { keep, rangeFill } from '../../ui.ts';

  let {
    name,
    value,
    min,
    max,
    step = 1,
    output,
    rail,
    class: cls,
    disabled = false,
    oninput,
  }: {
    name: string;
    value: number;
    min: number;
    max: number;
    step?: number;
    output: string;
    rail?: string;
    class?: string;
    disabled?: boolean;
    oninput: (value: number) => void;
  } = $props();
</script>

<label class="field">
  <span class="field-name">
    {name}
    <output>{output}</output>
  </span>
  <input
    type="range"
    class={cls}
    {min}
    {max}
    {step}
    {disabled}
    style:--p={rangeFill(value, min, max)}
    style:--rail-override={rail}
    {@attach keep(value)}
    oninput={(e) => oninput(Number(e.currentTarget.value))}
  />
</label>
