export const story = {
  camera: 0,
  lamp: 1,
  dof: 0,
  focus: 0,
  sweep: 0,
  relief: 0,
  place: 0,
  drag: 0,
  grab: 0,
  lift: 0,
  inside: 0,
  tubeOn: 1,
  tubeOff: 0,
  blinds: 1,
  osd: 0,
  typed: 0,
  written: 0,
  wall: 0,
  shape: 0,
  fly: 0,
  city: 0,
  drift: 0,
  progress: 0,
};

export type Story = typeof story;

export const pointer = { x: 0, y: 0 };

export const runtime = {
  still: -1,
  hero: false,
  idle: true,
  ready: false,
  shown: false,
  painted: false,
  inside: false,
  posed: false,
  compiled: false,
  props: false,
  scenery: false,
  gallery: false,
  abandoned: false,
  resume: null as (() => void) | null,
  frozen: null as number | null,
  timeShift: 0,
  changedAt: 0,
  settled: true,
  muted: false,
  clockOrigin: Date.now() - performance.now(),
  clockHold: true,
  clockSweepAt: 0,
  posterUp: true,
  restReady: false,
  dropPoster: null as (() => void) | null,
};

export function changed() {
  if (!runtime.muted) runtime.changedAt = performance.now();
}

export function wallClock() {
  return runtime.clockOrigin + performance.now();
}

export const CLOCK_SWEEP = 400;

export function clockSweeping(now = performance.now()) {
  return runtime.clockSweepAt > 0 && now - runtime.clockSweepAt < CLOCK_SWEEP + 50;
}

export function releaseClock() {
  if (!runtime.clockHold) return;
  runtime.clockHold = false;
  runtime.clockSweepAt = window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : performance.now();
  changed();
}
