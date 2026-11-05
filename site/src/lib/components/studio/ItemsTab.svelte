<script lang="ts">
  import {
    COLOR_MODES,
    DETAILS,
    MAX_LABEL_CHARS,
    POPUP_GRAPH_HEIGHTS,
    POPUP_WIDTHS,
    cleanLabel,
    stylesFor,
    typedLabel,
    WIDGET_STYLES,
    type ColorMode,
    type Detail,
    type GraphStyle,
    type WidgetStyle,
  } from '../../profile.ts';
  import { app, toast } from '../../state.svelte.ts';
  import { ITEM_KEYS, META, resolve } from '../../theme.ts';
  import { keep } from '../../ui.ts';
  import CheckField from '../fields/CheckField.svelte';
  import ChoiceField from '../fields/ChoiceField.svelte';
  import OwnColorField from '../fields/OwnColorField.svelte';
  import SegField from '../fields/SegField.svelte';
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
        <span class="dot" style:background={item.color ?? dark.items[app.selected]}></span>
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
  <h2 class="eyebrow">Label</h2>
  {#each [['label', 'Label', meta.short], ['shortLabel', 'One-letter label', meta.letter]] as const as [key, name, fallback] (key)}
    <label class="field">
      <span class="field-name">{name}</span>
      <input
        type="text"
        class="text-input"
        spellcheck="false"
        placeholder={fallback}
        {@attach keep(item[key] ?? '')}
        oninput={(e) => (item[key] = typedLabel(e.currentTarget.value) || null)}
        onchange={(e) => (item[key] = cleanLabel(e.currentTarget.value))}
      />
    </label>
  {/each}
  <p class="note">On the taskbar only, up to {MAX_LABEL_CHARS} characters. Empty keeps Pulsar's.</p>
</section>
<section>
  {#if app.selected === 'disk'}
    <SegField
      name="Each drive shows"
      value={app.profile.disk.value}
      options={[
        ['activity', 'Activity'],
        ['used', 'Space used'],
      ]}
      onchange={(v) => (app.profile.disk.value = v)}
    />
    <p class="note">For drives with cells of their own, chosen in Pulsar's settings.</p>
  {/if}
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
  <OwnColorField
    name="Item color"
    themeName="The theme's color"
    hint="On the taskbar, widget and popup"
    bind:value={item.color}
    fallback={dark.items[app.selected]}
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
  <SizedField
    name="Popup width"
    autoName="As set for all popups"
    bind:value={item.popupWidth}
    min={POPUP_WIDTHS[0]}
    max={POPUP_WIDTHS[1]}
    fallback={app.profile.popup.width}
  />
  <SizedField
    name="Popup graph height"
    autoName="As set for all popups"
    bind:value={item.popupGraphHeight}
    min={POPUP_GRAPH_HEIGHTS[0]}
    max={POPUP_GRAPH_HEIGHTS[1]}
    fallback={app.profile.popup.graphHeight}
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
