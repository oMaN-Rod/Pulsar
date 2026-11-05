<script lang="ts">
  type Point = [number, number];
  interface Box {
    left: number;
    top: number;
    right: number;
    bottom: number;
    width: number;
    height: number;
  }
  interface Target {
    at: Point;
    pref: Point;
    rect?: Box;
  }

  const TIPS = [
    {
      id: 'overlay',
      lines: ['Hover an item for its popup,', 'click to edit it, drag to reorder'],
      tilt: -2,
    },
    {
      id: 'bar',
      lines: ['Right-click the taskbar for alignment,', 'location and transparency'],
      tilt: 1.5,
    },
    { id: 'apps', lines: ['Hover an app and hit ×', 'to make room for Pulsar'], tilt: -1 },
    { id: 'widget', lines: ['Drag the cards to', 'rearrange the widget'], tilt: 2 },
    { id: 'icons', lines: ['Open, save and share', 'your theme from here'], tilt: -1.5 },
  ];

  const OBSTACLES = '.hero, .panel, .widget, .desk-icons, .taskbar, .float-box, .panel-open';

  let { root }: { root: HTMLElement | undefined } = $props();

  let shown = $state(false);
  let spots = $state<Record<string, { left: number; top: number }>>({});
  let paths = $state<string[]>([]);
  let size = $state({ w: 0, h: 0 });
  const nodes: Record<string, HTMLElement> = {};
  let layer: HTMLElement;
  let signature = '';
  let edge = 'bottom';

  const box = (n: Element | null | undefined): Box | null => {
    if (
      !root ||
      !n?.isConnected ||
      n.closest('[hidden]') ||
      getComputedStyle(n).visibility === 'hidden'
    )
      return null;
    const d = root.getBoundingClientRect();
    const r = n.getBoundingClientRect();
    if (!r.width && !r.height) return null;
    return {
      left: r.left - d.left,
      top: r.top - d.top,
      right: r.right - d.left,
      bottom: r.bottom - d.top,
      width: r.width,
      height: r.height,
    };
  };
  const cx = (r: { left: number; right: number }) => (r.left + r.right) / 2;
  const cy = (r: { top: number; bottom: number }) => (r.top + r.bottom) / 2;
  const intoDesk = (dist: number, along = 0): Point =>
    edge === 'bottom' ? [along, -dist] : edge === 'left' ? [dist, along] : [-dist, along];
  const barSide = (r: Box, inset = 0): Point =>
    edge === 'bottom'
      ? [cx(r), r.top + inset]
      : edge === 'left'
        ? [r.right - inset, cy(r)]
        : [r.left + inset, cy(r)];

  function targets(desk: HTMLElement): Record<string, Target> {
    const bar = box(desk.querySelector('.taskbar'));
    if (!bar) return {};
    const overlay = box(desk.querySelector('.overlay'));
    const apps = [...desk.querySelectorAll('.tb-app')].map(box).filter((b): b is Box => !!b);
    const widget = box(desk.querySelector('.widget'));
    const icon = box(desk.querySelector('[data-export]'));
    const t: Record<string, Target> = {};
    if (overlay) t.overlay = { at: barSide(overlay, -6), pref: intoDesk(150, -110) };
    if (apps.length)
      t.apps = { at: barSide(apps[Math.min(3, apps.length - 1)], -6), pref: intoDesk(190, 40) };

    const vertical = edge !== 'bottom';
    const span = (r: Box): Point => (vertical ? [r.top, r.bottom] : [r.left, r.right]);
    const parts = ['.tb-group', '.taskbar .overlay', '.tb-tray-area']
      .map((s) => box(desk.querySelector(s)))
      .filter((b): b is Box => !!b)
      .map(span)
      .sort((a, b) => a[0] - b[0]);
    const [start, end] = span(bar);
    let best: Point | null = null;
    let from = start;
    for (const [a, b] of [...parts, [end, end]]) {
      if (a - from > (best ? best[1] - best[0] : 0)) best = [from, a];
      from = Math.max(from, b);
    }
    if (best && best[1] - best[0] > 60) {
      const mid = (best[0] + best[1]) / 2;
      const [x, y] = barSide(bar, -6);
      t.bar = { at: vertical ? [x, mid] : [mid, y], pref: intoDesk(120, 0) };
    }
    if (widget)
      t.widget = {
        at: [widget.left - 4, widget.top + Math.min(36, widget.height / 2)],
        pref: [-150, 90],
        rect: widget,
      };
    if (icon) t.icons = { at: [icon.right + 2, cy(icon)], pref: [150, 70], rect: icon };
    return t;
  }

  /** The point on `r`'s edge closest to `b`, just outside it. */
  function nearest(r: Box, b: Omit<Box, 'width' | 'height'>): Point {
    const x = Math.min(Math.max(cx(b), r.left + 12), r.right - 12);
    const y = Math.min(Math.max(cy(b), r.top + 12), r.bottom - 12);
    if (b.right < r.left) return [r.left - 6, y];
    if (b.left > r.right) return [r.right + 6, y];
    return b.top > r.bottom ? [x, r.bottom + 6] : [x, r.top - 6];
  }

  type Rect = { left: number; top: number; right: number; bottom: number };
  const hits = ([x, y]: Point, r: Rect, pad: number) =>
    x > r.left - pad && x < r.right + pad && y > r.top - pad && y < r.bottom + pad;
  const crosses = (p: Point, q: Point, r: Rect, pad: number) => {
    for (let i = 1; i < 24; i++) {
      const f = i / 24;
      if (hits([p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f], r, pad)) return true;
    }
    return false;
  };
  const turn = (a: Point, b: Point, c: Point) =>
    Math.sign((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]));
  const cut = (a: Point, b: Point, c: Point, d: Point) =>
    turn(a, b, c) !== turn(a, b, d) && turn(c, d, a) !== turn(c, d, b);

  /** Spots near the preferred one first, then anywhere on the desktop, nearest first. */
  function* offsets(home: Point, w: number, h: number, W: number, H: number): Generator<Point> {
    yield [0, 0];
    for (let r = 1; r <= 16; r++) {
      const s = r * 22;
      yield* [
        [0, -s],
        [0, s],
        [-s, 0],
        [s, 0],
        [-s, -s],
        [s, -s],
        [-s, s],
        [s, s],
      ] as Point[];
    }
    const grid: Point[] = [];
    for (let y = 76; y + h <= H - 12; y += 20) {
      for (let x = 12; x + w <= W - 12; x += 24) grid.push([x - home[0], y - home[1]]);
    }
    yield* grid.sort((a, b) => Math.hypot(...a) - Math.hypot(...b));
  }

  /** Where an arrow from text box `b` to `at` leaves the box; null when `at` is inside it. */
  function startOf(b: Rect, at: Point): Point | null {
    const sx = Math.min(Math.max(at[0], b.left + 16), b.right - 16);
    const sy = Math.min(Math.max(at[1], b.top + 6), b.bottom - 2);
    if (hits(at, b, 0)) return null;
    if (at[1] > b.bottom) return [sx, b.bottom + 8];
    if (at[1] < b.top) return [sx, b.top - 4];
    return [at[0] < b.left ? b.left - 8 : b.right + 8, sy];
  }

  const overlaps = (a: Rect, b: Rect, pad: number) =>
    a.left < b.right + pad &&
    a.right > b.left - pad &&
    a.top < b.bottom + pad &&
    a.bottom > b.top - pad;

  function arrow([sx, sy]: Point, [tx, ty]: Point, bend: number) {
    const dx = tx - sx;
    const dy = ty - sy;
    const len = Math.hypot(dx, dy) || 1;
    const qx = (sx + tx) / 2 - (dy / len) * len * 0.22 * bend;
    const qy = (sy + ty) / 2 + (dx / len) * len * 0.22 * bend;
    const ex = tx - ((tx - qx) / Math.hypot(tx - qx, ty - qy)) * 3;
    const ey = ty - ((ty - qy) / Math.hypot(tx - qx, ty - qy)) * 3;
    const a = Math.atan2(ey - qy, ex - qx);
    const head = (t: number) =>
      `${(ex - 12 * Math.cos(a + t)).toFixed(1)} ${(ey - 12 * Math.sin(a + t)).toFixed(1)}`;
    return `M${sx.toFixed(1)} ${sy.toFixed(1)}Q${qx.toFixed(1)} ${qy.toFixed(1)} ${ex.toFixed(1)} ${ey.toFixed(1)}M${head(0.5)} L${ex.toFixed(1)} ${ey.toFixed(1)} L${head(-0.5)}`;
  }

  export function reset() {
    signature = '';
  }

  export function draw(show: boolean, nextEdge: string) {
    edge = nextEdge;
    shown = show;
    layer.hidden = !show;
    if (!show || !root) {
      signature = '';
      return;
    }
    const desk = root;
    const t = targets(desk);
    const blocks = [...desk.querySelectorAll(OBSTACLES)]
      .map((n) => {
        const b = box(n);
        return b && { ...b, isIcons: n.classList.contains('desk-icons') };
      })
      .filter((b): b is Box & { isIcons: boolean } => !!b);
    const sig = JSON.stringify([t, blocks, desk.clientWidth, desk.clientHeight]);
    if (sig === signature) return;
    signature = sig;

    const W = desk.clientWidth;
    const H = desk.clientHeight;
    const placed: Rect[] = [];
    const lines: [Point, Point][] = [];
    const nextPaths: string[] = [];
    const nextSpots: Record<string, { left: number; top: number }> = {};
    TIPS.filter((tip) => t[tip.id]).forEach((tip, i) => {
      const node = nodes[tip.id];
      if (!node) return;
      const w = node.offsetWidth;
      const h = node.offsetHeight;
      const { pref, rect } = t[tip.id];
      const home: Point = [t[tip.id].at[0] + pref[0] - w / 2, t[tip.id].at[1] + pref[1] - h / 2];
      let found: { b: Rect; at: Point; start: Point | null } | null = null;
      for (const [ox, oy] of offsets(home, w, h, W, H)) {
        const b = {
          left: home[0] + ox,
          top: home[1] + oy,
          right: home[0] + ox + w,
          bottom: home[1] + oy + h,
        };
        if (b.left < 12 || b.top < 76 || b.right > W - 12 || b.bottom > H - 12) continue;
        if (blocks.some((o) => overlaps(b, o, 14)) || placed.some((o) => overlaps(b, o, 18)))
          continue;
        if (lines.some(([p, q]) => crosses(p, q, b, 10))) continue;
        const at = rect ? nearest(rect, b) : t[tip.id].at;
        const start = startOf(b, at);
        if (start) {
          const walls = blocks.filter((o) => !hits(at, o, 12) && !(edge === 'left' && o.isIcons));
          if (
            walls.some((o) => crosses(start, at, o, 4)) ||
            placed.some((o) => crosses(start, at, o, 8))
          )
            continue;
          if (lines.some(([p, q]) => cut(start, at, p, q))) continue;
        }
        found = { b, at, start };
        break;
      }
      if (!found) return;
      placed.push(found.b);
      nextSpots[tip.id] = { left: found.b.left, top: found.b.top };
      if (found.start) {
        lines.push([found.start, found.at]);
        nextPaths.push(arrow(found.start, found.at, i % 2 ? 1 : -1));
      }
    });
    spots = nextSpots;
    paths = nextPaths;
    size = { w: W, h: H };
  }
</script>

<div class="notes" aria-hidden="true" hidden={!shown} bind:this={layer}>
  <svg class="notes-ink" width={size.w} height={size.h} viewBox="0 0 {size.w} {size.h}">
    {#each paths as d, i (i)}
      <path {d} />
    {/each}
  </svg>
  {#each TIPS as tip (tip.id)}
    <p
      class="note-text"
      {@attach (node) => {
        nodes[tip.id] = node;
        return () => delete nodes[tip.id];
      }}
      style:--tilt="{tip.tilt}deg"
      style:left={spots[tip.id] ? `${spots[tip.id].left}px` : null}
      style:top={spots[tip.id] ? `${spots[tip.id].top}px` : null}
      style:visibility={spots[tip.id] ? null : 'hidden'}
    >
      {tip.lines[0]}
      <br />
      {tip.lines[1]}
    </p>
  {/each}
</div>
