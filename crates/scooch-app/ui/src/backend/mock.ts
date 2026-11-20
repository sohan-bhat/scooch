import { build, photoPath, photos, relit } from './scenarios';
import type { Backend, Picture, Problem, SwitchName, View } from './types';

const query = new URLSearchParams(window.location.search);
const name = query.get('state') ?? 'mac-on';
let view: View = build(name);
const listeners = new Set<() => void>();
const drops = new Set<(hovering: boolean, path: string | null) => void>();
const pause = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));

function change(next: Partial<View>) {
  view = { ...view, ...next };
  for (const listener of listeners) listener();
}

export const mock: Backend = {
  async view() {
    return view;
  },
  async pictures(): Promise<Picture[]> {
    return photos.map((photo) => ({ path: photoPath(photo), thumb: `thumbs/${photo}.jpg` }));
  },
  async turnOn() {
    change({ phase: 'starting', problem: null, pending: 0 });
    await pause(900);
    change({ phase: 'on', monitors: relit(view, name, true) });
  },
  async turnOff() {
    change({ phase: 'stopping', flipHelps: false });
    await pause(700);
    change({ phase: 'off', monitors: relit(view, name, false) });
  },
  async set(switchName: SwitchName, on: boolean) {
    change({ switches: { ...view.switches, [switchName]: on }, flipHelps: switchName === 'flip' && on ? false : view.flipHelps });
  },
  async use(path: string) {
    change({ current: path, problem: null });
    const on = view.phase !== 'off';
    change({ monitors: relit(view, name, on) });
  },
  async pick() {
    return photoPath('balloon-sky');
  },
  async fix(problem: Problem) {
    if (problem.kind === 'stopped') return mock.turnOn();
    if (problem.kind === 'move' || problem.kind === 'unknown') return change({ problem: null });
    change({ problem: null, monitors: relit(view, name, view.phase !== 'off') });
  },
  subscribe(listener) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  onDrop(listener) {
    drops.add(listener);
    if (query.has('drop')) window.setTimeout(() => listener(true, null), 0);
    return () => drops.delete(listener);
  },
};
