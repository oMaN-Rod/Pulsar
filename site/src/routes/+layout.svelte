<script lang="ts">
  import '../app.css';
  import { asset } from '$app/paths';
  import { page } from '$app/state';
  import { onMount, type Snippet } from 'svelte';
  import { loadCatalog } from '$lib/community.ts';
  import AppBar from '$lib/components/AppBar.svelte';
  import ThemeMenu from '$lib/components/ThemeMenu.svelte';
  import { app, save, step, themeOf, toast, wasRestored } from '$lib/state.svelte.ts';

  let { children }: { children: Snippet } = $props();

  const route = $derived(page.route.id === '/orbit' ? 'orbit' : 'studio');
  let themeButton = $state<HTMLButtonElement>();
  let toastEl: HTMLElement;

  $effect(() => {
    save();
  });

  $effect(() => {
    document.body.dataset.route = route;
    app.themeMenu = false;
    scrollTo(0, 0);
  });

  $effect(() => {
    document.body.classList.toggle('day', app.light);
  });

  $effect(() => {
    document.body.dataset.edge = route === 'studio' ? app.taskbar.edge : 'bottom';
  });

  $effect(() => {
    if (!app.toast) return;
    if (toastEl.matches(':popover-open')) toastEl.hidePopover();
    toastEl.showPopover();
  });

  onMount(() => {
    const timer = setInterval(() => {
      if (!app.live || document.hidden) return;
      step();
    }, 1000);
    loadCatalog(asset('/catalog.json'))
      .then((catalog) => {
        app.catalog = catalog;
        if (!wasRestored()) {
          const first = catalog.builtin.find((e) => e.id === 'aurora') ?? catalog.builtin[0];
          if (first) {
            app.theme = themeOf(first);
            app.source = { kind: 'builtin', id: first.id };
          }
        }
      })
      .catch(() => toast('The theme catalog could not be loaded'));
    return () => clearInterval(timer);
  });
</script>

<svelte:head>
  <title>{route === 'orbit' ? 'Orbit · Pulsar Themes' : 'Pulsar Themes'}</title>
</svelte:head>

<div class="bar-host"><AppBar {route} bind:themeButton /></div>
<main class="view-host">{@render children()}</main>
<div
  class={['toast', app.toast && 'shown']}
  role="status"
  aria-live="polite"
  popover="manual"
  bind:this={toastEl}
>
  {app.toast ?? ''}
</div>
<ThemeMenu anchor={themeButton} visible={app.themeMenu && route === 'studio'} />
