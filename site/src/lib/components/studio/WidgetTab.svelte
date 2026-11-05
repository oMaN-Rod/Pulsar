<script lang="ts">
  import { DETAILS, WIDGET_STYLES, type Detail, type WidgetStyle } from '../../profile.ts';
  import { app } from '../../state.svelte.ts';
  import { resolve } from '../../theme.ts';
  import { keep } from '../../ui.ts';
  import CheckField from '../fields/CheckField.svelte';
  import ChoiceField from '../fields/ChoiceField.svelte';
  import RangeField from '../fields/RangeField.svelte';
  import SegField from '../fields/SegField.svelte';

  const DETAIL_OPTIONS: [Detail, string][] = DETAILS.map((d) => [d.id, d.name]);
  const w = $derived(app.profile.widget);
  const shown = $derived(resolve(app.theme, app.light));
  const bg = $derived(w.background ?? shown.panel);
</script>

<CheckField name="Show the desktop widget" bind:checked={w.enabled} />
<ChoiceField
  name="Style"
  value={w.style}
  options={WIDGET_STYLES.map((s) => [s.id, s.name])}
  onchange={(v) => (w.style = v as WidgetStyle)}
/>
<SegField
  name="Detail"
  value={w.detail}
  options={DETAIL_OPTIONS}
  onchange={(v) => (w.detail = v)}
/>
<RangeField
  name="Width"
  min={220}
  max={600}
  value={w.width}
  output="{w.width} px"
  oninput={(v) => (w.width = v)}
/>
<div class="field">
  <span class="field-name">Background</span>
  <label class="check">
    <input
      type="checkbox"
      checked={!w.background}
      onchange={(e) => (w.background = e.currentTarget.checked ? null : shown.panel)}
    />
    <span>The theme's panel color</span>
  </label>
  <label class={['surface', !w.background && 'muted']}>
    <input
      type="color"
      {@attach keep(bg.toLowerCase())}
      oninput={(e) => (w.background = e.currentTarget.value.toUpperCase())}
    />
    <span class="surface-text">
      <span>Own color</span>
      <small>Behind the widget only</small>
    </span>
    <code>{w.background ? bg : `${bg} theme`}</code>
  </label>
</div>
<RangeField
  name="Background opacity"
  min={0}
  max={100}
  value={w.opacity}
  output="{w.opacity}%"
  oninput={(v) => (w.opacity = v)}
/>
<p class="note">
  Choose what the widget shows under Items, where each item can also have its own style and detail.
</p>
