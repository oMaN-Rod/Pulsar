<script lang="ts">
  import type { Tile } from '../styles.ts';
  import Runs from './Runs.svelte';

  let { tile, room = [0, 0] }: { tile: Tile; room?: [number, number] } = $props();
  const stretched = $derived(room[0] > 0 || room[1] > 0);
</script>

<svg
  class={['tile-svg', stretched && 'stretched']}
  width={tile.w}
  height={tile.h}
  viewBox="0 0 {tile.w} {tile.h}"
  preserveAspectRatio={stretched ? 'none' : undefined}
  aria-hidden="true"
>
  {#each tile.shapes as s, i (i)}
    {#if s.tag === 'rect'}
      <rect {...s.attrs} />
    {:else if s.tag === 'circle'}
      <circle {...s.attrs} />
    {:else if s.tag === 'path'}
      <path {...s.attrs} />
    {:else}
      <polyline {...s.attrs} />
    {/if}
  {/each}
</svg>
{#if stretched}
  <span class="tile-text" style:left="{room[0]}px" style:right="{room[1]}px">
    {#each tile.spans as span, i (i)}
      <span class={['t', span.small && 'small']} style={span.style}><Runs runs={span.runs} /></span>
    {/each}
  </span>
{:else}
  {#each tile.spans as span, i (i)}
    <span class={['t', span.small && 'small']} style={span.style}><Runs runs={span.runs} /></span>
  {/each}
{/if}
