<script lang="ts">
  import { tick } from 'svelte';
  import { app, type Section } from '../../state.svelte.ts';
  import { resolve } from '../../theme.ts';
  import ColorsTab from './ColorsTab.svelte';
  import DisplayTab from './DisplayTab.svelte';
  import ExportDialog from './ExportDialog.svelte';
  import ImportDialog from './ImportDialog.svelte';
  import ItemsTab from './ItemsTab.svelte';
  import PopupsTab from './PopupsTab.svelte';
  import RenameDialog from './RenameDialog.svelte';
  import ShareDialog from './ShareDialog.svelte';
  import WidgetTab from './WidgetTab.svelte';

  const SECTIONS: [Section, string][] = [
    ['colors', 'Colors'],
    ['items', 'Items'],
    ['display', 'Display'],
    ['popups', 'Popups'],
    ['widget', 'Widget'],
  ];

  let openButton: HTMLButtonElement;
  let hideButton: HTMLButtonElement;

  const shown = $derived(resolve(app.theme, app.light));
  const credit = $derived(
    [
      app.theme.author && `by ${app.theme.author}`,
      app.theme.basedOn && `after ${app.theme.basedOn}`,
    ]
      .filter(Boolean)
      .join(' · '),
  );

  async function togglePanel() {
    app.panelOpen = !app.panelOpen;
    await tick();
    (app.panelOpen ? hideButton : openButton).focus({ preventScroll: true });
  }

  $effect(() => {
    document.body.classList.toggle('panel-closed', !app.panelOpen);
    return () => document.body.classList.remove('panel-closed');
  });
</script>

<div class={['studio', !app.panelOpen && 'collapsed']}>
  <section class="hero">
    <span class="eyebrow">Theme</span>
    <div class="hero-name">
      <h1 class="hero-title">{app.theme.name}</h1>
      <button
        type="button"
        class="rename"
        aria-label="Edit the name and author"
        title="Edit the name and author"
        onclick={() => (app.dialog = 'rename')}
      >
        <svg viewBox="0 0 16 16" width="18" height="18" aria-hidden="true">
          <path
            d="M10.6 2.6a1.6 1.6 0 0 1 2.3 2.3L5.6 12.2l-3.1.8.8-3.1z"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linejoin="round"
          />
          <path d="m9.4 3.8 2.8 2.8" stroke="currentColor" stroke-width="1.3" />
        </svg>
      </button>
    </div>
    <p class="hero-credit" hidden={!credit}>{credit}</p>
    <p class="hero-note">
      Every color is what Pulsar draws. Pick an item in the panel or on the taskbar, then shape it.
      Right-click the taskbar for its layout.
    </p>
    <p class="hero-flag" hidden={!(app.light && (shown.derived || app.theme.light))}>
      {app.theme.light
        ? 'This theme brings its own light colors; edits change the dark ones.'
        : 'On a light taskbar Pulsar derives these colors from the dark ones.'}
    </p>
  </section>
  <button
    type="button"
    class="panel-open"
    aria-controls="settings-panel"
    aria-expanded="false"
    inert={app.panelOpen}
    bind:this={openButton}
    onclick={togglePanel}
  >
    <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
      <path
        d="M2 4.5h7M12 4.5h2M2 11.5h2M7 11.5h7"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linecap="round"
      />
      <circle cx="10.5" cy="4.5" r="1.6" fill="none" stroke="currentColor" stroke-width="1.4" />
      <circle cx="5.5" cy="11.5" r="1.6" fill="none" stroke="currentColor" stroke-width="1.4" />
    </svg>
    <span>Settings</span>
  </button>
  <aside class="panel" id="settings-panel" aria-label="Theme settings" inert={!app.panelOpen}>
    <div class="sections" role="tablist" aria-label="Settings">
      {#each SECTIONS as [id, name] (id)}
        <button
          type="button"
          role="tab"
          aria-selected={app.section === id}
          onclick={() => (app.section = id)}
        >
          {name}
        </button>
      {/each}
    </div>
    <div class="panel-scroll">
      <div class="section" hidden={app.section !== 'colors'}><ColorsTab /></div>
      <div class="section" hidden={app.section !== 'items'}><ItemsTab /></div>
      <div class="section" hidden={app.section !== 'display'}><DisplayTab /></div>
      <div class="section" hidden={app.section !== 'popups'}><PopupsTab /></div>
      <div class="section" hidden={app.section !== 'widget'}><WidgetTab /></div>
    </div>
    <div class="panel-foot">
      <button
        type="button"
        class="panel-hide"
        aria-controls="settings-panel"
        aria-expanded="true"
        bind:this={hideButton}
        onclick={togglePanel}
      >
        <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
          <rect
            x="1.75"
            y="2.75"
            width="12.5"
            height="10.5"
            rx="2"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
          />
          <path
            d="M10 3v10M5.5 6 7.5 8l-2 2"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
        <span>Hide settings</span>
      </button>
    </div>
  </aside>

  <RenameDialog />
  <ExportDialog />
  <ImportDialog />
  <ShareDialog />
</div>
