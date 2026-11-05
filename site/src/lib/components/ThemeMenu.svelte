<script lang="ts">
  import { tick } from 'svelte';
  import type { CatalogEntry } from '../community.ts';
  import { app, themeOf } from '../state.svelte.ts';
  import { orb } from '../ui.ts';

  let { anchor, visible }: { anchor: HTMLButtonElement | undefined; visible: boolean } = $props();

  let menu = $state<HTMLElement>();
  let pos = $state({ top: 0, right: 16 });
  let viewport = $state(0);

  const entries = $derived(
    app.catalog[app.tab].map((e: CatalogEntry) => {
      const t = themeOf(e);
      const credit =
        app.tab === 'community'
          ? [`by ${t.author ?? 'anonymous'}`, t.basedOn && `after ${t.basedOn}`]
              .filter(Boolean)
              .join(' · ')
          : '';
      return {
        id: e.id,
        name: e.name,
        orb: orb(t),
        credit,
        on: app.source.kind === app.tab && app.source.id === e.id,
      };
    }),
  );

  const note = $derived(
    app.source.kind === 'custom'
      ? `You're editing “${app.theme.name}”. Picking a theme here replaces it, so export it first to keep it.`
      : app.tab === 'community'
        ? 'Share yours with the Share icon on the desktop.'
        : 'These ship with Pulsar.',
  );

  $effect(() => {
    void viewport;
    void app.theme.name;
    if (!visible || !anchor) return;
    const b = anchor.getBoundingClientRect();
    pos = { top: b.bottom + 8, right: Math.max(16, innerWidth - b.right) };
  });

  let wasVisible = false;
  $effect(() => {
    const opening = visible && !wasVisible;
    wasVisible = visible;
    if (!opening) return;
    tick().then(() =>
      (
        menu?.querySelector<HTMLElement>('.theme-row[aria-pressed=true]') ??
        menu?.querySelector<HTMLElement>('[role=tab][aria-selected=true]')
      )?.focus(),
    );
  });

  function close(refocus: boolean) {
    if (!app.themeMenu) return;
    app.themeMenu = false;
    if (refocus) anchor?.focus();
  }

  function pick(id: string) {
    const kind = app.tab;
    const entry = app.catalog[kind].find((x) => x.id === id);
    if (entry) {
      app.theme = themeOf(entry);
      app.source = { kind, id };
    }
  }

  function keydown(e: KeyboardEvent) {
    const rows = [...(menu?.querySelectorAll<HTMLElement>('.theme-row') ?? [])];
    const i = rows.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') close(true);
    else if (e.key === 'ArrowDown' && i >= 0) rows[(i + 1) % rows.length].focus();
    else if (e.key === 'ArrowUp' && i >= 0) rows[(i - 1 + rows.length) % rows.length].focus();
    else return;
    e.preventDefault();
  }
</script>

<svelte:window onresize={() => viewport++} />
<svelte:document
  onpointerdown={(e) => {
    const target = e.target as Element;
    if (app.themeMenu && menu && !menu.contains(target) && !target.closest?.('.theme-button'))
      close(false);
  }}
/>

<div
  class="theme-menu"
  id="theme-menu"
  role="dialog"
  aria-label="Themes"
  tabindex="-1"
  hidden={!visible}
  bind:this={menu}
  style:top="{pos.top}px"
  style:right="{pos.right}px"
  onkeydown={keydown}
>
  <div class="tabs" role="tablist" aria-label="Where the themes come from">
    <button
      type="button"
      role="tab"
      aria-selected={app.tab === 'builtin'}
      onclick={() => (app.tab = 'builtin')}
    >
      Built-in
    </button>
    <button
      type="button"
      role="tab"
      aria-selected={app.tab === 'community'}
      onclick={() => (app.tab = 'community')}
    >
      Community
    </button>
  </div>
  <div class="theme-list">
    {#each entries as e (e.id)}
      <button
        type="button"
        class="theme-row"
        aria-pressed={e.on}
        title={e.credit || null}
        onclick={() => pick(e.id)}
      >
        <span class="orb small" style:background={e.orb}></span>
        <span class="theme-text">
          <span>{e.name}</span>
          {#if e.credit}
            <small>{e.credit}</small>
          {/if}
        </span>
      </button>
    {:else}
      <p class="empty">No community themes yet. Shape one and be the first.</p>
    {/each}
  </div>
  <p class="note theme-menu-note">{note}</p>
</div>
