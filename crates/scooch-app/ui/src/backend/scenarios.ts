import layouts from '../../mock/layouts.json';
import type { Monitor, Places, Problem, Rect, Switches, System, View } from './types';

type Layout = { width: number; height: number; icons: number[][] };

const rects = (layout: Layout): Places => ({ width: layout.width, height: layout.height, icons: layout.icons.map(([x, y, w, h]) => ({ x, y, w, h })) });
const bare = (layout: Layout): Places => ({ width: layout.width, height: layout.height, icons: [] });
const frame = (layout: Layout, x = 0, y = 0): Rect => ({ x, y, w: layout.width, h: layout.height });

export const photoPath = (photo: string) => `/Users/ada/Pictures/${photo}.jpg`;
export const photoOf = (path: string) => path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, '') ?? path;

export const photos = [
  'seagull-sky',
  'golden-tree',
  'lone-canoe',
  'great-wave',
  'balloon-sky',
  'two-swans',
  'fox',
  'moon-sky',
  'astronaut',
  'beach-sitter',
  'city-night',
  'frosty-tree',
  'tree-field',
  'dragonfly',
  'bamboo',
  'glass-cubes',
  'pearl-earring',
  'crown-portrait',
];

type Desk = { system: System; current: string; monitors: (on: boolean) => Monitor[] };

const mac: Desk = {
  system: 'macos',
  current: 'seagull-sky',
  monitors: (on) => [{ id: 'main', frame: frame(layouts.mac), picture: on ? 'drawn/seagull-sky-mac.jpg' : 'drawn/seagull-sky-mac-plain.jpg', places: on ? rects(layouts.mac) : null }],
};

const macTwo: Desk = {
  system: 'macos',
  current: 'seagull-sky',
  monitors: (on) => [
    ...mac.monitors(on),
    { id: 'side', frame: frame(layouts.wide, -layouts.wide.width, -236), picture: on ? 'drawn/seagull-sky-wide.jpg' : 'drawn/seagull-sky-wide-plain.jpg', places: on ? bare(layouts.wide) : null },
  ],
};

const windows: Desk = {
  system: 'windows',
  current: 'golden-tree',
  monitors: (on) => [{ id: 'main', frame: frame(layouts.win), picture: on ? 'drawn/golden-tree-win.jpg' : 'drawn/golden-tree-win-plain.jpg', places: rects(layouts.win) }],
};

const gnome: Desk = {
  system: 'linux',
  current: 'lone-canoe',
  monitors: (on) => {
    const picture = on ? 'drawn/lone-canoe-gnome.jpg' : 'drawn/lone-canoe-gnome-plain.jpg';
    return [
      { id: 'DP-1', frame: frame(layouts.gnome), picture, places: rects(layouts.gnome) },
      { id: 'HDMI-1', frame: frame(layouts.gnome, layouts.gnome.width, 0), picture, places: null },
    ];
  },
};

const xfce: Desk = {
  system: 'linux',
  current: 'great-wave',
  monitors: (on) => [{ id: 'eDP-1', frame: frame(layouts.xfce), picture: on ? 'drawn/great-wave-xfce.jpg' : 'drawn/great-wave-xfce-plain.jpg', places: rects(layouts.xfce) }],
};

const switchesFor = (system: System, extra: { xfce?: boolean; reduced?: boolean } = {}): Switches => {
  const glide = system !== 'linux' && !extra.reduced ? { glide: true } : {};
  const predict = extra.xfce ? { predict: true } : {};
  return { ...glide, scenery: true, flip: false, ...predict };
};

type Scenario = {
  desk: Desk;
  phase?: View['phase'];
  pending?: number;
  problem?: Problem;
  flipHelps?: boolean;
  textScale?: number;
  xfce?: boolean;
  reduced?: boolean;
  blank?: boolean;
  hideIcons?: boolean;
  none?: boolean;
  failure?: string;
};

