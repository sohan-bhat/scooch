import { PerformanceMonitor } from '@react-three/drei';
import { useFrame } from '@react-three/fiber';
import { useCallback, useState } from 'react';
import { runtime } from '@/story/state';
import { quality, type GpuClass } from './quality';
import { still } from './still';

const MAX_CHANGES = 3;
const GAP = 4000;
const RISE = 2;

const plan = { next: null as GpuClass | null, lastAt: -1e9, rises: 0 };

function bounds(): [number, number] {
  const rate = 1000 / quality.period;
  return [rate * 0.85, rate * 0.97];
}

function resting() {
  return still.phase === 'still' && !still.refresh && !still.tick && !still.stale;
}

export function Tiering() {
  const [measuring, setMeasuring] = useState(false);
  useFrame(() => {
    const now = performance.now();
    const live = runtime.shown && still.phase === 'live' && !document.hidden && quality.changes < MAX_CHANGES && plan.next === null;
    if (live !== measuring) setMeasuring(live);
    if (plan.next && resting() && now - plan.lastAt > GAP) {
      quality.setLevel(plan.next);
      plan.next = null;
      plan.lastAt = now;
      plan.rises = 0;
    }
  });
  const decline = useCallback(() => {
    plan.rises = 0;
    const next = quality.step(-1);
    if (next) plan.next = next;
  }, []);
  const incline = useCallback(() => {
    plan.rises += 1;
    if (plan.rises < RISE) return;
    const next = quality.step(1);
    if (next) plan.next = next;
  }, []);
  return measuring ? <PerformanceMonitor bounds={bounds} onDecline={decline} onIncline={incline} /> : null;
}
