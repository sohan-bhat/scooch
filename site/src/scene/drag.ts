import type { WebGLRenderer } from 'three';
import { engineFor } from './engine';
import { disposeLook, lookFromTables, type Frame, type Look, type Tables } from './screen/scenery';

const BUDGET = 5;

export const held = {
  active: false,
  frame: null as Frame | null,
  a: null as Look | null,
  b: null as Look | null,
  s: 1,
  next: null as Tables | null,
  constants: null as Float32Array | null,
  busy: false,
  queued: null as { rects: Float32Array; index: number; final: boolean } | null,
  owned: new Set<Look>(),
  answers: 0,
  started: 0,
  spent: [] as number[],
  lastAt: 0,
  rate: 80,
};

export function begin(frame: Frame, place: number) {
  if (held.active && held.frame === frame) return;
  end();
  const { to } = frame.select(place);
  held.active = true;
  held.frame = frame;
  held.a = frame.states[to];
  held.b = null;
  held.s = 1;
  held.next = null;
  held.lastAt = 0;
}

export async function ask(rects: Float32Array, index: number, final: boolean) {
  const frame = held.frame;
  if (!frame) return;
  if (held.busy) {
    held.queued = { rects, index, final: final || !!held.queued?.final };
    return;
  }
  held.busy = true;
  const start = performance.now();
  try {
    const engine = await engineFor(frame.scenery.info);
    const tables = final ? await engine.drop(rects) : await engine.drag(rects, index, BUDGET);
    if (tables && held.frame === frame) {
      const now = performance.now();
      const gap = held.lastAt ? now - held.lastAt : 35;
      held.lastAt = now;
      held.rate = Math.min(Math.max(3000 / Math.max(gap, 30), 10), 90);
      held.next = tables;
      held.answers += 1;
      held.spent.push(performance.now() - start);
      if (held.spent.length > 60) held.spent.shift();
    }
  } catch {
    held.queued = null;
  } finally {
    held.busy = false;
  }
  const queued = held.queued;
  held.queued = null;
  if (queued && held.frame === frame) void ask(queued.rects, queued.index, queued.final);
}

export function step(delta: number) {
  if (!held.active) return;
  if (held.s < 1) {
    held.s += (1 - held.s) * (1 - Math.exp(-delta * held.rate));
    if (held.s > 0.985) held.s = 1;
  }
  if (held.s >= 1 && held.next) {
    if (held.b) {
      if (held.a && held.owned.has(held.a)) {
        disposeLook(held.a);
        held.owned.delete(held.a);
      }
      held.a = held.b;
    }
    held.b = lookFromTables(held.next);
    held.owned.add(held.b);
    held.constants = held.next.constants;
    held.next = null;
    held.s = 0;
  }
}

export function draw(gl: WebGLRenderer) {
  if (!held.active || !held.frame || !held.a) return false;
  held.frame.renderLooks(gl, held.a, held.b ?? held.a, held.b ? held.s : 0, held.constants);
  return true;
}

export function end() {
  for (const look of held.owned) disposeLook(look);
  held.owned.clear();
  held.active = false;
  held.frame = null;
  held.a = null;
  held.b = null;
  held.next = null;
  held.queued = null;
  held.s = 1;
}
