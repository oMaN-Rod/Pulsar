<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { rgba } from '../color.ts';
  import type { Detail, ItemSettings, Profile, WidgetStyle } from '../profile.ts';
  import { graph, rate, type Row, type Sim } from '../sim.ts';
  import { drawn } from '../state.svelte.ts';
  import { META, type ItemKey, type Resolved } from '../theme.ts';
  import { rowRadius, type Arrangement, type Skin } from '../widget.ts';

  let {
    items,
    widget,
    settings,
    v,
    arrangement,
    skin,
    held = null,
  }: {
    items: ItemKey[];
    widget: Profile['widget'];
    settings: Record<ItemKey, ItemSettings>;
    v: Resolved;
    arrangement: Arrangement;
    skin: Skin;
    held?: ItemKey | null;
  } = $props();

  const DEVICES: Partial<Record<ItemKey, string>> = {
    cpu: 'Core i7-13700K',
    gpu: 'GeForce RTX 3070',
    gpu_memory: 'GeForce RTX 3070',
  };
  const TEMPS: Partial<Record<ItemKey, ItemKey>> = { cpu: 'cpu_temp', gpu: 'gpu_temp' };

  interface Card {
    key: ItemKey;
    name: string;
    text: string;
    big: string;
    unit: string;
    device: string | null;
    temp: string | null;
    down: string;
    up: string;
    upNow: number;
    hist: number[];
    upHist: number[];
    spark: { area: string; line: string };
    tile: { area: string; line: string };
    now: number;
    low: string;
    high: string;
    details: Row[];
    cores: number[] | null;
  }

  function details(k: ItemKey, detail: Detail, sim: Sim) {
    if (detail === 'simple') return { rows: [], cores: null };
    const d = sim.details(k);
    let list = [...d.rows];
    if (detail === 'detailed') {
      list = list.concat(d.more ?? []);
      for (const s of d.sections ?? []) list = list.concat(s.rows);
    } else list = list.slice(0, 4);
    return {
      rows: list,
      cores: list.length && d.cores && detail === 'detailed' ? [...sim.cores()] : null,
    };
  }

  /** `text` as a large number and its unit. */
  function split(text: string): [string, string] {
    const m = /^([-\d.,]+)\s*(.*)$/.exec(text);
    return m ? [m[1], m[2]] : [text, ''];
  }

  function extremes(k: ItemKey, sim: Sim, unit: string): [string, string] {
    const raw = sim.rawHistory(k);
    if (!raw.length) return ['', ''];
    const lo = Math.min(...raw);
    const hi = Math.max(...raw);
    if (k === 'network') return [rate(lo), rate(hi)];
    const digits = Math.abs(hi) < 10 && !Number.isInteger(hi) ? 1 : 0;
    return [`${lo.toFixed(digits)}${unit}`, `${hi.toFixed(digits)}${unit}`];
  }

  const model = $derived.by(() => {
    const sim = drawn();
    const net = sim.net();
    const upHist = sim.upHistory();
    const cards = items.map((k): Card & { style: WidgetStyle; detail: Detail } => {
      const style = settings[k].widgetStyle ?? widget.style;
      const detail = settings[k].widgetDetail ?? widget.detail;
      const text = k === 'network' ? net.down : sim.text(k);
      const [big, unit] = split(text);
      const [low, high] = extremes(k, sim, unit.startsWith('%') ? '%' : unit ? ` ${unit}` : '');
      const extra = details(k, detail, sim);
      const temp = TEMPS[k];
      return {
        key: k,
        style,
        detail,
        name: META[k].name,
        text: sim.text(k),
        big,
        unit,
        device: DEVICES[k] ?? null,
        temp: temp ? sim.text(temp) : null,
        down: net.down,
        up: net.up,
        upNow: upHist.at(-1) ?? 0,
        hist: sim.history(k),
        upHist,
        spark: graph(sim.history(k), 64, 12, 0),
        tile: graph(sim.history(k), 120, 30, 0.05),
        now: sim.level(k).now,
        low,
        high,
        details: extra.rows,
        cores: extra.cores,
      };
    });
    return { cards, cores: [...sim.cores()], pThreads: sim.pThreads };
  });

  const forced = $derived(arrangement.kind === 'forced' ? arrangement.style : null);

  const groups = $derived.by(() => {
    const list: { style: WidgetStyle; cards: Card[] }[] = [];
    for (const card of model.cards) {
      const style = forced ?? card.style;
      const last = list.at(-1);
      if (last && last.style === style) last.cards.push(card);
      else list.push({ style, cards: [card] });
    }
    return list;
  });

  let focusIndex = $state(0);
  const focusCard = $derived(model.cards[focusIndex % Math.max(1, model.cards.length)]);

  $effect(() => {
    if (arrangement.kind !== 'focus' || !widget.focus.cycle || model.cards.length < 2) return;
    const timer = setInterval(() => focusIndex++, widget.focus.intervalS * 1000);
    return () => clearInterval(timer);
  });

  function stepFocus(e: KeyboardEvent) {
    const step = ({ ArrowLeft: -1, ArrowRight: 1, Enter: 1, ' ': 1 } as Record<string, number>)[
      e.key
    ];
    if (!step) return;
    e.preventDefault();
    focusIndex = (focusIndex + step + model.cards.length) % model.cards.length;
  }

  function enter(node: Element) {
    const t = widget.focus.transition;
    const duration = t === 'none' ? 0 : widget.focus.transitionMs;
    if (t === 'slide') return fly(node, { x: 40, duration });
    if (t === 'slide_up') return fly(node, { y: 30, duration });
    return fade(node, { duration });
  }

  const panelStyle = $derived(
    `background:${skin.fill};border-radius:${skin.radius}px;` +
      (widget.border ? `outline:1px solid ${skin.border};outline-offset:-1px;` : ''),
  );
  const rowStyle = $derived.by(() => {
    const h = widget.sizing === 'chosen' ? widget.size[1] : arrangement.kind === 'rings' ? 60 : 52;
    return `${panelStyle}border-radius:${rowRadius(widget.rowRadius, h)}px;`;
  });
  const separate = $derived(
    widget.separate && arrangement.kind !== 'strip' && arrangement.kind !== 'rings',
  );
