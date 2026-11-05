<script lang="ts">
  import { contrast, fromHsl, inkOn, isHex, mix, toHsl } from '../../color.ts';
  import type { LoadLevel } from '../../profile.ts';
  import { app, editTheme } from '../../state.svelte.ts';
  import { loadColorBand } from '../../styles.ts';
  import { META, resolve } from '../../theme.ts';
  import { keep } from '../../ui.ts';
  import CheckField from '../fields/CheckField.svelte';
  import RangeField from '../fields/RangeField.svelte';
  import ItemPicker from './ItemPicker.svelte';

  const SURFACES = [
    ['text', 'Values', 'Numbers on tiles and popups'],
    ['label', 'Labels', 'Item names'],
    ['tile', 'Tile', 'Fill behind each item'],
    ['panel', 'Panel', 'Behind the overlay, widget and popups'],
  ] as const;

  const LOADS: [LoadLevel, string][] = [
    ['low', 'Low'],
    ['moderate', 'Moderate'],
    ['high', 'High'],
    ['critical', 'Critical'],
  ];

  const dark = $derived(resolve(app.theme, false));
  const shown = $derived(resolve(app.theme, app.light));
  const color = $derived(app.theme.dark.items[app.selected] ?? dark.items[app.selected]);
  const hsl = $derived(toHsl(color));
  const rails = $derived([
    Math.round(hsl[0]),
    Math.round(hsl[1]),
    Math.round(Math.max(20, Math.min(92, hsl[2]))),
  ]);
  const ratio = $derived.by(() => {
    const bar = app.light ? '#EEEEEE' : '#1C1C1C';
    const tileBg = mix(
      mix(bar, shown.panel, shown.panelOpacity / 100),
      shown.tile,
      shown.tileOpacity / 100,
    );
    return `${contrast(shown.text, tileBg).toFixed(1)}:1 values · ${contrast(shown.items[app.selected], tileBg).toFixed(1)}:1 graph`;
  });

  const setItem = (c: string) => editTheme((t) => (t.dark.items[app.selected] = c));

  function typedHex(value: string) {
    const v = value.trim();
    const full = v.startsWith('#') ? v : `#${v}`;
    if (isHex(full)) setItem(full.toUpperCase());
  }
</script>

<section class="pick">
  <ItemPicker />
  <div class="swatch" style:background={color} style:color={inkOn(color)}>
    <span class="swatch-name">{META[app.selected].name}</span>
    <div class="swatch-foot">
      <label class="sr-only" for="hex">Color</label>
      <input
        id="hex"
        class="swatch-hex"
        type="text"
        spellcheck="false"
        maxlength="7"
        autocomplete="off"
        {@attach keep(color)}
        oninput={(e) => typedHex(e.currentTarget.value)}
      />
      <span class="swatch-ratio">{ratio}</span>
    </div>
  </div>
  <RangeField
    name="Hue"
    class="rail-hue"
    min={0}
    max={359}
    value={rails[0]}
    output="{rails[0]}°"
    oninput={(h) => setItem(fromHsl(h, rails[1], rails[2]))}
  />
  <RangeField
    name="Saturation"
    class="rail-sat"
    min={0}
    max={100}
    value={rails[1]}
    output="{Math.round(hsl[1])}%"
    rail="linear-gradient(90deg, {fromHsl(hsl[0], 0, hsl[2])}, {fromHsl(hsl[0], 100, hsl[2])})"
    oninput={(s) => setItem(fromHsl(rails[0], s, rails[2]))}
  />
  <RangeField
    name="Lightness"
    class="rail-light"
    min={20}
    max={92}
    value={rails[2]}
    output="{Math.round(hsl[2])}%"
    rail="linear-gradient(90deg, {fromHsl(hsl[0], hsl[1], 20)}, {fromHsl(
      hsl[0],
      hsl[1],
      56,
    )}, {fromHsl(hsl[0], hsl[1], 92)})"
    oninput={(l) => setItem(fromHsl(rails[0], rails[1], l))}
  />
</section>
<section>
  <h2 class="eyebrow">Surface</h2>
  <div class="surfaces">
    {#each SURFACES as [k, name, hint] (k)}
      <label class="surface">
        <input
          type="color"
          {@attach keep(dark[k].toLowerCase())}
          oninput={(e) => editTheme((t) => (t.dark[k] = e.currentTarget.value.toUpperCase()))}
        />
        <span class="surface-text">
          <span>{name}</span>
          <small>{hint}</small>
        </span>
        <code>{k === 'label' && dark.colorLabels ? 'by item' : dark[k]}</code>
      </label>
    {/each}
  </div>
  <RangeField
    name="Tile opacity"
    class="rail-tile"
    min={0}
    max={100}
    value={dark.tileOpacity}
    output="{dark.tileOpacity}%"
    oninput={(n) => editTheme((t) => (t.dark.tileOpacity = n))}
  />
  <RangeField
    name="Panel opacity"
    class="rail-panel"
    min={0}
    max={100}
    value={dark.panelOpacity}
    output={dark.panelOpacity ? `${dark.panelOpacity}%` : 'none'}
    oninput={(n) => editTheme((t) => (t.dark.panelOpacity = n))}
  />
  <p class="note panel-rule" hidden={!!app.theme.light}>
    {dark.panelOpacity >= 50
      ? 'From 50%, these colors also show as they are on a light taskbar, dark panel and all.'
      : 'Below 50%, a light taskbar gets light colors that Pulsar derives from these. From 50%, these show as they are.'}
  </p>
  <label class="check">
    <input
      type="checkbox"
      class="color-labels"
      checked={dark.colorLabels}
      onchange={(e) => editTheme((t) => (t.colorLabels = e.currentTarget.checked))}
    />
    <span>Color labels by item</span>
  </label>
  <CheckField
    name="Draw graphs in the Windows accent color"
    bind:checked={app.profile.accentGraphs}
  />
</section>
<section>
  <h2 class="eyebrow">Load colors</h2>
  <p class="note">For items colored by load, and the tint, bubble and heat styles.</p>
  <div class="surfaces">
    {#each LOADS as [k, name], i (k)}
      {@const c = loadColorBand(i, app.light, app.profile.load)}
      <label class="surface">
        <input
          type="color"
          {@attach keep(c.toLowerCase())}
          oninput={(e) => (app.profile.load.colors[k] = e.currentTarget.value.toUpperCase())}
        />
        <span class="surface-text"><span>{name}</span></span>
        <code>{app.profile.load.colors[k] ? c : `${c} default`}</code>
      </label>
    {/each}
  </div>
  <button
    type="button"
    class="quiet left"
    onclick={() =>
      (Object.keys(app.profile.load.colors) as LoadLevel[]).forEach(
        (k) => (app.profile.load.colors[k] = null),
      )}
  >
    Use Pulsar's load colors
  </button>
</section>
