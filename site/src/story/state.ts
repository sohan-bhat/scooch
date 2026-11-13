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
  frost: 0,
  blinds: 1,
  progress: 0,
};

export type Story = typeof story;

export const pointer = { x: 0, y: 0 };

export const runtime = {
  still: -1,
  hero: false,
  idle: true,
  ready: false,
  power: 0,
  posed: false,
  compiled: false,
  props: false,
  introAt: -1,
  resume: null as (() => void) | null,
};
