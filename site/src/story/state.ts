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
  posed: false,
  compiled: false,
  props: false,
  scenery: false,
  introAt: null as number | null,
  resume: null as (() => void) | null,
};
