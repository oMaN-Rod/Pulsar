<script lang="ts">
  import type { ItemKey } from '../theme.ts';

  let { item }: { item: ItemKey } = $props();
</script>

<svg class="ic" viewBox="0 0 16 16" aria-hidden="true">
  {#if item === 'cpu'}
    <rect
      x="4"
      y="4"
      width="8"
      height="8"
      rx="1"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
    />
    <path
      d="M6 1.5v2M10 1.5v2M6 12.5v2M10 12.5v2M1.5 6h2M1.5 10h2M12.5 6h2M12.5 10h2"
      stroke="currentColor"
      stroke-width="1.1"
    />
  {:else if item === 'cpu_temp' || item === 'chipset_temp' || item === 'gpu_temp' || item === 'disk_temp'}
    <path
      d="M6.5 9.3V3a1.5 1.5 0 0 1 3 0v6.3a3 3 0 1 1-3 0z"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
    />
    <circle cx="8" cy="11.8" r="1.4" fill="currentColor" />
  {:else if item === 'cpu_power' || item === 'gpu_power'}
    <path
      d="M9 1.5 3.5 9H8l-1 5.5L12.5 7H8z"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
      stroke-linejoin="round"
    />
  {:else if item === 'cpu_clock'}
    <circle cx="8" cy="8.5" r="5.8" fill="none" stroke="currentColor" stroke-width="1.2" />
    <path d="M8 8.5 11 5.8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
  {:else if item === 'ram' || item === 'gpu_memory'}
    <rect
      x="1.5"
      y="4.5"
      width="13"
      height="7"
      rx="1"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
    />
    <path
      d="M4.5 7v2M7 7v2M9.5 7v2M12 7v2M3.5 11.5v2M12.5 11.5v2"
      stroke="currentColor"
      stroke-width="1.1"
    />
  {:else if item === 'gpu'}
    <rect
      x="1.5"
      y="4"
      width="13"
      height="8"
      rx="1"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
    />
    <circle cx="6" cy="8" r="2" fill="none" stroke="currentColor" stroke-width="1.1" />
    <path d="M10 6.5h2.5M10 9.5h2.5" stroke="currentColor" stroke-width="1.1" />
  {:else if item === 'disk'}
    <rect
      x="1.5"
      y="5"
      width="13"
      height="6"
      rx="1.5"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
    />
    <circle cx="11.5" cy="8" r=".9" fill="currentColor" />
  {:else if item === 'ping'}
    <circle cx="8" cy="12" r="1.2" fill="currentColor" />
    <path
      d="M5 9.5a4 4 0 0 1 6 0M2.8 7.2a7 7 0 0 1 10.4 0"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
      stroke-linecap="round"
    />
  {:else if item === 'battery'}
    <rect
      x="1.5"
      y="5"
      width="11.5"
      height="6"
      rx="1.2"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
    />
    <rect x="3" y="6.5" width="6" height="3" fill="currentColor" />
    <path d="M14.3 7v2" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
  {:else if item === 'fps'}
    <rect
      x="1.5"
      y="2.5"
      width="13"
      height="9"
      rx="1"
      fill="none"
      stroke="currentColor"
      stroke-width="1.2"
    />
    <path
      d="M5.5 14h5M8 11.5V14M4.5 9l2.5-3 2 2 2.5-3"
      fill="none"
      stroke="currentColor"
      stroke-width="1.1"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
  {/if}
</svg>
