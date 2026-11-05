<script lang="ts">
  import { app } from '../state.svelte.ts';
  import { orb } from '../ui.ts';

  let {
    route,
    themeButton = $bindable(),
  }: { route: 'studio' | 'orbit'; themeButton?: HTMLButtonElement } = $props();

  const ROUTES = [
    ['studio', 'Studio', '#/'],
    ['orbit', 'Orbit', '#/orbit'],
  ] as const;
</script>

<header class="appbar">
  <a class="brand" href="#/">
    <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
      <polyline
        points="1,11 5,11 7.5,4 11,16 13.5,8 19,8"
        fill="none"
        stroke="currentColor"
        stroke-width="1.6"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
    <span>Pulsar</span>
    <span class="brand-sub">Themes</span>
  </a>
  <nav class="routes" aria-label="Pages">
    {#each ROUTES as [id, name, href] (id)}
      <a {href} aria-current={route === id ? 'page' : undefined}>{name}</a>
    {/each}
  </nav>
  <div class="appbar-tools">
    {#if route === 'studio'}
      <button
        type="button"
        class="theme-button"
        aria-haspopup="dialog"
        aria-expanded={app.themeMenu}
        aria-controls="theme-menu"
        bind:this={themeButton}
        onclick={() => (app.themeMenu = !app.themeMenu)}
      >
        <span class="orb tiny" style:background={orb(app.theme)}></span>
        <span class="theme-button-name">{app.theme.name}</span>
        <svg class="picker-chevron" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <path
            d="m4 6 4 4 4-4"
            fill="none"
            stroke="currentColor"
            stroke-width="1.5"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
    {/if}
    <div class="seg" role="group" aria-label="Taskbar">
      <button type="button" aria-pressed={!app.light} onclick={() => (app.light = false)}>
        Night
      </button>
      <button type="button" aria-pressed={app.light} onclick={() => (app.light = true)}>Day</button>
    </div>
    <button
      type="button"
      class="pill-toggle"
      aria-pressed={app.live}
      onclick={() => (app.live = !app.live)}
    >
      <span class="pill-dot"></span>
      Live
    </button>
  </div>
</header>
