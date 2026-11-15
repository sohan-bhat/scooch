import { afterFrame, quiet, WAIT } from '@/lib/idle';
import { runtime } from '@/story/state';
import { quality } from './quality';
import { still } from './still';

type Connection = { saveData?: boolean; effectiveType?: string };
type Level = 'none' | 'main' | 'all';

export const budget = { stage: 0, busy: true, steps: 0, lastStepAt: 0 };

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

export function finished() {
  budget.busy = false;
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

function granted(within: number) {
  return new Promise<boolean>((resolve) => {
    const resume = () => {
      window.clearTimeout(timer);
      resolve(true);
    };
    const timer = window.setTimeout(() => {
      const index = waiting.indexOf(resume);
      if (index >= 0) waiting.splice(index, 1);
      resolve(false);
    }, within);
    waiting.push(resume);
  });
}

export async function calm() {
  const until = performance.now() + WAIT;
  for (;;) {
    const left = until - performance.now();
    if (left <= 0) {
      await afterFrame();
      return;
    }
    if (stillsPlanned()) {
      if ((await granted(left)) && stillsPlanned()) return;
      continue;
    }
    const now = performance.now();
    const resting = !quality.moving && quality.chain === 'sharp';
    if (resting && (now - quality.slowAt > HEADROOM || now - still.inputAt > PATIENCE)) {
      await quiet();
      return;
    }
    await new Promise((resolve) => window.setTimeout(resolve, Math.min(50, left)));
  }
}

export function advance(to: number) {
  budget.stage = Math.max(budget.stage, to);
}

export const upgrades = new Set<(wait: () => Promise<void>) => Promise<unknown>>();

export function prefetch(urls: string[]) {
  for (const url of urls) void fetch(url, { priority: 'low' }).then((response) => response.blob()).catch(() => undefined);
}
