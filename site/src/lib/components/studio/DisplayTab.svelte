<script lang="ts">
  import {
    cleanLabel,
    fontName,
    FONTS,
    LOAD_UNITS,
    MAX_BPS,
    TEXT_GAPS,
    typedLabel,
    thresholdsFor,
    type LoadUnit,
    type Profile,
  } from '../../profile.ts';
  import { app } from '../../state.svelte.ts';
  import { keep } from '../../ui.ts';
  import CheckField from '../fields/CheckField.svelte';
  import ChoiceField from '../fields/ChoiceField.svelte';
  import RangeField from '../fields/RangeField.svelte';
  import SegField from '../fields/SegField.svelte';

  const LEVELS = ['Moderate', 'High', 'Critical'];
  const FIT_STATE = {
    full: 'Everything fits.',
    shrunk: 'Tightened or trimmed to fit.',
    hide: 'Nothing fits, so Pulsar hides.',
    tray: 'Nothing fits, so Pulsar shows its first item in the tray.',
    float: 'Nothing fits, so Pulsar floats above the taskbar.',
  };

  const p = $derived(app.profile);
  const known = $derived(!p.font.family || FONTS.includes(p.font.family));
  let customPicked = $state(false);
  let fontInput = $state<HTMLInputElement>();
  const closed = $derived(app.taskbar.closed.length);

  function pickFont(value: string) {
    customPicked = value === 'custom';
    if (value !== 'custom') p.font.family = value === FONTS[0] ? '' : value;
    else queueMicrotask(() => fontInput?.focus());
  }

  function setThreshold(unit: LoadUnit, index: number, input: HTMLInputElement) {
    const n = Math.round(Number(input.value)) || 0;
    const list = [...p.load.thresholds[unit]];
    list[index] = n;
    p.load.thresholds[unit] = thresholdsFor(unit, list) ?? p.load.thresholds[unit];
    input.value = String(p.load.thresholds[unit][index]);
  }

  const rateUnit = $derived(
    p.units.network === 'bits' ? { name: 'kb/s', bytes: 125 } : { name: 'KB/s', bytes: 1024 },
  );

  function setFloor(dir: 'minDownBps' | 'minUpBps', input: HTMLInputElement) {
    const n = Math.max(0, Math.round(Number(input.value)) || 0);
    p.network[dir] = Math.min(MAX_BPS, n * rateUnit.bytes);
    input.value = String(Math.round(p.network[dir] / rateUnit.bytes));
  }

  function setLabels(value: Profile['labels']) {
    p.labels = value;
    if (value === 'none') p.icons = true;
  }
</script>

<SegField
  name="Overlay"
  value={p.mode}
  options={[
    ['graph', 'Graphs'],
    ['text', 'Text'],
  ]}
  onchange={(v) => (p.mode = v)}
/>
<section>
  <h2 class="eyebrow">Fit</h2>
  <CheckField name="Fit to the taskbar" bind:checked={p.fit.enabled} />
  <p class="note">
    When the items don't fit, Pulsar first tightens them, then drops them one at a time.
  </p>
  <SegField
    name="Drop items from the"
    value={p.fit.removeFrom}
    options={[
      ['left', 'Left'],
      ['right', 'Right'],
    ]}
    onchange={(v) => (p.fit.removeFrom = v)}
  />
  <SegField
    name="When nothing fits"
    value={p.fit.noRoom}
    options={[
      ['hide', 'Hide'],
      ['tray', 'Tray icon'],
      ['float', 'Float'],
    ]}
    onchange={(v) => (p.fit.noRoom = v)}
  />
  <CheckField
    name="On a centered taskbar, try the other side first"
    bind:checked={p.fit.otherSide}
  />
  <p class="fit-state note" aria-live="polite">
    {p.position === 'floating'
      ? 'A floating overlay keeps its full size.'
      : p.fit.enabled
        ? FIT_STATE[app.fit]
        : ''}
  </p>
</section>
<section>
  <h2 class="eyebrow">Taskbar</h2>
  <SegField
    name="Pulsar sits"
    value={p.position}
    options={[
      ['left', 'After the apps'],
      ['right', 'By the tray'],
      ['floating', 'Floating'],
    ]}
    onchange={(v) => (p.position = v)}
  />
  <CheckField name="Transparent taskbar" bind:checked={app.taskbar.transparent} />
  <button
    type="button"
    class="quiet left"
    hidden={!closed}
    onclick={() => {
      app.taskbar.closed = [];
    }}
  >
    Bring back closed apps ({closed})
  </button>
  <p class="note">
    Right-click an app on the taskbar, or hover it and use ×, to close it and make room.
  </p>
</section>
<section>
  <h2 class="eyebrow">Labels</h2>
  <SegField
    name="Label"
    value={p.labels}
    options={[
      ['full', 'Full'],
      ['short', 'One letter'],
      ['none', 'Icon only'],
    ]}
    onchange={setLabels}
  />
  <CheckField
    name="An icon before each label"
    bind:checked={p.icons}
    disabled={p.labels === 'none'}
  />
  <CheckField name="Label and value on one line" bind:checked={p.inline} />
  <CheckField name="Labels stacked one letter per line" bind:checked={p.verticalLabels} />
