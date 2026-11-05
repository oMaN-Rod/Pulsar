<script lang="ts">
  import {
    COLOR_MODES,
    DETAILS,
    stylesFor,
    WIDGET_STYLES,
    type ColorMode,
    type Detail,
    type GraphStyle,
    type WidgetStyle,
  } from '../../profile.ts';
  import { app, toast } from '../../state.svelte.ts';
  import { ITEM_KEYS, META, resolve } from '../../theme.ts';
  import CheckField from '../fields/CheckField.svelte';
  import ChoiceField from '../fields/ChoiceField.svelte';
  import SizedField from '../fields/SizedField.svelte';
  import ItemPicker from './ItemPicker.svelte';

  const DETAIL_OPTIONS: [string, string][] = DETAILS.map((d) => [d.id, d.name]);

  const item = $derived(app.profile.items[app.selected]);
  const meta = $derived(META[app.selected]);
  const dark = $derived(resolve(app.theme, false));

  function styleAll() {
    const style = item.style;
    for (const k of ITEM_KEYS)
      if (stylesFor(k).some((s) => s.id === style)) app.profile.items[k].style = style;
    toast('Every item that supports it uses this style');
  }
</script>

<section>
  <ItemPicker />
  <div class="show-head">
    <span></span>
    <span>Taskbar</span>
    <span>Widget</span>
  </div>
  <div class="show-one">
    <div class="show-row">
      <span class="show-name">
        <span class="dot" style:background={dark.items[app.selected]}></span>
        {meta.name}
      </span>
      {#each ['taskbar', 'widget'] as const as where (where)}
        <button
          type="button"
          class="box"
          role="checkbox"
          aria-checked={item[where]}
          aria-label="{meta.name} on the {where}"
          onclick={() => (item[where] = !item[where])}
        ></button>
      {/each}
    </div>
  </div>
  <p class="note">
    To change the order, press and drag an item on the taskbar or a card on the widget. A line shows
    where it will land.
  </p>
</section>
<section>
  <CheckField name="Show a graph" bind:checked={item.graph} />
  <ChoiceField
    name="Graph style"
    value={item.style}
    options={stylesFor(app.selected).map((s) => [s.id, s.name])}
    disabled={!item.graph}
    onchange={(v) => (item.style = v as GraphStyle)}
  />
  <button type="button" class="quiet left" onclick={styleAll}>Use this style for every item</button>
  <ChoiceField
    name="Color"
    value={item.colorMode}
    options={COLOR_MODES.map((m) => [m.id, m.name])}
    onchange={(v) => (item.colorMode = v as ColorMode)}
  />
  <CheckField name="Trend arrow against the recent average" bind:checked={item.trend} />
  <SizedField
    name="Tile width"
    autoName="Fit the text"
    bind:value={item.tileWidth}
    min={16}
    max={200}
    fallback={64}
  />
  <SizedField
    name="Tile height"
    autoName="Fill the taskbar"
    bind:value={item.tileHeight}
    min={16}
    max={44}
    fallback={32}
  />
  <ChoiceField
    name="Popup"
    value={item.popup ?? ''}
    options={[['', 'As set for all popups'], ...DETAIL_OPTIONS]}
    onchange={(v) => (item.popup = (v || null) as Detail | null)}
  />
  <ChoiceField
    name="Widget style"
    value={item.widgetStyle ?? ''}
    options={[
      ['', 'As set for the widget'],
      ...WIDGET_STYLES.map((s): [string, string] => [s.id, s.name]),
    ]}
    onchange={(v) => (item.widgetStyle = (v || null) as WidgetStyle | null)}
  />
  <ChoiceField
    name="Widget detail"
    value={item.widgetDetail ?? ''}
    options={[['', 'As set for the widget'], ...DETAIL_OPTIONS]}
    onchange={(v) => (item.widgetDetail = (v || null) as Detail | null)}
  />
</section>
