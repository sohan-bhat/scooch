import { quiet } from '@/lib/idle';
import { runtime } from '@/story/state';
import { quality } from './quality';
import { still } from './still';

type Connection = { saveData?: boolean; effectiveType?: string };
type Level = 'none' | 'main' | 'all';

export const budget = { stage: 0, busy: true, hero: false, heroAt: 0, steps: 0, lastStepAt: 0 };

const waiting: (() => void)[] = [];

export function stillsPlanned() {
  return quality.stillScale < 0.999 && runtime.still < 0 && !still.off;
}

export function grant(open: boolean) {
  if (!open || waiting.length === 0) return;
  budget.steps += 1;
  budget.lastStepAt = performance.now();
  still.stale = true;
  waiting.shift()?.();
}

export function release() {
  for (const resume of waiting.splice(0)) resume();
}

export function heroReady() {
  if (budget.hero) return;
  performance.mark('scooch:hero');
  budget.hero = true;
  budget.heroAt = performance.now();
}

export function finished() {
  heroReady();
  if (budget.busy) performance.mark('scooch:uploads');
  budget.busy = false;
  if (!runtime.restLoaded) {
    runtime.restLoaded = true;
    window.dispatchEvent(new Event('scooch:rest'));
  }
}

export function network() {
  const connection = (navigator as Navigator & { connection?: Connection }).connection;
  if (!connection) return true;
  if (connection.saveData) return false;
  return !['slow-2g', '2g', '3g'].includes(connection.effectiveType ?? '');
}

export function memory() {
  const reported = (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
  if (reported !== undefined) return reported;
  return !quality.phone && (navigator.hardwareConcurrency ?? 0) >= 8 ? 8 : 4;
}

export function level(): Level {
  if (quality.phone || !network()) return 'none';
  const amount = memory();
  if (amount >= 8) return 'all';
  return amount >= 4 ? 'main' : 'none';
}

const HEADROOM = 600;
const PATIENCE = 4000;

export function posterCovers() {
  return runtime.posterUp && runtime.still < 0;
}

export function covered() {
  return posterCovers() && runtime.ready;
}

export function restHeld() {
  return budget.hero && still.phase === 'still' && !quality.moving;
}

export function fastPath() {
  return (covered() && !budget.hero) || restHeld();
}

const FRAME = 16;
const wakers = new Set<() => void>();

export function pause(slow: number) {
  const fast = posterCovers() || restHeld();
  return new Promise<void>((resolve) => {
    const wake = () => {
      wakers.delete(wake);
      window.clearTimeout(timer);
      resolve();
    };
    const timer = window.setTimeout(wake, fast ? FRAME : slow);
    if (fast) wakers.add(wake);
  });
}

export async function calm() {
  if (fastPath()) {
    if (budget.hero) budget.lastStepAt = performance.now();
    await quiet();
    return;
  }
  for (;;) {
    if (stillsPlanned()) {
      await new Promise<void>((resolve) => waiting.push(resolve));
      if (stillsPlanned()) return;
      continue;
    }
    await quiet();
    const now = performance.now();
    const resting = !quality.moving && quality.chain === 'sharp';
    if (resting && now - quality.slowAt > HEADROOM) return;
    if (resting && now - still.inputAt > PATIENCE) return;
    await new Promise((resolve) => window.setTimeout(resolve, 300));
  }
}

export function advance(to: number) {
  if (to > budget.stage) performance.mark(`scooch:stage-${to}`);
  budget.stage = Math.max(budget.stage, to);
  for (const wake of [...wakers]) wake();
}

export const upgrades = new Set<(wait: () => Promise<void>) => Promise<unknown>>();

export function prefetch(urls: string[]) {
  for (const url of urls) void fetch(url, { priority: 'low' }).then((response) => response.blob()).catch(() => undefined);
}
