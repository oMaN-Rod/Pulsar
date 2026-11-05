<script lang="ts">
  import { mount, tick, unmount, untrack, type Snippet } from 'svelte';
  import { on } from 'svelte/events';
  import { rgba } from '../color.ts';
  import { moveKey, taskbarKeys, widgetKeys, type Profile } from '../profile.ts';
  import type { Sim } from '../sim.ts';
  import {
    app,
    readings,
    resetWallpaper,
    useWallpaper,
    type Fit,
    type TaskbarSettings,
  } from '../state.svelte.ts';
  import { fontCss, fontPx, renderTile, textColumns, withItemColors } from '../styles.ts';
  import { META, resolve, type ItemKey, type Resolved } from '../theme.ts';
  import DeskIcon, { type DeskIconName } from './DeskIcon.svelte';
  import Notes from './Notes.svelte';
  import PopupContent from './PopupContent.svelte';
  import TextOverlay from './TextOverlay.svelte';
  import TileArt from './TileArt.svelte';
  import WidgetContent from './WidgetContent.svelte';

  let { children }: { children?: Snippet } = $props();

  /** A vertical taskbar is as wide as the horizontal one is tall. */
  const SIDE = 48;
  const GAP = 4;

  const APPS: { id: string; name: string; icon: DeskIconName; run?: 'open' | 'active' }[] = [
    { id: 'files', name: 'File Explorer', icon: 'folder', run: 'open' },
    { id: 'browser', name: 'Browser', icon: 'browser', run: 'active' },
    { id: 'terminal', name: 'Terminal', icon: 'terminal', run: 'open' },
    { id: 'mail', name: 'Mail', icon: 'mail' },
    { id: 'chat', name: 'Chat', icon: 'chat', run: 'open' },
    { id: 'music', name: 'Music', icon: 'music', run: 'open' },
    { id: 'photos', name: 'Photos', icon: 'photos' },
    { id: 'store', name: 'Store', icon: 'store' },
    { id: 'notes', name: 'Notes', icon: 'notes', run: 'open' },
    { id: 'settings', name: 'Settings', icon: 'settings' },
  ];

  type SettingKey = 'align' | 'edge' | 'position';
  type MenuItem =
    | '-'
    | { title: string; key: SettingKey; options: [string, string][] }
    | { act: string; name: string; checked?: boolean; app?: string };

  const TASKBAR_MENU: MenuItem[] = [
    {
      title: 'Taskbar alignment',
      key: 'align',
      options: [
        ['left', 'Left'],
        ['center', 'Center'],
      ],
    },
    {
      title: 'Taskbar location',
      key: 'edge',
      options: [
        ['bottom', 'Bottom'],
        ['left', 'Left side'],
        ['right', 'Right side'],
      ],
    },
    '-',
    {
      title: 'Pulsar on the taskbar',
      key: 'position',
      options: [
        ['left', 'After the app buttons'],
        ['right', 'Next to the tray'],
        ['floating', 'Floating above it'],
      ],
    },
  ];

  type Where = 'left' | 'right' | 'float' | 'none';
  interface Rung {
    p: Profile;
    keys: ItemKey[];
  }
  interface Layout extends Rung {
    where: Where;
    fixedWidth: number;
    trayKey: ItemKey | null;
  }

  let root = $state<HTMLElement>();
  let taskbarEl = $state<HTMLElement>();
  let groupEl = $state<HTMLElement>();
  let trayEl = $state<HTMLElement>();
  let trayMetricEl = $state<HTMLElement>();
  let overlayEl = $state<HTMLElement>();
  let popEl = $state<HTMLElement>();
  let ctxEl = $state<HTMLElement>();
  let widgetEl = $state<HTMLElement>();
  let widgetBody = $state<HTMLElement>();
  let picker = $state<HTMLInputElement>();
  let notes = $state<Notes>();

  const colors: Resolved = $derived(withItemColors(resolve(app.theme, app.light), app.profile));
  const vertical = $derived(app.taskbar.edge !== 'bottom');

  const initial = untrack(() => $state.snapshot(app.profile) as Profile);
  let lay = $state.raw<Layout>({
    p: initial,
    keys: taskbarKeys(initial),
    where: initial.position === 'floating' ? 'float' : initial.position,
    fixedWidth: 0,
    trayKey: null,
  });
  let laySig = '';
  let viewport = $state(0);

  let hovered = $state<ItemKey | null>(null);
  let popShown = $state(false);
  let popPos = $state<Record<string, string>>({});

  interface Drag {
    key: ItemKey;
    from: number;
    active: boolean;
    id: number;
    index: number | null;
    before: ItemKey | null;
    after: ItemKey | null;
    shift: number;
  }
  let drag = $state<Drag | null>(null);
  let widgetDrag = $state<{
    key: ItemKey;
    x: number;
    y: number;
    active: boolean;
    id: number;
    index: number | null;
    before: ItemKey | null;
    after: ItemKey | null;
  } | null>(null);
  let swallowClick = false;
  let mark = $state<{ left: number; top: number; width: number; height: number } | null>(null);

  let menu = $state<{ items: MenuItem[]; x: number; y: number } | null>(null);
  let menuPos = $state({ left: 8, top: 8 });

  let clockTime = $state('');
  let clockDate = $state('');

  function clock() {
    const now = new Date();
    clockTime = now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    clockDate = now.toLocaleDateString();
  }
  clock();

  $effect(() => {
    const timer = setInterval(clock, 15000);
    const offResize = on(window, 'resize', () => {
      viewport++;
      drawNotes();
    });
    const refresh = () => {
      notes?.reset();
      drawNotes();
    };
    const offFonts = on(document.fonts, 'loadingdone', refresh);
    document.fonts.ready.then(refresh);
    return () => {
      clearInterval(timer);
      offResize();
      offFonts();
    };
  });

  const selectItem = (k: ItemKey) => {
    app.selected = k;
    app.section = app.section === 'colors' ? 'colors' : 'items';
  };

  const keyAt = (e: Event) =>
    (((e.target as Element).closest('[data-key]') as HTMLElement | null)?.dataset.key as
      ItemKey | undefined) ?? null;
  const hover = (k: ItemKey | null) => {
    if (k !== hovered) hovered = k;
  };

  const along = (r: DOMRect): [number, number] =>
    vertical ? [r.top, r.height] : [r.left, r.width];
  const at = (e: PointerEvent) => (vertical ? e.clientY : e.clientX);

  /** Where `key` lands in `keys` when dropped before `before`, or after `after` at the end. */
  function landing(keys: ItemKey[], key: ItemKey, before: ItemKey | null, after: ItemKey | null) {
    const rest = keys.filter((k) => k !== key);
    if (before) return rest.indexOf(before);
    return after ? rest.indexOf(after) + 1 : rest.length;
  }

  /** Draws the insertion line before `next`, or after `last` when there is no next. */
  function showMark(next: Element | undefined, last: Element | undefined, across: boolean) {
    const node = next ?? last;
    if (!node || !root) {
      mark = null;
      return;
    }
    const r = node.getBoundingClientRect();
    const d = root.getBoundingClientRect();
    if (across) {
      const y = next ? r.top - 3 : r.bottom + 3;
      mark = { left: r.left - d.left, top: y - d.top - 1.5, width: r.width, height: 3 };
    } else {
      const x = next ? r.left - 3 : r.right + 3;
      mark = { left: x - d.left - 1.5, top: r.top - d.top, width: 3, height: r.height };
    }
  }

  const keyNodes = (host: Element | undefined) => [
    ...(host?.querySelectorAll<HTMLElement>('[data-key]') ?? []),
  ];

  function overlayDown(e: PointerEvent) {
    const k = keyAt(e);
    if (!k || e.button !== 0) return;
    drag = {
      key: k,
      from: at(e),
      active: false,
      id: e.pointerId,
      index: null,
      before: null,
      after: null,
      shift: 0,
    };
    hover(null);
  }

  function overlayMove(e: PointerEvent) {
    if (!drag) {
      if (!widgetDrag) hover(keyAt(e));
      return;
    }
    if (e.pointerId !== drag.id) return;
    if (!drag.active) {
      if (Math.abs(at(e) - drag.from) < 5) return;
      drag.active = true;
      try {
        overlayEl?.setPointerCapture(e.pointerId);
      } catch {}
    }
    drag.shift = at(e) - drag.from;
    const key = drag.key;
    const others = keyNodes(overlayEl).filter((n) => n.dataset.key !== key);
    drag.index = others.filter((n) => {
      const [s, len] = along(n.getBoundingClientRect());
      return s + len / 2 < at(e);
    }).length;
    drag.before = (others[drag.index]?.dataset.key as ItemKey) ?? null;
    drag.after = (others.at(-1)?.dataset.key as ItemKey) ?? null;
    showMark(others[drag.index], others.at(-1), vertical);
  }

  function endDrag() {
    if (!drag) return;
    const { key, active, index, before, after } = drag;
    mark = null;
    drag = null;
    if (!active) return;
    swallowClick = true;
    setTimeout(() => (swallowClick = false), 0);
    const keys = taskbarKeys(app.profile);
    const to = index === null ? -1 : landing(keys, key, before, after);
    if (to >= 0 && keys.indexOf(key) !== to) moveKey(app.profile, 'taskbar', key, to);
  }

  function overlayClick(e: MouseEvent) {
    if (swallowClick) {
      swallowClick = false;
      return;
    }
    const k = keyAt(e);
    if (k) selectItem(k);
  }

  function overlayKey(e: KeyboardEvent) {
    const k = keyAt(e);
    const step = (
      { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 } as Record<string, number>
    )[e.key];
    if (!k || !step || !e.altKey) return;
    e.preventDefault();
    const keys = taskbarKeys(app.profile);
    moveKey(app.profile, 'taskbar', k, keys.indexOf(k) + step);
    tick().then(() => overlayEl?.querySelector<HTMLElement>(`[data-key="${k}"]`)?.focus());
  }

  function widgetDown(e: PointerEvent) {
    const k = keyAt(e);
    if (!k || e.button !== 0) return;
    widgetDrag = {
      key: k,
      x: e.clientX,
      y: e.clientY,
      active: false,
      id: e.pointerId,
      index: null,
      before: null,
      after: null,
    };
    try {
      widgetEl?.setPointerCapture(e.pointerId);
    } catch {}
  }

  function widgetMove(e: PointerEvent) {
    if (!widgetDrag || e.pointerId !== widgetDrag.id) return;
    if (!widgetDrag.active) {
      if (Math.hypot(e.clientX - widgetDrag.x, e.clientY - widgetDrag.y) < 5) return;
      widgetDrag.active = true;
    }
    const key = widgetDrag.key;
    const others = keyNodes(widgetBody).filter((n) => n.dataset.key !== key);
    widgetDrag.index = others.filter((n) => {
      const r = n.getBoundingClientRect();
      return r.bottom <= e.clientY || (r.top <= e.clientY && r.left + r.width / 2 < e.clientX);
    }).length;
    widgetDrag.before = (others[widgetDrag.index]?.dataset.key as ItemKey) ?? null;
    widgetDrag.after = (others.at(-1)?.dataset.key as ItemKey) ?? null;
    const ref = others[widgetDrag.index] ?? others.at(-1);
    const wide =
      !!ref &&
      !!widgetBody &&
      ref.getBoundingClientRect().width > widgetBody.getBoundingClientRect().width * 0.6;
    showMark(others[widgetDrag.index], others.at(-1), wide);
  }

  function endWidgetDrag() {
    if (!widgetDrag) return;
    const { key, active, index, before, after } = widgetDrag;
    mark = null;
    widgetDrag = null;
    const keys = widgetKeys(app.profile);
    const to = active && index !== null ? landing(keys, key, before, after) : -1;
    if (to >= 0 && keys.indexOf(key) !== to) moveKey(app.profile, 'widget', key, to);
  }

  function closeApp(id: string) {
    app.taskbar.closed = [...app.taskbar.closed, id];
  }

  const current = (key: SettingKey) =>
    key === 'position' ? app.profile.position : app.taskbar[key];

  function openMenu(e: MouseEvent, items: (MenuItem | null)[]) {
    e.preventDefault();
    hover(null);
    menu = { items: items.filter((x): x is MenuItem => !!x), x: e.clientX, y: e.clientY };
  }
  const closeMenu = () => (menu = null);

  function taskbarMenu(e: MouseEvent) {
    e.stopPropagation();
    const el = (e.target as Element).closest<HTMLElement>('[data-app]');
    if (el) {
      const a = APPS.find((x) => x.id === el.dataset.app);
      if (a)
        openMenu(e, [
          {
            act: 'close-app',
            app: a.id,
            name: a.run ? `Close ${a.name}` : `Unpin ${a.name} from the taskbar`,
          },
        ]);
      return;
    }
    const closed = app.taskbar.closed.length;
    openMenu(e, [
      ...TASKBAR_MENU,
      '-',
      { act: 'transparent', name: 'Transparent taskbar', checked: !!app.taskbar.transparent },
      closed ? { act: 'restore-apps', name: `Bring back closed apps (${closed})` } : null,
    ]);
  }

  function deskMenu(e: MouseEvent) {
    if ((e.target as Element).closest('.taskbar, .panel, .hero, input, .ctx, dialog')) return;
    openMenu(e, [
      { act: 'wallpaper', name: 'Change wallpaper…' },
      app.wallpaper ? { act: 'reset-wallpaper', name: 'Default wallpaper' } : null,
      '-',
      {
        act: 'widget',
        name: app.profile.widget.enabled ? 'Hide the Pulsar widget' : 'Show the Pulsar widget',
      },
    ]);
  }

  function setting(key: SettingKey, value: string) {
    if (key === 'position') app.profile.position = value as Profile['position'];
    else if (key === 'align') app.taskbar.align = value as TaskbarSettings['align'];
    else app.taskbar.edge = value as TaskbarSettings['edge'];
  }

  function act(name: string, appId?: string) {
    closeMenu();
    if (name === 'wallpaper') picker?.click();
    else if (name === 'reset-wallpaper') resetWallpaper();
    else if (name === 'widget') app.profile.widget.enabled = !app.profile.widget.enabled;
    else if (name === 'transparent') app.taskbar.transparent = !app.taskbar.transparent;
    else if (name === 'restore-apps') app.taskbar.closed = [];
    else if (name === 'close-app' && appId) closeApp(appId);
  }

  function menuKey(e: KeyboardEvent) {
    const buttons = [...(ctxEl?.querySelectorAll('button') ?? [])];
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (e.key === 'Escape') closeMenu();
    else if (e.key === 'ArrowDown') buttons[(i + 1) % buttons.length].focus();
    else if (e.key === 'ArrowUp') buttons[(i - 1 + buttons.length) % buttons.length].focus();
    else return;
    e.preventDefault();
  }

  $effect(() => {
    if (!menu || !ctxEl || !root) return;
    const r = root.getBoundingClientRect();
    const x = Math.min(menu.x - r.left, r.width - ctxEl.offsetWidth - 8);
    const y = Math.min(menu.y - r.top, r.height - ctxEl.offsetHeight - 8);
    menuPos = { left: Math.max(8, x), top: Math.max(8, y) };
    ctxEl.querySelector('button')?.focus();
  });

  function wallpaperPicked() {
    const file = picker?.files?.[0];
    if (file) useWallpaper(file);
    if (picker) picker.value = '';
  }

  /** The room Pulsar has on each side of the taskbar buttons, along the bar. */
  function room(tb: TaskbarSettings) {
    const upright = tb.edge !== 'bottom';
    const span = (r: DOMRect | undefined): [number, number, number] =>
      !r ? [0, 0, 0] : upright ? [r.top, r.bottom, r.height] : [r.left, r.right, r.width];
    const [barStart, barEnd, barLen] = span(taskbarEl?.getBoundingClientRect());
    const [gStart, gEnd, gLen] = span(groupEl?.getBoundingClientRect());
    const metric =
      trayMetricEl && !trayMetricEl.hidden
        ? span(trayMetricEl.getBoundingClientRect())[2] + GAP
        : 0;
    const trayLen = span(trayEl?.getBoundingClientRect())[2] - metric;
    const pad = 14;
    if (tb.align === 'left') {
      const free = barLen - gLen - trayLen - pad - 6;
      return { left: free, right: free };
    }
    return { left: gStart - barStart - pad, right: barEnd - gEnd - trayLen - pad };
  }

  function measure(rung: Rung, upright: boolean, v: Resolved, sim: Sim, light: boolean) {
    if (rung.p.mode === 'text') {
      if (!root) return 0;
      const probe = document.createElement('div');
      probe.className = upright ? 'overlay probe upright' : 'overlay probe';
      probe.style.fontFamily = fontCss(rung.p.font);
      probe.style.fontSize = `${fontPx(rung.p.font)}px`;
      root.append(probe);
      const view = mount(TextOverlay, {
        target: probe,
        props: {
          columns: textColumns(rung.keys, { v, sim, p: rung.p, light }),
          weight: rung.p.font.bold ? 700 : 400,
          color: v.text,
        },
      });
      const len = upright ? probe.offsetHeight : probe.offsetWidth;
      unmount(view);
      probe.remove();
      return len;
    }
    const fixedWidth = upright ? SIDE - 6 : 0;
    const sizes = rung.keys.map((k) =>
      renderTile(k, { v, sim, p: rung.p, item: rung.p.items[k], light, fixedWidth }),
    );
    return (
      sizes.reduce((s, d) => s + (upright ? d.h : d.w), 0) + GAP * Math.max(0, sizes.length - 1) + 4
    );
  }

  /** Pulsar's fit ladder: full, then compact, then fewer and fewer items. */
  function rungs(p: Profile, keys: ItemKey[], upright: boolean, size: (r: Rung) => number): Rung[] {
    const list: Rung[] = [{ p, keys }];
    if (!p.fit.enabled) return list;
    let compact: Profile = {
      ...p,
      labels: p.icons ? 'none' : p.labels === 'full' ? 'short' : p.labels,
      font: { ...p.font, size: Math.max(6, p.font.size * 0.9) },
    };
    if (p.inline && p.mode !== 'text') {
      const stacked = { ...compact, inline: false };
      if (size({ p: stacked, keys }) < size({ p: compact, keys })) compact = stacked;
    }
    list.push({ p: compact, keys });
    for (let n = 1; n < keys.length; n++)
      list.push({
        p: compact,
        keys: p.fit.removeFrom === 'left' ? keys.slice(n) : keys.slice(0, keys.length - n),
      });
    return list;
  }

  function refit(
    p: Profile,
    tb: TaskbarSettings,
    v: Resolved,
    light: boolean,
    sim: Sim,
    dragging: boolean,
  ): { layout: Layout; fit: Fit } {
    const upright = tb.edge !== 'bottom';
    const keys = taskbarKeys(p);
    if (p.position === 'floating')
      return { fit: 'full', layout: { p, keys, where: 'float', fixedWidth: 0, trayKey: null } };
    const side = p.position;
    const space = room(tb);
    const size = (r: Rung) => measure(r, upright, v, sim, light);
    let chosen: Rung | null = null;
    let where: Where = side;
    const ladder = rungs(p, keys, upright, size);
    if (!p.fit.enabled || dragging || !keys.length) chosen = ladder[0];
    else {
      const other = side === 'left' ? 'right' : 'left';
      const full = size(ladder[0]);
      if (full <= space[side]) chosen = ladder[0];
      else if (tb.align === 'center' && p.fit.otherSide && full <= space[other]) {
        chosen = ladder[0];
        where = other;
      } else chosen = ladder.slice(1).find((r) => size(r) <= space[side]) ?? null;
    }
    const noRoom = !chosen && keys.length > 0;
    if (noRoom) {
      where = p.fit.noRoom === 'float' ? 'float' : 'none';
      chosen = p.fit.noRoom === 'float' ? ladder[0] : { p, keys: [] };
    }
    const rung = chosen as Rung;
    return {
      fit: noRoom ? p.fit.noRoom : rung === ladder[0] ? 'full' : 'shrunk',
      layout: {
        p: rung.p,
        keys: rung.keys,
        where,
        fixedWidth: upright && where !== 'float' ? SIDE - 6 : 0,
        trayKey: noRoom && p.fit.noRoom === 'tray' && keys[0] ? keys[0] : null,
      },
    };
  }

  $effect(() => {
    const sim = readings();
    const p = $state.snapshot(app.profile) as Profile;
    const tb = $state.snapshot(app.taskbar) as TaskbarSettings;
    const v = colors;
    const light = app.light;
    const dragging = !!drag?.active;
    void viewport;
    if (!taskbarEl) return;
    untrack(() => {
      const next = refit(p, tb, v, light, sim, dragging);
      app.fit = next.fit;
      const sig = JSON.stringify(next.layout);
      if (sig !== laySig) {
        laySig = sig;
        lay = next.layout;
      }
    });
  });

  const tiles = $derived.by(() => {
    const sim = readings();
    if (lay.p.mode === 'text') return [];
    return lay.keys.map((k) => ({
      key: k,
      tile: renderTile(k, {
        v: colors,
        sim,
        p: lay.p,
        item: lay.p.items[k],
        light: app.light,
        fixedWidth: lay.fixedWidth,
      }),
    }));
  });

  const columns = $derived.by(() => {
    const sim = readings();
    return lay.p.mode === 'text'
      ? textColumns(lay.keys, { v: colors, sim, p: lay.p, light: app.light })
      : [];
  });

  const trayText = $derived.by(() => {
    const sim = readings();
    return lay.trayKey ? sim.text(lay.trayKey) : '';
  });

  const dragStyle = $derived(
    drag?.active
      ? vertical
        ? `translateY(${drag.shift}px)`
        : `translateX(${drag.shift}px)`
      : null,
  );

  $effect(() => {
    void app.tick;
    void lay;
    const k = hovered;
    void (k && app.profile.items[k].popupWidth);
    void (k && app.profile.items[k].popupGraphHeight);
    const settings = app.profile.popup;
    void settings.width;
    void settings.graphHeight;
    void settings.processes;
    void settings.graph;
    void app.profile.popupDetail;
    const edge = app.taskbar.edge;
    untrack(() => {
      const anchor =
        k &&
        (overlayEl?.querySelector(`[data-key="${k}"]`) ??
          (trayMetricEl && !trayMetricEl.hidden ? trayMetricEl : null));
      if (!anchor || !popEl || !root || !taskbarEl) {
        popShown = false;
        return;
      }
      popEl.hidden = false;
      const r = root.getBoundingClientRect();
      const t = anchor.getBoundingClientRect();
      const bar = taskbarEl.getBoundingClientRect();
      const w = popEl.offsetWidth;
      const h = popEl.offsetHeight;
      if (edge === 'bottom') {
        popPos = {
          left: `${Math.max(8, Math.min(r.width - w - 8, t.left - r.left + t.width / 2 - w / 2))}px`,
          bottom: `${r.bottom - Math.min(bar.top, t.top) + 10}px`,
        };
      } else {
        const top = `${Math.max(8, Math.min(r.height - h - 8, t.top - r.top + t.height / 2 - h / 2))}px`;
        popPos =
          edge === 'left'
            ? { top, left: `${Math.max(bar.right, t.right) - r.left + 10}px` }
            : { top, right: `${r.right - Math.min(bar.left, t.left) + 10}px` };
      }
      popShown = true;
    });
  });

  function drawNotes() {
    untrack(() => notes?.draw(app.notes && innerWidth > 900, app.taskbar.edge));
  }

  $effect(() => {
    JSON.stringify([
      app.theme,
      app.profile,
      app.taskbar,
      app.selected,
      app.section,
      app.light,
      app.panelOpen,
      app.notes,
      app.toast,
      app.themeMenu,
      app.wallpaper,
    ]);
    void lay;
    void notes;
    untrack(() => notes?.reset());
    drawNotes();
  });

  $effect(() => {
    void app.tick;
    drawNotes();
  });
