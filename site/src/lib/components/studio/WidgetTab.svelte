<script lang="ts">
  import {
    DETAILS,
    FOCUS_INTERVALS_S,
    FOCUS_TRANSITIONS_MS,
    HIDE_DELAYS_MS,
    ROW_RADII,
    WIDGET_CHOSEN_HEIGHTS,
    WIDGET_CHOSEN_WIDTHS,
    WIDGET_LAYOUTS,
    WIDGET_MATERIALS,
    WIDGET_STYLES,
    WIDGET_WIDTHS,
    type Detail,
    type OverFullscreen,
    type WidgetLayout,
    type WidgetStyle,
  } from '../../profile.ts';
  import { app } from '../../state.svelte.ts';
  import { resolve } from '../../theme.ts';
  import { keep } from '../../ui.ts';
  import { heightRange } from '../../widget.ts';
  import CheckField from '../fields/CheckField.svelte';
  import ChoiceField from '../fields/ChoiceField.svelte';
  import RangeField from '../fields/RangeField.svelte';
  import SegField from '../fields/SegField.svelte';

  const DETAIL_OPTIONS: [Detail, string][] = DETAILS.map((d) => [d.id, d.name]);
  const w = $derived(app.profile.widget);
  const shown = $derived(resolve(app.theme, app.light));
  const bg = $derived(w.background ?? shown.panel);
  const placement = $derived(w.dock.edge ? 'docked' : w.layer);
  const row = $derived(w.layout === 'strip' || w.layout === 'rings');
  const heights = $derived(heightRange(w.layout) ?? WIDGET_CHOSEN_HEIGHTS);

  function place(v: 'desktop' | 'on_top' | 'docked') {
    if (v === 'docked') w.dock.edge ??= 'right';
    else {
      w.dock.edge = null;
      w.layer = v;
    }
  }

  function chooseLayout(v: WidgetLayout) {
    w.layout = v;
    const range = heightRange(v);
    if (range) w.size[1] = Math.max(range[0], Math.min(range[1], w.size[1]));
  }
</script>

