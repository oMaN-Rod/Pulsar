<script lang="ts">
  import { goto } from '$app/navigation';
  import { contrast, fromHsl, mix, rgba, toHsl } from '../color.ts';
  import { generateTheme, HARMONIES, harmonyOf } from '../generate.ts';
  import { defaultItem } from '../profile.ts';
  import { app, readings, resetWallpaper, useWallpaper, type Orbit } from '../state.svelte.ts';
  import { fontCss, renderTile } from '../styles.ts';
  import { META, resolve, type ItemKey } from '../theme.ts';
  import { keep, rangeFill } from '../ui.ts';
  import TileArt from './TileArt.svelte';

  const SIZE = 560;
  const C = SIZE / 2;
  const STRIP: ItemKey[] = ['cpu', 'cpu_temp', 'ram', 'gpu', 'gpu_temp', 'disk', 'network'];
  const SLIDERS = [
    ['spread', 'Spread', 0, 100],
    ['vivid', 'Vividness', 30, 95],
    ['bright', 'Brightness', 50, 80],
  ] as const;

  const polar = (deg: number, r: number): [number, number] => {
    const a = (deg * Math.PI) / 180;
    return [C + r * Math.sin(a), C - r * Math.cos(a)];
  };

  let wrap = $state<HTMLElement>();
  let wheel = $state<HTMLElement>();
  let scale = $state(1);
  let over = $state(false);

  $effect(() => {
    if (!wrap) return;
    const node = wrap;
    const fit = () =>
      (scale = Math.max(0.4, Math.min(1, node.clientWidth / SIZE, node.clientHeight / SIZE)));
    const observer = new ResizeObserver(fit);
    observer.observe(node);
    return () => observer.disconnect();
  });

  const o = $derived(app.orbit);
  const H = $derived(harmonyOf(o.harmony));
  const theme = $derived(generateTheme(o));
  const hue = $derived(toHsl(o.seed)[0]);
  const sp = $derived(10 + o.spread * 0.7);

  const dots = $derived.by(() => {
    const placed: [number, number][] = [];
    return (Object.entries(theme.dark.items) as [ItemKey, string][]).map(([k, c]) => {
      const [hh, , l] = toHsl(c);
      const r = 90 + ((l - 40) / 48) * 170;
      const [x, y] = polar(hh, r);
      const [lx, ly] = polar(hh, r + 24);
      const crowded = placed.some(([px, py]) => Math.abs(px - lx) < 40 && Math.abs(py - ly) < 14);
      if (!crowded) placed.push([lx, ly]);
      return { k, c, x, y, label: crowded ? null : { x: lx, y: ly } };
    });
  });
  const lines = $derived(H.anchors(hue, sp).map((a) => polar(a, C)));
  const seedAt = $derived(polar(hue, C));

  const score = $derived.by(() => {
    const v = resolve(theme, false);
    const tileBg = mix('#1C1C1C', v.tile, v.tileOpacity / 100);
    const worst = Math.min(...Object.values(v.items).map((c) => contrast(c, tileBg)));
    return { worst, level: worst >= 4.5 ? 'good' : worst >= 3 ? 'fair' : 'poor' };
  });

  const bands = $derived.by(() => {
    const sim = readings();
    return (
      [
        ['night', false],
        ['day', true],
      ] as const
    ).map(([band, light]) => {
      const v = resolve(theme, light);
      return {
        band,
        tiles: STRIP.map((k) => ({
          k,
          v,
          tile: renderTile(k, {
            v,
            sim,
            p: app.profile,
            item: { ...defaultItem(k), style: 'area' },
            light,
          }),
        })),
      };
    });
  });

  function setOrbit(patch: Partial<Orbit>) {
    app.orbit = { ...app.orbit, ...patch };
  }

  function hueAt(e: PointerEvent) {
    const r = (wheel as HTMLElement).getBoundingClientRect();
    const x = e.clientX - r.left - r.width / 2;
    const y = e.clientY - r.top - r.height / 2;
    return (Math.atan2(x, -y) * 180) / Math.PI + 360;
  }

  function moveSeed(deg: number) {
    const [, s, l] = toHsl(app.orbit.seed);
    setOrbit({ seed: fromHsl(deg, Math.max(s, 35), Math.min(Math.max(l, 35), 75)) });
  }

  function wheelKey(e: KeyboardEvent) {
    const step = (
      { ArrowRight: 5, ArrowUp: 5, ArrowLeft: -5, ArrowDown: -5 } as Record<string, number>
    )[e.key];
    if (!step) return;
    e.preventDefault();
    moveSeed(toHsl(app.orbit.seed)[0] + step);
  }

  function refine() {
    app.theme = generateTheme($state.snapshot(app.orbit));
    app.source = { kind: 'custom', id: null };
    goto('#/');
  }

  const takeImage = (file: File | undefined) => file && useWallpaper(file);
  const wallUrl = $derived(app.wallpaper ? `url("${app.wallpaper.url}")` : null);
</script>

