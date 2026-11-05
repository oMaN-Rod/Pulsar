<script lang="ts">
  import type { Tile } from '../styles.ts';
  import Runs from './Runs.svelte';

  let { tile }: { tile: Tile } = $props();
</script>

<svg
  class="tile-svg"
  width={tile.w}
  height={tile.h}
  viewBox="0 0 {tile.w} {tile.h}"
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
{#each tile.spans as span, i (i)}
  <span class={['t', span.small && 'small']} style={span.style}><Runs runs={span.runs} /></span>
{/each}
