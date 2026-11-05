// A mostly idle PC with bursts of load that temperatures, clocks and power trail behind.
import type { Profile } from './profile.ts';
import type { ItemKey, Unit } from './theme.ts';

const HISTORY = 40;
const THREADS = 16;
const P_THREADS = 12;

type Units = Profile['units'];

let UNITS: Units = { temperature: 'celsius', network: 'bytes', memory: 'percent' };

/** Text follows Pulsar's unit settings; graphs and load colors never change. */
export function setUnits(units: Units) {
  UNITS = units;
}

const temp = (c: number) =>
  UNITS.temperature === 'fahrenheit' ? `${Math.round((c * 9) / 5 + 32)}°F` : `${Math.round(c)}°C`;
const memory = (pct: number, totalGb: number) =>
  UNITS.memory === 'bytes' ? `${((pct / 100) * totalGb).toFixed(1)} GB` : `${Math.round(pct)}%`;

interface Spec {
  min?: number;
  max: number;
  fmt: (v: number) => string;
  unit?: Unit;
}

const SPECS: Record<ItemKey, Spec> = {
  cpu: { max: 100, fmt: (v) => `${Math.round(v)}%` },
  cpu_temp: { min: 30, max: 100, fmt: temp, unit: 'celsius' },
  cpu_power: { max: 120, fmt: (v) => `${Math.round(v)} W` },
  cpu_clock: { min: 0.8, max: 5.4, fmt: (v) => `${v.toFixed(2)} GHz` },
  ram: { max: 100, fmt: (v) => memory(v, 31.8) },
  chipset_temp: { min: 30, max: 90, fmt: temp, unit: 'celsius' },
  gpu: { max: 100, fmt: (v) => `${Math.round(v)}%` },
  gpu_temp: { min: 30, max: 95, fmt: temp, unit: 'celsius' },
  gpu_memory: { max: 100, fmt: (v) => memory(v, 8) },
  gpu_power: { max: 320, fmt: (v) => `${Math.round(v)} W` },
  disk: { max: 100, fmt: (v) => `${Math.round(v)}%` },
  disk_temp: { min: 25, max: 75, fmt: temp, unit: 'celsius' },
  network: { max: 4.5e6, fmt: (v) => rate(v) },
  ping: { max: 120, fmt: (v) => `${Math.round(v)} ms` },
  battery: { max: 100, fmt: (v) => `${Math.round(v)}%` },
  fps: { max: 240, fmt: (v) => `${Math.round(v)} fps` },
};

const SPEC_KEYS = Object.keys(SPECS) as ItemKey[];

export function rate(bps: number): string {
  if (UNITS.network === 'bits') {
    const b = bps * 8;
    if (b >= 1e6) return `${(b / 1e6).toFixed(1)} Mb/s`;
    if (b >= 1e3) return `${(b / 1e3).toFixed(1)} kb/s`;
    return `${Math.round(b)} b/s`;
  }
  if (bps >= 1024 * 1024) return `${(bps / 1024 / 1024).toFixed(1)} MB/s`;
  if (bps >= 1024) return `${(bps / 1024).toFixed(1)} KB/s`;
  return `${Math.round(bps)} B/s`;
}

const gb = (b: number) => `${(b / 1024 ** 3).toFixed(1)} GB`;

const PROCESSES = [
  'Code',
  'dwm',
  'msedge',
  'Discord',
  'explorer',
  'System',
  'WindowsTerminal',
  'Spotify',
  'MsMpEng',
  'steam',
];

export type Row = [string, string];

export interface Details {
  rows: Row[];
  cores?: boolean;
  processes?: Row[];
  more?: Row[];
  sections?: { title: string; rows: Row[] }[];
}

export interface Level {
  now: number;
  low: number;
  high: number;
}

export interface NetText {
  down: string;
  up: string;
}