export const scenarios: Record<string, Scenario> = {
  'mac-on': { desk: mac },
  'mac-two': { desk: macTwo },
  'mac-off': { desk: mac, phase: 'off' },
  'mac-pending': { desk: macTwo, phase: 'off', pending: 2 },
  'mac-starting': { desk: mac, phase: 'starting' },
  'mac-flip': { desk: mac, flipHelps: true },
  'mac-reduced': { desk: mac, reduced: true },
  'mac-automation': { desk: mac, problem: { kind: 'automation' }, hideIcons: true },
  'mac-asking': { desk: mac, problem: { kind: 'asking' }, hideIcons: true },
  'mac-finder': { desk: mac, problem: { kind: 'finder' }, hideIcons: true },
  'mac-move': { desk: mac, phase: 'off', problem: { kind: 'move' } },
  'mac-slideshow': { desk: mac, problem: { kind: 'wallpaper', what: 'slideshow' }, blank: true },
  'mac-moving': { desk: mac, problem: { kind: 'wallpaper', what: 'moving' }, blank: true },
  'mac-stopped': { desk: mac, problem: { kind: 'stopped' } },
  'mac-other': { desk: mac, problem: { kind: 'other' } },
  'mac-large-text': { desk: mac, textScale: 1.3 },
  'win-on': { desk: windows },
  'win-off': { desk: windows, phase: 'off' },
  'win-video': { desk: windows, problem: { kind: 'wallpaper', what: 'video' }, blank: true },
  'win-admin': { desk: windows, problem: { kind: 'admin' } },
  'win-startup': { desk: windows, problem: { kind: 'startup' } },
  'win-text-150': { desk: windows, textScale: 1.5 },
  'win-text-200': { desk: windows, textScale: 2 },
  'win-text-200-problem': { desk: windows, textScale: 2, problem: { kind: 'startup' } },
  'gnome-two': { desk: gnome },
  'gnome-icons': { desk: gnome, problem: { kind: 'gnome', icons: 'off' }, hideIcons: true },
  'gnome-missing': { desk: gnome, problem: { kind: 'gnome', icons: 'missing' }, hideIcons: true },
  'gnome-blocked': { desk: gnome, problem: { kind: 'gnome', icons: 'blocked' }, hideIcons: true },
  'mac-move-failed': { desk: mac, phase: 'off', problem: { kind: 'move' }, failure: 'move' },
  'gnome-color': { desk: gnome, problem: { kind: 'wallpaper', what: 'color' }, blank: true },
  'linux-unknown': { desk: gnome, phase: 'off', problem: { kind: 'unknown', desktop: 'Budgie' }, none: true },
  'xfce-on': { desk: xfce, xfce: true },
  'xfce-off': { desk: xfce, xfce: true, phase: 'off' },
  'no-screen': { desk: mac, problem: { kind: 'screen' }, none: true },
};

export function counted(monitors: Monitor[]) {
  return { icons: monitors.reduce((sum, monitor) => sum + (monitor.places?.icons.length ?? 0), 0), screens: new Set(monitors.map((monitor) => monitor.picture)).size };
}

export function build(name: string): View {
  const scenario = scenarios[name] ?? scenarios['mac-on'];
  const phase = scenario.phase ?? 'on';
  const lit = phase === 'on' || phase === 'stopping';
  let monitors = scenario.desk.monitors(lit);
  if (scenario.hideIcons) monitors = monitors.map((monitor) => ({ ...monitor, places: null }));
  if (scenario.blank) monitors = monitors.map((monitor) => ({ ...monitor, picture: null, places: null }));
  if (scenario.none) monitors = [];
  return {
    system: scenario.desk.system,
    phase,
    pending: scenario.pending ?? 0,
    monitors,
    switches: switchesFor(scenario.desk.system, scenario),
    problem: scenario.problem ?? null,
    flipHelps: scenario.flipHelps ?? false,
    current: scenario.blank || scenario.none ? null : photoPath(scenario.desk.current),
    counted: phase === 'on' && !scenario.hideIcons && !scenario.blank && !scenario.none ? counted(monitors) : null,
    reduceMotion: scenario.reduced ?? false,
    textScale: scenario.textScale ?? 1,
    failure: scenario.failure ?? null,
  };
}

export function relit(view: View, name: string, on: boolean): Monitor[] {
  const scenario = scenarios[name] ?? scenarios['mac-on'];
  return scenario.desk.monitors(on).map((monitor) => ({ ...monitor, picture: !view.current || photoOf(view.current) === scenario.desk.current ? monitor.picture : `thumbs/${photoOf(view.current)}.jpg` }));
}
