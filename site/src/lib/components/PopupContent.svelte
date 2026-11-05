<script lang="ts">
  import { rgba } from '../color.ts';
  import type { Detail, Profile } from '../profile.ts';
  import { graph, type Row } from '../sim.ts';
  import { readings } from '../state.svelte.ts';
  import { META, type ItemKey, type Resolved } from '../theme.ts';

  let {
    item,
    detail,
    v,
    settings,
  }: { item: ItemKey; detail: Detail; v: Resolved; settings: Profile['popup'] } = $props();

  const m = $derived.by(() => {
    const sim = readings();
    const gh = settings.graphHeight;
    const d = detail === 'simple' ? null : sim.details(item);
    return {
      gh,
      text: sim.text(item),
      net: sim.net(),
      g: graph(sim.history(item), 276, gh, 0.08),
      up: item === 'network' ? graph(sim.upHistory(), 276, gh, 0.08).line : null,
      d,
      cores: d?.cores ? [...sim.cores()] : [],
    };
  });
  const c = $derived(v.items[item]);
</script>

{#snippet rows(list: Row[])}
  {#each list as [n, val], i (i)}
    <div class="pp-row">
      <span style:color={v.label}>{n}</span>
      <span>{val}</span>
    </div>
  {/each}
{/snippet}

<div class="pp-head">
  <span class="pp-title">{META[item].name}</span>
  <span class="pp-value">
    {#if item === 'network'}
      <span style:color={c}>↓ {m.net.down}</span>
      <span style:color={v.up}>↑ {m.net.up}</span>
    {:else}
      <span style:color={c}>{m.text}</span>
    {/if}
  </span>
</div>
{#if settings.graph || detail === 'simple'}
  <svg
    class="pp-graph"
    viewBox="0 0 276 {m.gh}"
    preserveAspectRatio="none"
    style:height="{m.gh}px"
    style:background={rgba(v.text, 0.05)}
    aria-hidden="true"
  >
    <path d={m.g.area} fill={rgba(c, 0.3)} />
    <polyline
      points={m.g.line}
      fill="none"
      stroke={c}
      stroke-width="1.3"
      vector-effect="non-scaling-stroke"
    />
    {#if m.up !== null}
      <polyline
        points={m.up}
        fill="none"
        stroke={v.up}
        stroke-width="1.3"
        vector-effect="non-scaling-stroke"
      />
    {/if}
  </svg>
{/if}
{#if m.d}
  {@render rows(m.d.rows)}
  {#if detail === 'detailed' && m.d.more}
    {@render rows(m.d.more)}
  {/if}
  {#if m.d.cores}
    <div class="pp-section" style:color={v.label}>Cores</div>
    <div class="pp-cores">
      {#each m.cores as val, i (i)}
        <span style:background={rgba(c, 0.12)}>
          <i style:height="{Math.max(4, val * 100).toFixed(0)}%" style:background={c}></i>
        </span>
      {/each}
    </div>
  {/if}
  {#if detail === 'detailed'}
    {#each m.d.sections ?? [] as s, i (i)}
      <div class="pp-section" style:color={v.label}>{s.title}</div>
      {@render rows(s.rows)}
    {/each}
  {/if}
  {#if m.d.processes && settings.processes > 0}
    <div class="pp-section" style:color={v.label}>Top processes</div>
    {@render rows(m.d.processes.slice(0, settings.processes))}
  {/if}
{/if}
