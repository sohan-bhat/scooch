import { useFrame } from '@react-three/fiber';
import { runtime } from '@/story/state';
import { budget } from './budget';
import { quality, type GpuClass } from './quality';
import { still } from './still';

const MAX_CHANGES = 3;
const GAP = 4000;

const plan = { next: null as GpuClass | null, lastAt: -1e9 };

function resting() {
  return still.phase === 'still' && !still.refresh && !still.tick && !still.stale;
}

export function Tiering() {
  useFrame((_, delta) => {
    const now = performance.now();
    const loaded = runtime.props && runtime.gallery && runtime.painted && runtime.inside && !budget.busy;
    quality.measuring = runtime.shown && loaded && quality.lostAt === 0 && !document.hidden && quality.changes < MAX_CHANGES && plan.next === null && now - plan.lastAt > GAP;
    if (quality.measuring && !quality.timed) {
      if (quality.moving) quality.note(delta * 1000, 'motion');
      else if (still.phase === 'live' && quality.chain === 'sharp' && !quality.hold) quality.note(delta * 1000, 'rest');
    }
    if (quality.fresh && plan.next === null) {
      quality.fresh = false;
      plan.next = quality.decide();
    }
    if (plan.next && resting()) {
      quality.setLevel(plan.next);
      plan.next = null;
      plan.lastAt = now;
    }
  });
  return null;
}
