import { Vector3 } from 'three';

export const DESK_TOP = 0.787;
export const LAPTOP = { position: [0.0, DESK_TOP, -0.06] as const, rotation: -0.14, scale: 0.5, lid: -0.16 };
export const LAMP = { position: [-0.1, DESK_TOP, -0.45] as const, rotation: Math.PI + 0.38 };
export const TELEVISION = { position: [0.5, DESK_TOP, 0.1] as const, rotation: -0.1 };
export const BULB = new Vector3(-0.005, 0.69, -0.177);

export type Placement = {
  model: string;
  node?: string;
  x: number;
  z: number;
  rotation: number;
  lift?: number;
  tilt?: number;
  roll?: number;
  scale?: number;
  phone?: boolean;
};

export const placements: Placement[] = [
  { model: 'notepads', node: 'office_notepads_yellow_pad', x: -0.2, z: 0.13, rotation: 0.26 },
  { model: 'stationery', node: 'stationery_supplies_pencil_used', x: -0.19, z: 0.15, rotation: -0.62, lift: 0.0102 },
  { model: 'notepads', node: 'office_notepads_sticky_stack', x: -0.42, z: 0.03, rotation: -0.35 },
  { model: 'notepads', node: 'office_notepads_note_stack', x: -0.64, z: 0.12, rotation: 0.18, phone: false },
  { model: 'stationery', node: 'stationery_supplies_pencil_new_a', x: -0.05, z: 0.31, rotation: 0.95 },
  { model: 'stationery', node: 'stationery_supplies_eraser', x: -0.36, z: 0.24, rotation: 0.3 },
  { model: 'stationery', node: 'stationery_supplies_pencilcup', x: -0.5, z: -0.27, rotation: 0.4, phone: false },
  { model: 'stationery', node: 'stationery_supplies_pen_fancy', x: 0.2, z: 0.31, rotation: -1.3 },
  { model: 'clock', x: -0.4, z: -0.27, rotation: 0.5 },
  { model: 'plant', x: -0.7, z: -0.3, rotation: 0.9, phone: false },
  { model: 'cassette', x: -0.86, z: -0.08, rotation: 0.55, phone: false },
  { model: 'stapler', x: -0.56, z: 0.22, rotation: 0.62, phone: false },
  { model: 'glasses', x: 0.02, z: 0.3, rotation: 2.7, phone: false },
  { model: 'watch', x: 0.29, z: 0.4, rotation: 1.15, phone: false },
  { model: 'gamepad', x: 0.86, z: 0.26, rotation: -0.45, phone: false },
];
