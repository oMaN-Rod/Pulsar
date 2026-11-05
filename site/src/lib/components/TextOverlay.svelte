<script lang="ts">
  import type { TextColumn } from '../styles.ts';
  import type { ItemKey } from '../theme.ts';
  import Runs from './Runs.svelte';

  let {
    columns,
    weight,
    color,
    drag = null,
    onclick,
    onkeydown,
  }: {
    columns: TextColumn[];
    weight: number;
    color: string;
    drag?: { key: ItemKey; active: boolean; transform: string | null } | null;
    onclick?: (e: MouseEvent) => void;
    onkeydown?: (e: KeyboardEvent) => void;
  } = $props();

  const held = (k: ItemKey) => drag?.key === k;
</script>

<div class="text-overlay" style:font-weight={weight} style:color>
  {#each columns as col, i (i)}
    <div class="tx-col">
      {#if col.kind === 'network'}
        <button
          type="button"
          class={[
            'tx',
            'tx-net',
            held('network') && 'held',
            held('network') && drag?.active && 'lifted',
          ]}
          data-key="network"
          aria-label="Network"
          style:transform={held('network') ? drag?.transform : null}
          {onclick}
          {onkeydown}
        >
          {#each col.rows as row, j (j)}
            <span><Runs runs={row} /></span>
          {/each}
        </button>
      {:else}
        {#each col.cells as cell (cell.key)}
          <button
            type="button"
            class={['tx', held(cell.key) && 'held', held(cell.key) && drag?.active && 'lifted']}
            data-key={cell.key}
            aria-label={cell.name}
            style:transform={held(cell.key) ? drag?.transform : null}
            {onclick}
            {onkeydown}
          >
            <span style:color={cell.labelColor}><Runs runs={cell.label} /></span>
            <span style:color={cell.valueColor}>{cell.value}</span>
          </button>
        {/each}
      {/if}
    </div>
  {/each}
</div>
