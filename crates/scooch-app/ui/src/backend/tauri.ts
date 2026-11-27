import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { Backend, Monitor, Phase, Picture, Problem, SwitchName, View } from './types';

type Stamped = Monitor & { stamp: number | null };

type Facade = Omit<View, 'phase' | 'monitors'> & { on: boolean; running: boolean; restoring: boolean; monitors: Stamped[] };

const KINDS = /\.(jpe?g|png|heic|heif|webp|tiff?|bmp|avif)$/i;
const listeners = new Set<() => void>();
let busy: Phase | null = null;
let failure: string | null = null;
let failedWith: View['failedWith'] = null;
let failedOn: boolean | null = null;
let lastOn: boolean | null = null;

const shown = (path: string, query?: string) => `${convertFileSrc(path, 'scooch')}${query ? `?${query}` : ''}`;

function tell() {
  for (const listener of listeners) listener();
}

function shaped(raw: Facade): View {
  if (failure && failedOn !== null && raw.on !== failedOn) {
    failure = null;
    failedWith = null;
    failedOn = null;
  }
  lastOn = raw.on;
  return {
    system: raw.system,
    phase: busy ?? (raw.on ? 'on' : 'off'),
    pending: raw.pending,
    monitors: raw.monitors.map(({ stamp, ...monitor }) => ({ ...monitor, picture: monitor.picture ? shown(monitor.picture, stamp == null ? undefined : `v=${stamp}`) : null })),
    switches: raw.switches,
    problem: raw.problem,
    flipHelps: raw.flipHelps,
    current: raw.current,
    counted: raw.counted,
    reduceMotion: raw.reduceMotion,
    textScale: raw.textScale,
    failure,
    failedWith,
  };
}

function sentence(error: unknown) {
  const text = typeof error === 'string' ? error : error instanceof Error ? error.message : String(error);
  return text.trim() || null;
}

async function attempt(command: View['failedWith'], work: () => Promise<unknown>) {
  try {
    await work();
    failure = null;
    failedWith = null;
    failedOn = null;
  } catch (error) {
    failure = sentence(error);
    failedWith = command;
    failedOn = lastOn;
  }
}

async function during(phase: Phase, command: View['failedWith'], work: () => Promise<unknown>) {
  busy = phase;
  tell();
  await attempt(command, work);
  busy = null;
  tell();
}

async function quietly(command: View['failedWith'], work: () => Promise<unknown>) {
  await attempt(command, work);
  tell();
}

export const tauri: Backend = {
  async view() {
    return shaped(await invoke<Facade>('view'));
  },
  async pictures(): Promise<Picture[]> {
    const paths = await invoke<string[]>('pictures');
    return paths.map((path) => ({ path, thumb: shown(path, 'thumb') }));
  },
  turnOn: () => during('starting', 'turn-on', () => invoke('turn_on')),
  turnOff: () => during('stopping', 'turn-off', () => invoke('turn_off')),
  set: (name: SwitchName, on: boolean) => quietly('other', () => invoke('set', { switch: name, on })),
  use: (path: string) => quietly('other', () => invoke('use_picture', { path })),
  pick: () => invoke<string | null>('pick'),
  fix: (problem: Problem) => (problem.kind === 'stopped' || problem.kind === 'other' ? during('starting', 'fix', () => invoke('fix')) : quietly('fix', () => invoke('fix'))),
  firstOpen: () => invoke<boolean>('first_open'),
  subscribe(listener) {
    listeners.add(listener);
    const stop = listen('changed', listener);
    return () => {
      listeners.delete(listener);
      stop.then((unlisten) => unlisten());
    };
  },
  onDrop(listener) {
    const stop = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === 'enter' || payload.type === 'over') listener(true, null);
      else if (payload.type === 'drop') listener(false, payload.paths.find((path) => KINDS.test(path)) ?? null);
      else listener(false, null);
    });
    return () => {
      stop.then((unlisten) => unlisten());
    };
  },
};
