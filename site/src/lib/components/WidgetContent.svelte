<script lang="ts">
  import { rgba } from '../color.ts';
  import type { Detail, ItemSettings, Profile, WidgetStyle } from '../profile.ts';
  import { graph, type Row, type Sim } from '../sim.ts';
  import { readings } from '../state.svelte.ts';
  import { META, type ItemKey, type Resolved } from '../theme.ts';

  let {
    items,
    widget,
    settings,
    v,
    held = null,
  }: {
    items: ItemKey[];
    widget: Profile['widget'];
    settings: Record<ItemKey, ItemSettings>;
    v: Resolved;
    held?: ItemKey | null;
  } = $props();

  interface Card {
    key: ItemKey;
    name: string;
    text: string;
    down: string;
    up: string;
    spark: { area: string; line: string };
    tile: { area: string; line: string };
    now: number;
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

  const model = $derived.by(() => {
    const sim = readings();
    const groups: { style: WidgetStyle; cards: Card[] }[] = [];
    for (const k of items) {
      const style = settings[k].widgetStyle ?? widget.style;
      const detail = settings[k].widgetDetail ?? widget.detail;
      const net = sim.net();
      const extra = details(k, detail, sim);
      const card: Card = {
        key: k,
        name: META[k].name,
        text: sim.text(k),
        down: net.down,
        up: net.up,
        spark: graph(sim.history(k), 64, 12, 0),
        tile: graph(sim.history(k), 120, 30, 0.05),
        now: sim.level(k).now,
        details: extra.rows,
        cores: extra.cores,
      };
      const last = groups.at(-1);
      if (last && last.style === style) last.cards.push(card);
      else groups.push({ style, cards: [card] });
    }
    return { groups, cores: [...sim.cores()], pThreads: sim.pThreads };
  });
</script>

{#snippet value(card: Card)}
  {#if card.key === 'network'}
    <span style:color={v.items.network}>↓ {card.down}</span>
    &nbsp;
    <span style:color={v.up}>↑ {card.up}</span>
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

{#snippet ring(k: ItemKey, now: number, size: number)}
  {@const r = size / 2 - 6}
  {@const circ = 2 * Math.PI * r}
  <svg width={size} height={size} viewBox="0 0 {size} {size}" aria-hidden="true">
    <circle
      cx={size / 2}
      cy={size / 2}
      {r}
      fill="none"
      stroke={rgba(v.text, 0.12)}
      stroke-width="6"
    />
    <circle
      cx={size / 2}
      cy={size / 2}
      {r}
      fill="none"
      stroke={v.items[k]}
      stroke-width="6"
      stroke-linecap="round"
      stroke-dasharray="{(circ * now).toFixed(1)} {circ.toFixed(1)}"
      transform="rotate(-90 {size / 2} {size / 2})"
    />
  </svg>
{/snippet}

{#snippet row(card: Card, middle: 'spark' | 'meter' | 'none')}
  <div class="w-row">
    <span class="w-name">{card.name}</span>
    {#if middle === 'spark'}
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

{#if !items.length}
  <p class="w-empty" style:color={v.label}>No items on the widget</p>
{:else}
  {#each model.groups as group, gi (gi)}
    {#if group.style === 'tiles'}
      <div class="w-tiles">
        {#each group.cards as card (card.key)}
          <div
            class={['w-tile', card.key === 'network' && 'wide', held === card.key && 'held']}
            data-key={card.key}
            style:background={rgba(v.tile, Math.max(v.tileOpacity, 8) / 100)}
          >
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
          </div>
        {/each}
      </div>
    {:else if group.style === 'dials'}
      <div class="w-dials">
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
        <div class={['w-dial-net', held === card.key && 'held']} data-key="network">
          {@render ring('network', card.now, 70)}
          <div>
            <span class="w-name">Network</span>
            <span class="w-value">{@render value(card)}</span>
          </div>
        </div>
      {/each}
    {:else}
      {#each group.cards as card (card.key)}
        <div class={['w-item', held === card.key && 'held']} data-key={card.key}>
          {#if group.style === 'core_wall' && card.key === 'cpu'}
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
          {:else}
            {@render row(
              card,
              group.style === 'meters' && card.key !== 'network' ? 'meter' : 'spark',
            )}
          {/if}
          {@render more(card)}
        </div>
      {/each}
    {/if}
  {/each}
{/if}

{#snippet wall(list: number[])}
  <div class="w-wall">
    {#each list as val, i (i)}
      <span style:background={rgba(v.items.cpu, 0.15)}>
        <i style:height="{Math.max(3, val * 100).toFixed(0)}%" style:background={v.items.cpu}></i>
      </span>
    {/each}
  </div>
{/snippet}
