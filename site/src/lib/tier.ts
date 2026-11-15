import { getGPUTier, type TierResult } from 'detect-gpu';
import { asset } from './base';

const WAIT = 1500;

let pending: Promise<TierResult | null> | null = null;

export function detectTier() {
  pending ??= Promise.race([
    getGPUTier({ benchmarksURL: asset('benchmarks').replace(/\/$/, '') }).catch(() => null),
    new Promise<null>((resolve) => window.setTimeout(() => resolve(null), WAIT)),
  ]);
  return pending;
}
