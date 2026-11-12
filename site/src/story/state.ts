export const story = {
  camera: 0,
  push: 0,
  lamp: 1,
  dof: 0,
  focus: 0,
  sweep: 0,
  mixAB: 0,
  mixBC: 0,
  drag: 0,
  cursorX: 0.62,
  cursorY: 0.72,
  cursorAlpha: 0,
  grab: 0,
  progress: 0,
};

export type Story = typeof story;

export const pointer = { x: 0, y: 0 };

export const runtime = {
  still: -1,
  idle: true,
  ready: false,
  power: 0,
};
