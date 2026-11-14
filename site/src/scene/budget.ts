import { quiet } from '@/lib/idle';
import { quality } from './quality';

type Connection = { saveData?: boolean; effectiveType?: string };
type Level = 'none' | 'main' | 'all';

export const budget = { stage: 0 };

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

export async function calm() {
  for (;;) {
    await quiet();
    if (!quality.moving && quality.chain === 'sharp') return;
    await new Promise((resolve) => window.setTimeout(resolve, 300));
  }
}

export function advance(to: number) {
  budget.stage = Math.max(budget.stage, to);
}