</script>

<svelte:document
  onpointerdown={(e) => {
    if (menu && ctxEl && !ctxEl.contains(e.target as Node)) closeMenu();
  }}
/>

{#snippet group()}
  <div class="tb-group" role="group" aria-label="Taskbar buttons" bind:this={groupEl}>
    <span class="tb-btn" aria-hidden="true"><DeskIcon name="start" /></span>
    <span class="tb-btn tb-search" aria-hidden="true"><DeskIcon name="search" /></span>
    <span class="tb-btn" aria-hidden="true"><DeskIcon name="taskview" /></span>
    {#each APPS.filter((a) => !app.taskbar.closed.includes(a.id)) as a (a.id)}
      <span class="tb-btn tb-app" data-app={a.id}>
        <span aria-hidden="true"><DeskIcon name={a.icon} /></span>
        {#if a.run}
          <i class="run {a.run}"></i>
        {/if}
        <button
          type="button"
          class="app-close"
          aria-label="{a.run ? 'Close' : 'Unpin'} {a.name}"
          onclick={() => closeApp(a.id)}
        >
          <DeskIcon name="close" />
        </button>
      </span>
    {/each}
  </div>
{/snippet}

{#snippet overlay()}
  <div
    class={['overlay', lay.p.mode === 'text' && 'text', drag?.active && 'dragging']}
    role="group"
    aria-label="Pulsar overlay"
    bind:this={overlayEl}
    style:font-family={fontCss(lay.p.font)}
    style:font-size="{fontPx(lay.p.font)}px"
    style:background={colors.panelOpacity
      ? rgba(colors.panel, colors.panelOpacity / 100)
      : 'transparent'}
    onpointermove={overlayMove}
    onpointerleave={() => hover(null)}
    onfocusin={(e) => hover(keyAt(e))}
    onfocusout={() => hover(null)}
    onpointerdown={overlayDown}
    onpointerup={endDrag}
    onpointercancel={endDrag}
  >
    {#if lay.p.mode === 'text'}
      <TextOverlay
        {columns}
        weight={lay.p.font.bold ? 700 : 400}
        color={colors.text}
        drag={drag ? { key: drag.key, active: drag.active, transform: dragStyle } : null}
        onclick={overlayClick}
        onkeydown={overlayKey}
      />
    {:else}
      {#each tiles as t (t.key)}
        <button
          type="button"
          class={[
            'tile',
            t.key === app.selected && 'selected',
            drag?.key === t.key && 'held',
            drag?.key === t.key && drag.active && 'lifted',
          ]}
          data-key={t.key}
          aria-label={META[t.key].name}
          aria-roledescription="Pulsar item; Alt and the arrow keys move it"
          style:width="{t.tile.w}px"
          style:height="{t.tile.h}px"
          style:background={t.tile.bg ?? rgba(colors.tile, colors.tileOpacity / 100)}
          style:transform={drag?.key === t.key ? dragStyle : null}
          onclick={overlayClick}
          onkeydown={overlayKey}
        >
          <TileArt tile={t.tile} />
        </button>
      {/each}
    {/if}
  </div>
{/snippet}

{#snippet tray()}
  <div class="tb-tray-area" bind:this={trayEl}>
    <span
      class="tray-metric"
      hidden={!lay.trayKey}
      data-key={lay.trayKey}
      aria-label={lay.trayKey ? `${META[lay.trayKey].name} ${trayText}` : null}
      bind:this={trayMetricEl}
      onpointermove={(e) => !drag && !widgetDrag && hover(keyAt(e))}
      onpointerleave={() => hover(null)}
      onfocusin={(e) => hover(keyAt(e))}
      onfocusout={() => hover(null)}
      onclick={() => hovered && selectItem(hovered)}
      role="presentation"
    >
      {#if lay.trayKey}
        <span style:color={colors.items[lay.trayKey]}>{trayText.replace(/\s.*$/, '')}</span>
      {/if}
    </span>
    <span class="tb-tray" aria-hidden="true"><DeskIcon name="chevron" /></span>
    <span class="tb-tray tb-tray-icons" aria-hidden="true">
      <DeskIcon name="wifi" />
      <DeskIcon name="volume" />
      <DeskIcon name="battery" />
    </span>
    <span class="tb-tray tb-clock" aria-hidden="true">
      <span class="clock-time">{clockTime}</span>
      <span class="clock-date">{clockDate}</span>
    </span>
    <span class="tb-tray" aria-hidden="true"><DeskIcon name="bell" /></span>
    <span class="tb-peek" aria-hidden="true"></span>
  </div>
{/snippet}

<div
  class={['desk', app.light && 'light', app.taskbar.transparent && 'clear-bar']}
  data-edge={app.taskbar.edge}
  data-align={app.taskbar.align}
  data-fit={app.fit}
  bind:this={root}
  oncontextmenu={deskMenu}
  ontransitionend={drawNotes}
  role="presentation"
>
  <div
    class={['desk-wall', app.wallpaper && 'custom']}
    style:background-image={app.wallpaper ? `url("${app.wallpaper.url}")` : null}
  ></div>
  <Notes bind:this={notes} {root} />
  <div class="desk-icons">
    <div class="desk-icon" aria-hidden="true">
      <DeskIcon name="bin" />
      <span>Recycle Bin</span>
    </div>
    <button
      type="button"
      class="desk-icon"
      title="Choose a picture for the desktop"
      onclick={() => picker?.click()}
    >
      <DeskIcon name="picture" />
      <span>Wallpaper</span>
    </button>
    <button
      type="button"
      class="desk-icon"
      title="Open a theme or profile"
      onclick={() => (app.dialog = 'import')}
    >
      <DeskIcon name="open" />
      <span>Import</span>
    </button>
    <button
      type="button"
      class="desk-icon"
      data-export
      title="Save the theme and profile for Pulsar"
      onclick={() => (app.dialog = 'export')}
    >
      <DeskIcon name="save" />
      <span>Export</span>
    </button>
    <button
      type="button"
      class="desk-icon"
      title="Share the theme with the community"
      onclick={() => (app.dialog = 'share')}
    >
      <DeskIcon name="share" />
      <span>Share</span>
    </button>
    <button
      type="button"
      class="desk-icon"
      aria-pressed={app.notes}
      title="Show or hide the handwritten tips"
      onclick={() => (app.notes = !app.notes)}
    >
      <DeskIcon name="board" />
      <span>Tips</span>
    </button>
  </div>
  <input
    type="file"
    accept="image/*"
    class="sr-only"
    tabindex="-1"
    bind:this={picker}
    onchange={wallpaperPicked}
  />
  <div class="desk-content">{@render children?.()}</div>
  <div
    class={['widget', widgetDrag?.active && 'dragging']}
    aria-label="Pulsar desktop widget"
    hidden={!app.profile.widget.enabled}
    bind:this={widgetEl}
    style:width="{app.profile.widget.width}px"
    style:background={rgba(
      app.profile.widget.background ?? colors.panel,
      app.profile.widget.opacity / 100,
    )}
    style:color={colors.text}
    style:font-family={fontCss(app.profile.font)}
    onpointerdown={widgetDown}
    onpointermove={widgetMove}
    onpointerup={endWidgetDrag}
    onpointercancel={endWidgetDrag}
    role="group"
  >
    <div class="widget-body" bind:this={widgetBody}>
      {#if app.profile.widget.enabled}
        <WidgetContent
          items={widgetKeys(app.profile)}
          widget={app.profile.widget}
          settings={app.profile.items}
          v={colors}
          held={widgetDrag?.key ?? null}
        />
      {/if}
    </div>
  </div>
  <div class="float-box" hidden={lay.where !== 'float'} style:background={rgba(colors.panel, 0.92)}>
    {#if lay.where === 'float'}
      {@render overlay()}
    {/if}
  </div>
  <div
    class={['pop', app.profile.popup.background === 'solid' && 'solid']}
    role="tooltip"
    hidden={!popShown || !hovered}
    bind:this={popEl}
    style:background={rgba(colors.panel, app.profile.popup.background === 'solid' ? 1 : 0.88)}
    style:color={colors.text}
    style:width="{(hovered && app.profile.items[hovered].popupWidth) || app.profile.popup.width}px"
    style:font-family={fontCss(app.profile.font)}
    style:left={popPos.left}
    style:right={popPos.right}
    style:top={popPos.top}
    style:bottom={popPos.bottom}
  >
    {#if hovered}
      <PopupContent
        item={hovered}
        detail={app.profile.items[hovered].popup ?? app.profile.popupDetail}
        v={colors}
        settings={{
          ...app.profile.popup,
          graphHeight: app.profile.items[hovered].popupGraphHeight ?? app.profile.popup.graphHeight,
        }}
      />
    {/if}
  </div>
  <div
    class="ctx"
    role="menu"
    tabindex="-1"
    hidden={!menu}
    bind:this={ctxEl}
    style:left="{menuPos.left}px"
    style:top="{menuPos.top}px"
    onkeydown={menuKey}
  >
    {#each menu?.items ?? [] as item, i (i)}
      {#if item === '-'}
        <div class="ctx-sep" role="separator"></div>
      {:else if 'options' in item}
        <div class="ctx-title">{item.title}</div>
        {#each item.options as [val, name] (val)}
          <button
            type="button"
            role="menuitemradio"
            aria-checked={current(item.key) === val}
            onclick={() => (closeMenu(), setting(item.key, val))}
          >
            <span class="ctx-check"></span>
            {name}
          </button>
        {/each}
      {:else if item.checked === undefined}
        <button type="button" role="menuitem" onclick={() => act(item.act, item.app)}>
          <span class="ctx-check"></span>
          {item.name}
        </button>
      {:else}
        <button
          type="button"
          role="menuitemcheckbox"
          aria-checked={item.checked}
          onclick={() => act(item.act, item.app)}
        >
          <span class="ctx-check box"></span>
          {item.name}
        </button>
      {/if}
    {/each}
  </div>
  <div
    class="drop-mark"
    aria-hidden="true"
    hidden={!mark}
    style:left="{mark?.left}px"
    style:top="{mark?.top}px"
    style:width="{mark?.width}px"
    style:height="{mark?.height}px"
  ></div>
  <div
    class="taskbar"
    title="Right-click for taskbar settings"
    bind:this={taskbarEl}
    oncontextmenu={taskbarMenu}
    role="presentation"
  >
    <div class="tb-a">
      {#if app.taskbar.align === 'left'}
        {@render group()}
      {/if}
      {#if lay.where === 'left'}
        {@render overlay()}
      {/if}
    </div>
    <div class="tb-b">
      {#if app.taskbar.align === 'center'}
        {@render group()}
      {/if}
    </div>
    <div class="tb-c">
      {#if lay.where === 'right'}
        {@render overlay()}
      {/if}
      {@render tray()}
    </div>
  </div>
</div>