</script>

{#snippet value(card: Card)}
  {#if card.key === 'network'}
    {#if widget.networkInline}
      <span style:color={v.items.network}>↓ {card.down}</span>
      &nbsp;
      <span style:color={v.up}>↑ {card.up}</span>
    {:else}
      <span class="w-stack">
        <span style:color={v.items.network}>↓ {card.down}</span>
        <span style:color={v.up}>↑ {card.up}</span>
      </span>
    {/if}
  {:else}
    <span style:color={v.items[card.key]}>{card.text}</span>
  {/if}
{/snippet}

{#snippet more(card: Card)}
  {#if card.details.length}
    {#if card.cores}
      <div class="w-segs">
        {#each card.cores as val, i (i)}
          <i style:background={val > 0.15 ? v.items[card.key] : rgba(v.items[card.key], 0.25)}></i>
        {/each}
      </div>
    {/if}
    <div class="w-grid">
      {#each card.details as [n, val], i (i)}
        <span style:color={v.label}>{n}</span>
        <span>{val}</span>
      {/each}
    </div>
  {/if}
{/snippet}

{#snippet ring(k: ItemKey, now: number, size: number, stroke = 6)}
  {@const r = size / 2 - stroke}
  {@const circ = 2 * Math.PI * r}
  <svg width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true">
    <circle
      cx={size / 2}
      cy={size / 2}
      {r}
      fill="none"
      stroke={rgba(v.text, 0.12)}
      stroke-width={stroke}
    />
    <circle
      cx={size / 2}
      cy={size / 2}
      {r}
      fill="none"
      stroke={v.items[k]}
      stroke-width={stroke}
      stroke-linecap="round"
      stroke-dasharray="{(circ * now).toFixed(1)} {circ.toFixed(1)}"
      transform="rotate(-90 {size / 2} {size / 2})"
    />
  </svg>
{/snippet}

{#snippet chart(card: Card, w: number, h: number, grid = false)}
  {@const down = graph(card.hist, w, h, 0.05)}
  {@const up = card.key === 'network' ? graph(card.upHist, w, h, 0.05) : null}
  <svg class="w-chart" viewBox="0 0 {w} {h}" preserveAspectRatio="none" aria-hidden="true">
    {#if grid}
      {#each [0.25, 0.5, 0.75] as y (y)}
        <line
          x1="0"
          x2={w}
          y1={h * y}
          y2={h * y}
          stroke={rgba(v.text, 0.08)}
          vector-effect="non-scaling-stroke"
        />
      {/each}
    {/if}
    <path d={down.area} fill={rgba(v.items[card.key], 0.3)} />
    <polyline
      points={down.line}
      fill="none"
      stroke={v.items[card.key]}
      stroke-width="1.4"
      vector-effect="non-scaling-stroke"
    />
    {#if up}
      <path d={up.area} fill={rgba(v.up, 0.3)} />
      <polyline
        points={up.line}
        fill="none"
        stroke={v.up}
        stroke-width="1.4"
        vector-effect="non-scaling-stroke"
      />
    {/if}
  </svg>
{/snippet}

{#snippet head(card: Card)}
  <div class="w-card-head">
    <span class="w-card-name">{card.name}</span>
    {#if card.device}<small style:color={v.label}>{card.device}</small>{/if}
    {#if card.temp}<span class="w-card-temp">{card.temp}</span>{/if}
  </div>
{/snippet}

{#snippet bigValue(card: Card, size: number)}
  {#if card.key === 'network'}
    <div class="w-card-rates">
      {#each [['↓', card.down, v.items.network], ['↑', card.up, v.up]] as [arrow, text, color] (arrow)}
        {@const [n, u] = split(text)}
        <span>
          <b style:color>{arrow}</b>
          <span class="w-big" style:font-size="{size * 0.55}px">{n}</span>
          <small style:color={v.label}>{u}</small>
        </span>
      {/each}
    </div>
  {:else}
    <span class="w-big-line">
      <span class="w-big" style:font-size="{size}px">{card.big}</span>
      <small style:color={v.label}>{card.unit}</small>
    </span>
  {/if}
{/snippet}

{#snippet deviceCard(card: Card)}
  <div class="w-card">
    {@render head(card)}
    <div class="w-card-body">
      {@render bigValue(card, 30)}
      {@render chart(card, 160, 40)}
    </div>
    {@render more(card)}
  </div>
{/snippet}

{#snippet row(card: Card, middle: 'spark' | 'meter' | 'none')}
  <div class="w-row">
    <span class="w-name">{card.name}</span>
    {#if middle === 'meter' && card.key === 'network'}
      <span class="w-meters">
        {#each [[card.now, v.items.network], [card.upNow, v.up]] as [now, color], i (i)}
          <span class="w-meter" style:background={rgba(v.text, 0.12)}>
            <i style:width="{(Number(now) * 100).toFixed(0)}%" style:background={color}></i>
          </span>
        {/each}
      </span>
    {:else if middle === 'spark'}
      <svg
        class="w-spark"
        height="12"
        viewBox="0 0 64 12"
        preserveAspectRatio="none"
        aria-hidden="true"
      >
        <path d={card.spark.area} fill={rgba(v.items[card.key], 0.25)} />
        <polyline
          points={card.spark.line}
          fill="none"
          stroke={v.items[card.key]}
          stroke-width="1.2"
          vector-effect="non-scaling-stroke"
        />
      </svg>
    {:else if middle === 'meter'}
      <span class="w-meter" style:background={rgba(v.text, 0.12)}>
        <i style:width="{(card.now * 100).toFixed(0)}%" style:background={v.items[card.key]}></i>
      </span>
    {:else}
      <span></span>
    {/if}
    <span class="w-value">{@render value(card)}</span>
  </div>
{/snippet}

{#snippet wall(list: number[])}
  <div class="w-wall">
    {#each list as val, i (i)}
      <span style:background={rgba(v.items.cpu, 0.15)}>
        <i style:height="{Math.max(3, val * 100).toFixed(0)}%" style:background={v.items.cpu}></i>
      </span>
    {/each}
  </div>
{/snippet}

{#snippet lane(card: Card, style: WidgetStyle)}
  {#if style === 'cards'}
    {@render deviceCard(card)}
  {:else if style === 'core_wall' && card.key === 'cpu'}
    {@render row(card, 'none')}
    <div class="w-walls">
      <div>
        {@render wall(model.cores.slice(0, model.pThreads))}
        <small style:color={v.label}>P-cores, {model.pThreads} threads</small>
      </div>
      <div>
        {@render wall(model.cores.slice(model.pThreads))}
        <small style:color={v.label}>
          E-cores, {model.cores.length - model.pThreads} threads
        </small>
      </div>
    </div>
    {@render more(card)}
  {:else}
    {@render row(card, style === 'meters' ? 'meter' : 'spark')}
    {@render more(card)}
  {/if}
{/snippet}

{#snippet tile(card: Card)}
  <span class="w-name">{card.name}</span>
  <span class="w-value">{@render value(card)}</span>
  <svg viewBox="0 0 120 30" preserveAspectRatio="none" aria-hidden="true">
    <path d={card.tile.area} fill={rgba(v.items[card.key], 0.3)} />
    <polyline
      points={card.tile.line}
      fill="none"
      stroke={v.items[card.key]}
      stroke-width="1.2"
      vector-effect="non-scaling-stroke"
    />
  </svg>
  {@render more(card)}
{/snippet}

{#if !items.length}
  <div class="w-panel" style={panelStyle}>
    <p class="w-empty" style:color={v.label}>No items on the widget</p>
  </div>
{:else if arrangement.kind === 'strip'}
  <div class="w-strip w-panel" style={rowStyle}>
    {#each model.cards as card (card.key)}
      <span class={['w-strip-item', held === card.key && 'held']} data-key={card.key}>
        <i style:background={v.items[card.key]}></i>
        <span>{card.name}</span>
        {#if card.key === 'network'}
          <span class="w-stack small">
            <b>↓ {card.down}</b>
            <b>↑ {card.up}</b>
          </span>
        {:else}
          <b>{card.text}</b>
        {/if}
      </span>
    {/each}
  </div>
{:else if arrangement.kind === 'rings'}
  <div class="w-rings w-panel" style={rowStyle}>
    {#each model.cards as card (card.key)}
      <span class={['w-ring', held === card.key && 'held']} data-key={card.key} title={card.name}>
        {@render ring(card.key, card.now, 44, 4)}
        <span>{card.big}</span>
      </span>
    {/each}
  </div>
{:else if arrangement.kind === 'focus'}
  <div
    class="w-focus w-panel"
    style={panelStyle}
    role="button"
    aria-label="Show the next item"
    tabindex="0"
    onclick={() => focusIndex++}
    onkeydown={stepFocus}
  >
    {#key focusCard.key}
      <div class="w-focus-card" data-key={focusCard.key} in:enter>
        {@render head(focusCard)}
        {@render bigValue(focusCard, 52)}
        {@render chart(focusCard, 260, 90, true)}
        <div class="w-focus-foot" style:color={v.label}>
          Min <b style:color={v.text}>{focusCard.low}</b>
          Max
          <b style:color={v.text}>{focusCard.high}</b>
        </div>
      </div>
    {/key}
  </div>
{:else if arrangement.kind === 'dashboard'}
  <div
    class={['w-dash', separate ? 'separate' : 'w-panel']}
    style={separate ? null : panelStyle}
    style:--cols={arrangement.columns}
    style:--rows={Math.ceil(model.cards.length / arrangement.columns)}
  >
    {#each model.cards as card, i (card.key)}
      <div
        class={[
          'w-dash-cell',
          i % Math.ceil(model.cards.length / arrangement.columns) === 0 && 'top',
          separate && 'w-panel',
          held === card.key && 'held',
        ]}
        style={separate ? panelStyle : null}
        data-key={card.key}
      >
        {@render deviceCard(card)}
      </div>
    {/each}
  </div>
{:else}
  <div
    class={['w-stackpanel', separate ? 'separate' : 'w-panel']}
    style={separate ? null : panelStyle}
  >
    {#each groups as group, gi (gi)}
      {#if group.style === 'tiles'}
        <div class="w-tiles">
          {#each group.cards as card (card.key)}
            <div
              class={['w-tile', card.key === 'network' && 'wide', held === card.key && 'held']}
              data-key={card.key}
              style={separate ? panelStyle : null}
              style:background={separate
                ? skin.fill
                : rgba(v.tile, Math.max(v.tileOpacity, 8) / 100)}
            >
              {@render tile(card)}
            </div>
          {/each}
        </div>
      {:else if group.style === 'dials'}
        <div class={['w-dials', separate && 'w-panel']} style={separate ? panelStyle : null}>
          {#each group.cards.filter((c) => c.key !== 'network') as card (card.key)}
            <div class={['w-dial', held === card.key && 'held']} data-key={card.key}>
              <div class="w-ring">
                {@render ring(card.key, card.now, 62)}
                <span>{card.text}</span>
              </div>
              <span class="w-name small">{card.name}</span>
            </div>
          {/each}
        </div>
        {#each group.cards.filter((c) => c.key === 'network').slice(0, 1) as card (card.key)}
          <div
            class={['w-dial-net', separate && 'w-panel', held === card.key && 'held']}
            style={separate ? panelStyle : null}
            data-key="network"
          >
            {@render ring('network', card.now, 70)}
            <div>
              <span class="w-name">Network</span>
              <span class="w-value">{@render value(card)}</span>
            </div>
          </div>
        {/each}
      {:else}
        {#each group.cards as card (card.key)}
          <div
            class={['w-item', separate && 'w-panel', held === card.key && 'held']}
            style={separate ? panelStyle : null}
            data-key={card.key}
          >
            {@render lane(card, group.style)}
          </div>
        {/each}
      {/if}
    {/each}
  </div>
{/if}
