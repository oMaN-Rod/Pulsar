<script lang="ts">
  import { DETAILS, WIDGET_MATERIALS, type Detail } from '../../profile.ts';
  import { app } from '../../state.svelte.ts';
  import CheckField from '../fields/CheckField.svelte';
  import RangeField from '../fields/RangeField.svelte';
  import SegField from '../fields/SegField.svelte';

  const DETAIL_OPTIONS: [Detail, string][] = DETAILS.map((d) => [d.id, d.name]);
  const pop = $derived(app.profile.popup);
</script>

<SegField
  name="Detail"
  value={app.profile.popupDetail}
  options={DETAIL_OPTIONS}
  onchange={(v) => (app.profile.popupDetail = v)}
/>
<SegField
  name="Material"
  value={pop.background}
  options={WIDGET_MATERIALS.map((m) => [m.id, m.name])}
  onchange={(v) => (pop.background = v)}
/>
{#if pop.background === 'glass' || pop.background === 'liquid'}
  <RangeField
    name="Glass tint"
    min={0}
    max={100}
    value={pop.glassTint}
    output={pop.glassTint ? `${pop.glassTint}%` : 'Clear glass'}
    oninput={(v) => (pop.glassTint = v)}
  />
{/if}
<CheckField name="Show the graph" bind:checked={pop.graph} />
<RangeField
  name="Width"
  min={220}
  max={800}
  value={pop.width}
  output="{pop.width} px"
  oninput={(v) => (pop.width = v)}
/>
<RangeField
  name="Graph height"
  min={24}
  max={400}
  value={pop.graphHeight}
  output="{pop.graphHeight} px"
  oninput={(v) => (pop.graphHeight = v)}
/>
<RangeField
  name="Top processes"
  min={0}
  max={15}
  value={pop.processes}
  output={`${pop.processes}`}
  oninput={(v) => (pop.processes = v)}
/>
<p class="note">
  Hover an item on the taskbar to see its popup. Items can override the detail under Items.
</p>