</section>
<section>
  <h2 class="eyebrow">Font</h2>
  <ChoiceField
    name="Font"
    value={!known || customPicked ? 'custom' : p.font.family || FONTS[0]}
    options={[...FONTS.map((f): [string, string] => [f, f]), ['custom', 'Another font…']]}
    onchange={pickFont}
  />
  <label class="field custom-font" hidden={known && !customPicked}>
    <span class="field-name">Font name</span>
    <input
      type="text"
      class="text-input"
      placeholder="As installed on Windows"
      spellcheck="false"
      bind:this={fontInput}
      {@attach keep(known ? '' : p.font.family)}
      oninput={(e) => (p.font.family = fontName(e.currentTarget.value.trim()))}
    />
  </label>
  <RangeField
    name="Size"
    min={7}
    max={14}
    step={0.5}
    value={p.font.size}
    output="{p.font.size} pt"
    oninput={(v) => (p.font.size = v)}
  />
  <CheckField name="Bold" bind:checked={p.font.bold} />
  <p class="note">Fonts show as they would in Pulsar when they're installed on this computer.</p>
</section>
<section>
  <h2 class="eyebrow">Text spacing</h2>
  {#each [['item', 'Between items', TEXT_GAPS.item], ['label', 'Label to value', TEXT_GAPS.label], ['row', 'Between rows', TEXT_GAPS.row]] as const as [k, name, [min, max]] (k)}
    <RangeField
      {name}
      {min}
      {max}
      value={p.textGaps[k]}
      output="{p.textGaps[k]} px"
      oninput={(v) => (p.textGaps[k] = v)}
    />
  {/each}
  <p class="note">
    For the Text overlay. On a vertical taskbar, items are three fifths as far apart.
  </p>
</section>
<section>
  <h2 class="eyebrow">Units</h2>
  <SegField
    name="Temperature"
    value={p.units.temperature}
    options={[
      ['celsius', '°C'],
      ['fahrenheit', '°F'],
    ]}
    onchange={(v) => (p.units.temperature = v)}
  />
  <SegField
    name="Network"
    value={p.units.network}
    options={[
      ['bytes', 'Bytes'],
      ['bits', 'Bits'],
    ]}
    onchange={(v) => (p.units.network = v)}
  />
  <SegField
    name="Memory"
    value={p.units.memory}
    options={[
      ['percent', 'Percent'],
      ['bytes', 'Amount'],
    ]}
    onchange={(v) => (p.units.memory = v)}
  />
</section>
<section>
  <h2 class="eyebrow">Network</h2>
  <SegField
    name="Show"
    value={p.network.show}
    options={[
      ['both', 'Both'],
      ['download', 'Download'],
      ['upload', 'Upload'],
    ]}
    onchange={(v) => (p.network.show = v)}
  />
  <CheckField name="Upload above download" bind:checked={p.network.uploadOnTop} />
  <CheckField
    name="A graph tile for each direction"
    bind:checked={p.network.splitTiles}
    disabled={p.mode !== 'graph' || p.network.show !== 'both'}
  />
  <CheckField name="A dot in the color of the connection" bind:checked={p.network.connectionDot} />
  <p class="note">
    Green with internet access, orange with the local network only, red with neither.
  </p>
  {#each [['downLabel', 'Download label', '↓'], ['upLabel', 'Upload label', '↑']] as const as [key, name, arrow] (key)}
    <label class="field">
      <span class="field-name">{name}</span>
      <input
        type="text"
        class="text-input"
        spellcheck="false"
        placeholder={arrow}
        {@attach keep(p.network[key] ?? '')}
        oninput={(e) => (p.network[key] = typedLabel(e.currentTarget.value) || null)}
        onchange={(e) => (p.network[key] = cleanLabel(e.currentTarget.value))}
      />
    </label>
  {/each}
  <SegField
    name="The popup shows"
    value={p.network.popupShow}
    options={[
      ['both', 'Both'],
      ['download', 'Download'],
      ['upload', 'Upload'],
    ]}
    onchange={(v) => (p.network.popupShow = v)}
  />
  <div class="thresholds floors">
    <span>Show as 0 below</span>
    {#each [['minDownBps', 'Download'], ['minUpBps', 'Upload']] as const as [key, name] (key)}
      <label>
        <span class="sr-only">{name} shown as 0 below</span>
        <input
          type="number"
          class="text-input"
          min="0"
          title={name}
          {@attach keep(Math.round(p.network[key] / rateUnit.bytes))}
          onchange={(e) => setFloor(key, e.currentTarget)}
        />
      </label>
    {/each}
    <small>{rateUnit.name}</small>
  </div>
  <p class="note">
    Download, then upload, on the taskbar and widget. Popups show the rate as measured.
  </p>
</section>
<section>
  <h2 class="eyebrow">Load thresholds</h2>
  <p class="note">Where moderate, high and critical begin.</p>
  {#each LOAD_UNITS as u (u.id)}
    <div class="thresholds">
      <span>{u.name}</span>
      {#each [0, 1, 2] as i (i)}
        <label>
          <span class="sr-only">{u.name} {LEVELS[i]}</span>
          <input
            type="number"
            class="text-input"
            min="0"
            max={u.max}
            {@attach keep(p.load.thresholds[u.id][i])}
            onchange={(e) => setThreshold(u.id, i, e.currentTarget)}
          />
        </label>
      {/each}
      <small>{u.suffix}</small>
    </div>
  {/each}
</section>