<div class="orbit">
  <div class="orbit-main">
    <section class="wheel-wrap" aria-label="Color wheel" bind:this={wrap}>
      <div class="wheel-box" style:--wheel-scale={scale.toFixed(3)}>
        <div
          class="wheel"
          role="slider"
          tabindex="0"
          aria-label="Seed hue"
          aria-valuemin="0"
          aria-valuemax="359"
          aria-valuenow={Math.round(hue)}
          bind:this={wheel}
          onpointerdown={(e) => {
            wheel?.setPointerCapture(e.pointerId);
            moveSeed(hueAt(e));
          }}
          onpointermove={(e) => {
            if (wheel?.hasPointerCapture(e.pointerId)) moveSeed(hueAt(e));
          }}
          onkeydown={wheelKey}
        >
          <div class="wheel-ring"></div>
          <div class="wheel-guide g1"></div>
          <div class="wheel-guide g2"></div>
          <svg class="wheel-lines" viewBox="0 0 {SIZE} {SIZE}" aria-hidden="true">
            {#each lines as [x, y], i (i)}
              <line x1={C} y1={C} x2={x.toFixed(1)} y2={y.toFixed(1)} />
            {/each}
          </svg>
          <div class="wheel-dots" aria-hidden="true">
            {#each dots as d (d.k)}
              <span
                class="wheel-dot"
                title={META[d.k].name}
                style:left="{d.x.toFixed(1)}px"
                style:top="{d.y.toFixed(1)}px"
                style:background={d.c}
              ></span>
              {#if d.label}
                <span
                  class="wheel-label"
                  style:left="{d.label.x.toFixed(1)}px"
                  style:top="{d.label.y.toFixed(1)}px"
                >
                  {META[d.k].short}
                </span>
              {/if}
            {/each}
          </div>
          <span
            class="wheel-seed"
            aria-hidden="true"
            style:left="{seedAt[0]}px"
            style:top="{seedAt[1]}px"
            style:background={o.seed}
          ></span>
          <div class="wheel-center">
            <span class="eyebrow wheel-harmony">{H.name}</span>
            <span class="wheel-title">{H.title}</span>
          </div>
        </div>
      </div>
    </section>
    <aside class="orbit-side">
      <div class="seed">
        <label class="seed-swatch" style:background={o.seed}>
          <span class="sr-only">Seed color</span>
          <input
            type="color"
            class="seed-input"
            {@attach keep(o.seed.toLowerCase())}
            oninput={(e) => setOrbit({ seed: e.currentTarget.value.toUpperCase() })}
          />
        </label>
        <div>
          <span class="eyebrow">Seed</span>
          <span class="seed-hex">{o.seed}</span>
        </div>
      </div>
      <div class="wall-pick">
        <label
          class={['drop', over && 'over']}
          ondragover={(e) => {
            e.preventDefault();
            over = true;
          }}
          ondragleave={() => (over = false)}
          ondrop={(e) => {
            e.preventDefault();
            over = false;
            takeImage(e.dataTransfer?.files[0]);
          }}
        >
          <input
            type="file"
            accept="image/*"
            class="sr-only drop-input"
            onchange={(e) => {
              takeImage(e.currentTarget.files?.[0]);
              e.currentTarget.value = '';
            }}
          />
          <span class="drop-thumb" style:background-image={wallUrl}></span>
          <span class="drop-text">
            <span class="drop-title">
              {app.wallpaper ? app.wallpaper.name : 'From your wallpaper'}
            </span>
            <small hidden={!!app.wallpaper}>
              Drop an image or browse. It becomes the studio's desktop.
            </small>
          </span>
        </label>
        <div class="found" hidden={!o.palette.length && !app.wallpaper}>
          <div class="found-swatches">
            {#each o.palette as c (c)}
              <button
                type="button"
                class="found-swatch"
                aria-label="Use {c}"
                aria-pressed={c === o.seed}
                style:background={c}
                onclick={() => setOrbit({ seed: c })}
              ></button>
            {/each}
          </div>
          <button type="button" class="quiet" hidden={!app.wallpaper} onclick={resetWallpaper}>
            Default wallpaper
          </button>
        </div>
      </div>
      <div class="harmonies" role="group" aria-label="Harmony">
        {#each HARMONIES as h (h.id)}
          <button
            type="button"
            aria-pressed={h.id === o.harmony}
            onclick={() => setOrbit({ harmony: h.id })}
          >
            <span class="pill-dot"></span>
            {h.name}
          </button>
        {/each}
      </div>
      <div class="orbit-sliders">
        {#each SLIDERS as [k, name, min, max] (k)}
          <label class="field">
            <span class="field-name">
              {name}
              <output>{o[k]}</output>
            </span>
            <input
              type="range"
              {min}
              {max}
              style:--p={rangeFill(o[k], min, max)}
              {@attach keep(o[k])}
              oninput={(e) => setOrbit({ [k]: Number(e.currentTarget.value) })}
            />
          </label>
        {/each}
      </div>
      <p class="orbit-score" data-level={score.level}>
        Lowest contrast on the taskbar {score.worst.toFixed(1)}:1
      </p>
      <div class="orbit-actions">
        <button type="button" class="primary" onclick={refine}>Refine in the studio</button>
      </div>
    </aside>
  </div>

  <section class="bands" aria-label="Night and day">
    {#each bands as b (b.band)}
      <div class="band" data-band={b.band}>
        <div class={['band-wall', app.wallpaper && 'custom']} style:background-image={wallUrl}>
          <div class="band-bar">
            {#each b.tiles as t (t.k)}
              <div
                class="tile static"
                style:width="{t.tile.w}px"
                style:background={rgba(t.v.tile, t.v.tileOpacity / 100)}
                style:font-family={fontCss(app.profile.font).replace(/"/g, "'")}
                style:font-size="{app.profile.font.size * 1.25}px"
              >
                <TileArt tile={t.tile} />
              </div>
            {/each}
          </div>
        </div>
        <div class="band-cap">
          <span>{b.band === 'night' ? 'Night' : 'Day'}</span>
          <small>{b.band === 'night' ? 'As written' : 'Derived for a light taskbar'}</small>
        </div>
      </div>
    {/each}
  </section>
</div>