<CheckField name="Show the desktop widget" bind:checked={w.enabled} />
<section>
  <h2 class="eyebrow">Placement</h2>
  <SegField
    name="Placement"
    value={placement}
    options={[
      ['desktop', 'On the desktop'],
      ['on_top', 'Always on top'],
      ['docked', 'Docked'],
    ]}
    onchange={place}
  />
  {#if w.dock.edge}
    <SegField
      name="Edge"
      value={w.dock.edge}
      options={[
        ['left', 'Left'],
        ['top', 'Top'],
        ['right', 'Right'],
        ['bottom', 'Bottom'],
      ]}
      onchange={(v) => (w.dock.edge = v)}
    />
    <SegField
      name="Push to show"
      value={w.dock.push}
      options={[
        ['light', 'Light'],
        ['medium', 'Medium'],
        ['firm', 'Firm'],
      ]}
      onchange={(v) => (w.dock.push = v)}
    />
    <RangeField
      name="Hide after leaving"
      min={HIDE_DELAYS_MS[0]}
      max={HIDE_DELAYS_MS[1]}
      step={50}
      value={w.dock.hideDelayMs}
      output="{w.dock.hideDelayMs} ms"
      oninput={(v) => (w.dock.hideDelayMs = v)}
    />
    <CheckField name="Show an edge line" bind:checked={w.dock.lip} />
    <ChoiceField
      name="Over fullscreen apps"
      value={w.dock.overFullscreen}
      options={[
        ['never', 'Never'],
        ['hotkey', 'Shortcut only'],
        ['hotkey_and_edge', 'Shortcut and edge'],
      ]}
      onchange={(v) => (w.dock.overFullscreen = v as OverFullscreen)}
    />
    <p class="note">
      Hidden at the screen edge until you push the pointer against it. Try it on the desktop here;
      Pulsar picks the monitor in its own settings.
    </p>
  {:else}
    <p class="note">On the desktop stays behind your windows.</p>
  {/if}
</section>
<section>
  <h2 class="eyebrow">Look</h2>
  <SegField
    name="Size"
    value={w.sizing}
    options={[
      ['content', 'Fit the content'],
      ['chosen', 'Drag to size'],
    ]}
    onchange={(v) => (w.sizing = v)}
  />
  {#if w.sizing === 'chosen'}
    <RangeField
      name="Width"
      min={WIDGET_CHOSEN_WIDTHS[0]}
      max={Math.min(1200, WIDGET_CHOSEN_WIDTHS[1])}
      value={w.size[0]}
      output="{w.size[0]} px"
      oninput={(v) => (w.size[0] = v)}
    />
    <RangeField
      name="Height"
      min={heights[0]}
      max={Math.min(900, heights[1])}
      value={w.size[1]}
      output="{w.size[1]} px"
      disabled={heights[0] === heights[1]}
      oninput={(v) => (w.size[1] = v)}
    />
    <p class="note">Or drag the widget's edges. Strip and Rings keep to a single row.</p>
  {/if}
  <ChoiceField
    name="Layout"
    value={w.layout}
    options={WIDGET_LAYOUTS.map((l) => [l.id, l.name])}
    onchange={(v) => chooseLayout(v as WidgetLayout)}
  />
  <p class="note">
    {w.sizing === 'chosen'
      ? 'Automatic picks one from the size.'
      : 'Automatic shows each item in the style chosen for it.'}
    Focus shows one item at a time, with a large graph.
  </p>
  {#if w.layout === 'focus'}
    <CheckField name="Cycle through items" bind:checked={w.focus.cycle} />
    <RangeField
      name="Interval"
      min={FOCUS_INTERVALS_S[0]}
      max={FOCUS_INTERVALS_S[1]}
      value={w.focus.intervalS}
      output="{w.focus.intervalS} s"
      disabled={!w.focus.cycle}
      oninput={(v) => (w.focus.intervalS = v)}
    />
    <SegField
      name="Transition"
      value={w.focus.transition}
      options={[
        ['none', 'None'],
        ['fade', 'Fade'],
        ['slide', 'Slide'],
        ['slide_up', 'Slide up'],
      ]}
      onchange={(v) => (w.focus.transition = v)}
    />
    <RangeField
      name="Transition length"
      min={FOCUS_TRANSITIONS_MS[0]}
      max={FOCUS_TRANSITIONS_MS[1]}
      step={50}
      value={w.focus.transitionMs}
      output="{w.focus.transitionMs} ms"
      disabled={w.focus.transition === 'none'}
      oninput={(v) => (w.focus.transitionMs = v)}
    />
    <p class="note">In Pulsar, shortcuts can also step to the next and previous item.</p>
  {/if}
  {#if row}
    <label class="check">
      <input
        type="checkbox"
        checked={w.rowRadius !== null}
        onchange={(e) => (w.rowRadius = e.currentTarget.checked ? 12 : null)}
      />
      <span>Custom corner radius</span>
    </label>
    <RangeField
      name="Corner radius"
      min={ROW_RADII[0]}
      max={ROW_RADII[1]}
      value={w.rowRadius ?? 12}
      output={w.rowRadius === null ? 'Fully round' : `${w.rowRadius} px`}
      disabled={w.rowRadius === null}
      oninput={(v) => (w.rowRadius = v)}
    />
  {/if}
  <ChoiceField
    name="Style"
    value={w.style}
    options={WIDGET_STYLES.map((s) => [s.id, s.name])}
    disabled={w.sizing !== 'content' || w.layout !== 'auto'}
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
    min={WIDGET_WIDTHS[0]}
    max={WIDGET_WIDTHS[1]}
    value={w.width}
    output="{w.width} px"
    disabled={w.sizing !== 'content' || row}
    oninput={(v) => (w.width = v)}
  />
</section>
<section>
  <h2 class="eyebrow">Material</h2>
  <SegField
    name="Material"
    value={w.material}
    options={WIDGET_MATERIALS.map((m) => [m.id, m.name])}
    onchange={(v) => (w.material = v)}
  />
  <p class="note">
    Glass, Liquid glass and Acrylic blur what is behind the widget. Liquid glass keeps more of it in
    focus. Clear never blurs, for use over games.
  </p>
  {#if w.material === 'glass' || w.material === 'liquid'}
    <RangeField
      name="Glass tint"
      min={0}
      max={100}
      value={w.glassTint}
      output={w.glassTint ? `${w.glassTint}%` : 'Clear glass'}
      oninput={(v) => (w.glassTint = v)}
    />
  {/if}
  <CheckField
    name="Separate cards"
    bind:checked={w.separate}
    disabled={row || w.layout === 'focus'}
  />
  <CheckField name="Border" bind:checked={w.border} />
  <CheckField name="Network rates on one line" bind:checked={w.networkInline} />
</section>
<section>
  <h2 class="eyebrow">Background</h2>
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
    disabled={w.material === 'glass' || w.material === 'liquid'}
    oninput={(v) => (w.opacity = v)}
  />
</section>
<p class="note">
  Choose what the widget shows under Items, where each item can also have its own style and detail.
</p>