export interface Sim {
  step(): void;
  history(k: ItemKey): number[];
  rawHistory(k: ItemKey): number[];
  upHistory(): number[];
  raw(k: ItemKey): number;
  cores(): number[];
  pThreads: number;
  /** 0–1 positions of the latest value and the recent low and high. */
  level(k: ItemKey): Level;
  text(k: ItemKey): string;
  net(): NetText;
  /** `rows`, `cores` and `processes` are standard; `more` and `sections` only detailed. */
  details(k: ItemKey): Details;
}

export function createSim(seed = 7): Sim {
  let s = seed;
  const rand = () => {
    s = (s * 16807) % 2147483647;
    return (s - 1) / 2147483646;
  };
  const toward = (v: number, target: number, k: number) => v + (target - v) * k;

  const st = {
    load: 0.1,
    burst: 0,
    gpuLoad: 0.08,
    gaming: 0,
    cpu: 9,
    cpu_temp: 41,
    cpu_power: 18,
    cpu_clock: 3.1,
    ram: 58,
    chipset_temp: 47,
    gpu: 6,
    gpu_temp: 44,
    gpu_memory: 38,
    gpu_power: 22,
    disk: 1,
    disk_temp: 39,
    network: 80e3,
    up: 4e3,
    ping: 21,
    battery: 87,
    fps: 144,
    received: 1.7e6,
    sent: 0.9e6,
    uptime: 126000,
  };
  const cores = Array.from({ length: THREADS }, () => 0.05);
  const history = Object.fromEntries(SPEC_KEYS.map((k) => [k, []])) as unknown as Record<
    ItemKey,
    number[]
  >;
  const raw = Object.fromEntries(SPEC_KEYS.map((k) => [k, []])) as unknown as Record<
    ItemKey,
    number[]
  >;
  const upHistory: number[] = [];
  const procs = PROCESSES.map((name) => ({ name, cpu: 0, gpu: 0 }));

  function step() {
    if (st.burst <= 0 && rand() < 0.08) st.burst = 3 + Math.floor(rand() * 8);
    if (st.gaming <= 0 && rand() < 0.03) st.gaming = 10 + Math.floor(rand() * 15);
    const busy = st.burst > 0;
    const gaming = st.gaming > 0;
    st.burst--;
    st.gaming--;

    st.load = toward(st.load, busy ? 0.45 + rand() * 0.45 : 0.05 + rand() * 0.1, 0.55);
    st.gpuLoad = toward(st.gpuLoad, gaming ? 0.75 + rand() * 0.2 : 0.03 + rand() * 0.08, 0.35);
    for (let i = 0; i < THREADS; i++) {
      const target = st.load * (i < P_THREADS ? 1.15 : 0.6) * (0.4 + rand() * 1.2);
      cores[i] = Math.max(0, Math.min(1, toward(cores[i], target, 0.6)));
    }

    st.cpu = (cores.reduce((a, b) => a + b, 0) / THREADS) * 100;
    st.cpu_clock = toward(st.cpu_clock, 2.2 + st.load * 3 + rand() * 0.2, 0.6);
    st.cpu_power = toward(st.cpu_power, 9 + st.load * 95, 0.5);
    st.cpu_temp = toward(st.cpu_temp, 38 + st.load * 48, 0.18);
    st.chipset_temp = toward(st.chipset_temp, 45 + st.load * 10, 0.05);
    st.ram = toward(st.ram, 55 + st.load * 12 + st.gpuLoad * 8 + rand() * 2, 0.1);
    st.gpu = st.gpuLoad * 100;
    st.gpu_temp = toward(st.gpu_temp, 40 + st.gpuLoad * 36, 0.12);
    st.gpu_memory = toward(st.gpu_memory, 30 + st.gpuLoad * 45, 0.15);
    st.gpu_power = toward(st.gpu_power, 15 + st.gpuLoad * 260, 0.4);
    st.disk = rand() < (busy ? 0.5 : 0.12) ? 5 + rand() * 60 : rand() * 2;
    st.disk_temp = toward(st.disk_temp, 37 + st.disk / 12, 0.05);
    st.network = rand() < 0.15 ? 2e5 + rand() * 4e6 : 2e4 + rand() * 1.2e5;
    st.up = rand() < 0.1 ? 1e5 + rand() * 6e5 : 2e3 + rand() * 1.5e4;
    st.ping = toward(st.ping, 18 + rand() * 10 + (rand() < 0.05 ? 40 : 0), 0.7);
    st.battery = Math.max(20, st.battery - 0.01);
    st.fps = gaming ? toward(st.fps, 120 + rand() * 50, 0.5) : toward(st.fps, 60, 0.3);
    st.received += st.network;
    st.sent += st.up;
    st.uptime += 1;
    for (const p of procs) {
      p.cpu = toward(p.cpu, rand() < 0.3 ? rand() * st.cpu * 0.5 : 0, 0.5);
      p.gpu = toward(p.gpu, rand() < 0.2 ? rand() * st.gpu * 0.6 : 0, 0.5);
    }

    for (const k of SPEC_KEYS) {
      const spec = SPECS[k];
      const lo = spec.min ?? 0;
      push(history[k], (st[k] - lo) / (spec.max - lo));
      push(raw[k], st[k], true);
    }
    push(upHistory, st.up / SPECS.network.max);
  }

  for (let i = 0; i < HISTORY; i++) step();

  const stats = (k: ItemKey) => {
    const vals = raw[k];
    const f = SPECS[k].fmt;
    return `${f(Math.min(...vals))} / ${f(vals.reduce((a, b) => a + b, 0) / vals.length)} / ${f(Math.max(...vals))}`;
  };
  const top = (field: 'cpu' | 'gpu'): Row[] =>
    [...procs].sort((a, b) => b[field] - a[field]).map((p) => [p.name, `${Math.round(p[field])}%`]);

  return {
    step,
    history: (k) => history[k],
    rawHistory: (k) => raw[k],
    upHistory: () => upHistory,
    raw: (k) => st[k],
    cores: () => cores,
    pThreads: P_THREADS,
    level(k) {
      const h = history[k];
      return { now: h.at(-1) ?? 0, low: Math.min(...h), high: Math.max(...h) };
    },
    text: (k) => (k === 'network' ? rate(st.network) : SPECS[k].fmt(st[k])),
    net: () => ({ down: rate(st.network), up: rate(st.up) }),
    details(k) {
      const temps: Partial<Record<ItemKey, string>> = {
        cpu_temp: 'CPU package',
        gpu_temp: 'GPU core',
        chipset_temp: 'Chipset',
        disk_temp: 'Drive',
      };
      const hot = st.cpu_temp + 3 + (st.load > 0.3 ? 4 : 0);
      switch (k) {
        case 'cpu':
          return {
            rows: [
              ['Speed', `${st.cpu_clock.toFixed(2)} GHz`],
              ['Temperature', temp(st.cpu_temp)],
              ['Power', `${st.cpu_power.toFixed(1)} W`],
              ['Hottest core', temp(hot)],
              ['Thermal throttling', 'No'],
            ],
            cores: true,
            processes: top('cpu'),
            more: [['Min / avg / max', stats('cpu')]],
            sections: [
              {
                title: 'System',
                rows: [
                  ['Processes', '307'],
                  ['Threads', '5066'],
                  ['Handles', '165907'],
                  ['Up time', uptime(st.uptime)],
                ],
              },
            ],
          };
        case 'ram': {
          const total = 31.8 * 1024 ** 3;
          const used = (total * st.ram) / 100;
          return {
            rows: [
              ['In use', `${gb(used)} of ${gb(total)}`],
              ['Available', gb(total - used)],
              ['Committed', `${gb(used * 1.4)} of ${gb(total * 1.7)}`],
            ],
            processes: top('cpu').map(([n], i) => [
              n,
              `${Math.max(0.1, 1.2 - i * 0.12).toFixed(1)} GB`,
            ]),
            more: [['Min / avg / max', stats('ram')]],
            sections: [
              {
                title: 'Composition',
                rows: [
                  ['Standby', gb(total * 0.35)],
                  ['Modified', '215 MB'],
                  ['Free', gb(total * 0.07)],
                  ['Paged pool', '1.8 GB'],
                  ['Non-paged pool', '1.3 GB'],
                ],
              },
            ],
          };
        }
        case 'gpu':
        case 'gpu_memory':
          return {
            rows: [
              ['Temperature', temp(st.gpu_temp)],
              ['Min / avg / max', stats(k)],
            ],
            processes: top('gpu'),
            sections: [
              {
                title: 'Memory',
                rows: [
                  ['Dedicated', gb((8 * 1024 ** 3 * st.gpu_memory) / 100)],
                  ['Shared', '237 MB'],
                ],
              },
              {
                title: 'Engines',
                rows: [
                  ['3D', `${Math.round(st.gpu)}%`],
                  ['Video decode', `${Math.round(st.gpu * 0.3)}%`],
                  ['Video encode', '0%'],
                  ['Copy', '0%'],
                ],
              },
              {
                title: 'Sensors',
                rows: [
                  ['Fan', `${Math.round(st.gpuLoad * 1900)} RPM`],
                  ['Power', `${Math.round(st.gpu_power)} W`],
                  ['Memory clock', '0.41 GHz'],
                ],
              },
            ],
          };
        case 'disk':
          return {
            rows: [
              ['Read', rate(st.disk * 2.4e6)],
              ['Write', rate(st.disk * 0.9e6)],
              ['Min / avg / max', stats('disk')],
            ],
            processes: top('cpu')
              .slice(0, 3)
              .map(([n]) => [n, rate(st.disk * 3e5)]),
            sections: [
              {
                title: 'Drives',
                rows: [
                  ['C:', '203 GB free of 931 GB'],
                  ['D:', '258 GB free of 932 GB'],
                ],
              },
            ],
          };
        case 'network':
          return {
            rows: [],
            sections: [
              { title: 'Adapters', rows: [['Ethernet', `↓ ${rate(st.network)} ↑ ${rate(st.up)}`]] },
              {
                title: 'Since Pulsar started',
                rows: [
                  ['Received', `${(st.received / 1024 ** 2).toFixed(1)} MB`],
                  ['Sent', `${(st.sent / 1024 ** 2).toFixed(1)} MB`],
                ],
              },
            ],
          };
        case 'ping':
          return {
            rows: [
              ['Host', '1.1.1.1'],
              ['Min / avg / max', stats('ping')],
              ['Lost', '0 of 40'],
            ],
          };
        case 'battery':
          return {
            rows: [
              ['State', 'Discharging'],
              ['Time left', '3 h 12 min'],
              ['Rate', '9.8 W'],
            ],
            sections: [
              {
                title: 'Health',
                rows: [
                  ['Capacity', '86%'],
                  ['Cycles', '214'],
                ],
              },
            ],
          };
        case 'fps':
          return {
            rows: [
              ['App', 'Game.exe'],
              ['Frame time', `${(1000 / st.fps).toFixed(1)} ms`],
              ['Min / avg / max', stats('fps')],
            ],
          };
        default: {
          const sensor = temps[k];
          if (sensor)
            return {
              rows: [
                ['Sensor', sensor],
                ['Min / avg / max', stats(k)],
              ],
              cores: k === 'cpu_temp',
            };
          return { rows: [['Min / avg / max', stats(k)]] };
        }
      }
    },
  };
}

function uptime(secs: number) {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  return `${d} d ${h} h`;
}

function push(arr: number[], v: number, rawValue = false) {
  arr.push(rawValue ? v : Math.max(0, Math.min(1, v)));
  if (arr.length > HISTORY) arr.shift();
}

/** `top` is the share of the height left clear for the tile's label. */
export function graph(values: number[], w: number, h: number, top = 0.4) {
  if (!values.length) return { area: '', line: '' };
  const n = values.length;
  const span = h * (1 - top);
  const pts = values.map(
    (v, i) => `${((i * w) / Math.max(1, n - 1)).toFixed(1)},${(h - 1 - v * span).toFixed(1)}`,
  );
  return { line: pts.join(' '), area: `M0,${h} L${pts.join(' L')} L${w},${h} Z` };
}
