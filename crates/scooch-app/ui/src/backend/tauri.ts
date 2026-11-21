import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import type { Backend, Monitor, Phase, Picture, Problem, SwitchName, View } from './types';

type Facade = Omit<View, 'phase' | 'monitors'> & { on: boolean; running: boolean; restoring: boolean; monitors: Monitor[] };

const KINDS = /\.(jpe?g|png|heic|heif|webp|tiff?|bmp|avif)$/i;
const listeners = new Set<() => void>();
let busy: Phase | null = null;
let failure: string | null = null;

const shown = (path: string, thumb = false) => `${convertFileSrc(path, 'scooch')}${thumb ? '?thumb' : ''}`;

function tell() {
  for (const listener of listeners) listener();
}

function shaped(raw: Facade): View {
  return {
    system: raw.system,
    phase: busy ?? (raw.on ? 'on' : 'off'),
    pending: raw.pending,
    monitors: raw.monitors.map((monitor) => ({ ...monitor, picture: monitor.picture ? shown(monitor.picture) : null })),
    switches: raw.switches,
    problem: raw.problem,
    flipHelps: raw.flipHelps,
    current: raw.current,
    counted: raw.counted,
    reduceMotion: raw.reduceMotion,
    textScale: raw.textScale,
    failure,
  };
}

function sentence(error: unknown) {
  const text = typeof error === 'string' ? error : error instanceof Error ? error.message : String(error);
  return text.trim() || null;
}

async function attempt(work: () => Promise<unknown>) {
  try {
    await work();
    failure = null;
  } catch (error) {
    failure = sentence(error);
  }
}

async function during(phase: Phase, work: () => Promise<unknown>) {
  busy = phase;
  tell();
  await attempt(work);
  busy = null;
  tell();
}

async function quietly(work: () => Promise<unknown>) {
  await attempt(work);
  tell();
}

export const tauri: Backend = {
  async view() {
    return shaped(await invoke<Facade>('view'));
  },
  async pictures(): Promise<Picture[]> {
    const paths = await invoke<string[]>('pictures');
    return paths.map((path) => ({ path, thumb: shown(path, true) }));
  },
  turnOn: () => during('starting', () => invoke('turn_on')),
  turnOff: () => during('stopping', () => invoke('turn_off')),
  set: (name: SwitchName, on: boolean) => quietly(() => invoke('set', { switch: name, on })),
  use: (path: string) => quietly(() => invoke('use_picture', { path })),
  pick: () => invoke<string | null>('pick'),
  fix: (problem: Problem) => (problem.kind === 'stopped' || problem.kind === 'other' ? during('starting', () => invoke('fix')) : quietly(() => invoke('fix'))),
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
