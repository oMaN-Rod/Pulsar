<script lang="ts">
  import type { Run } from '../styles.ts';
  import ItemIcon from './ItemIcon.svelte';

  let { runs }: { runs: Run[] } = $props();
</script>

{#each runs as run, i (i)}
  {#if run.dot}
    <i class="conn-dot" style:background={run.color ?? 'transparent'}></i>
  {:else if run.icon}
    <span style:color={run.color}><ItemIcon item={run.icon} /></span>
  {:else if run.color}
    <span style:color={run.color}>{run.text}</span>
  {:else}
    {run.text}
  {/if}
{/each}
