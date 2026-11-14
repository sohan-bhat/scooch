export type Vec3 = [number, number, number];

export type Frame = 'world' | 'glass' | 'space';

export type Key = {
  frame: Frame;
  position: Vec3;
  target: Vec3;
  fov: number;
  shiftX: number;
  shiftY: number;
};

export type Layout = 'desktop' | 'phone';

export const keys: Record<Layout, Key[]> = {
  desktop: [
    { frame: 'world', position: [-0.95, 1.66, 2.1], target: [0.26, 0.9, -0.12], fov: 30, shiftX: 0.48, shiftY: 0.02 },
    { frame: 'world', position: [-0.2, 1.86, 1.62], target: [0.28, 0.92, -0.05], fov: 30, shiftX: 0.12, shiftY: 0 },
    { frame: 'world', position: [0.98, 1.13, 0.9], target: [0.45, 1.02, 0.12], fov: 32, shiftX: -0.42, shiftY: 0 },
    { frame: 'glass', position: [0, 0, 0.38], target: [0, 0, 0], fov: 32, shiftX: 0, shiftY: 0 },
    { frame: 'glass', position: [0, 0.14, 0.95], target: [0, 0.06, 0.0], fov: 34, shiftX: 0.2, shiftY: 0 },
    { frame: 'space', position: [0.04, -1.1, 1.06], target: [0.03, 0.02, 0.0], fov: 34, shiftX: 0.35, shiftY: 0.02 },
    { frame: 'world', position: [0.36, 1.68, 1.62], target: [0.42, 1.1, 0.32], fov: 38, shiftX: 0, shiftY: 0 },
    { frame: 'world', position: [0.58, 1.5, 1.4], target: [0.5, 1.12, 0.3], fov: 44, shiftX: 0, shiftY: 0 },
    { frame: 'space', position: [-0.06, -1.2, 1.14], target: [0.05, 0.02, 0.0], fov: 34, shiftX: 0.36, shiftY: 0.02 },
    { frame: 'world', position: [0.36, 1.56, 1.82], target: [0.48, 1.42, -0.1], fov: 40, shiftX: 0, shiftY: 0 },
    { frame: 'world', position: [0.22, 1.16, 0.62], target: [0.0, 0.96, -0.12], fov: 34, shiftX: 0, shiftY: 0 },
    { frame: 'world', position: [1.0, 1.0, 1.95], target: [1.32, 0.4, 0.3], fov: 40, shiftX: 0, shiftY: 0 },
    { frame: 'world', position: [-1.02, 0.86, 0.3], target: [-0.82, 0.83, -0.08], fov: 30, shiftX: 0, shiftY: 0 },
    { frame: 'world', position: [-0.62, 0.85, 0.16], target: [-0.44, 0.85, -0.26], fov: 30, shiftX: 0, shiftY: 0 },
    { frame: 'world', position: [-0.25, 0.9, 0.02], target: [-0.41, 0.85, -0.27], fov: 30, shiftX: -0.2, shiftY: 0 },
    { frame: 'world', position: [0.5, 2.25, 3.05], target: [0.42, 0.62, -0.05], fov: 40, shiftX: -0.3, shiftY: 0 },
  ],
  phone: [
    { frame: 'world', position: [-0.5, 1.78, 2.15], target: [0.3, 0.93, -0.04], fov: 48, shiftX: 0, shiftY: 0.36 },
    { frame: 'world', position: [0.15, 2.02, 1.75], target: [0.36, 0.96, 0.0], fov: 46, shiftX: 0, shiftY: 0.34 },
    { frame: 'world', position: [0.75, 1.15, 0.9], target: [0.48, 1.03, 0.12], fov: 44, shiftX: 0, shiftY: 0.36 },
    { frame: 'glass', position: [0, -0.02, 0.86], target: [0, -0.02, 0], fov: 44, shiftX: 0, shiftY: 0.06 },
    { frame: 'glass', position: [0, 0.75, 2.6], target: [0, 0.27, 0.05], fov: 46, shiftX: 0, shiftY: 0.06 },
    { frame: 'space', position: [0.0, -1.62, 1.94], target: [0.0, 0.02, 0.0], fov: 46, shiftX: 0, shiftY: -0.1 },
    { frame: 'space', position: [0.2, -1.65, 2.0], target: [0.03, 0.03, 0.0], fov: 46, shiftX: 0, shiftY: -0.1 },
    { frame: 'space', position: [0.2, -1.65, 2.0], target: [0.03, 0.03, 0.0], fov: 46, shiftX: 0, shiftY: -0.1 },
    { frame: 'space', position: [-0.16, -1.66, 1.98], target: [0.0, 0.03, 0.0], fov: 46, shiftX: 0, shiftY: -0.1 },
    { frame: 'space', position: [-0.16, -1.66, 1.98], target: [0.0, 0.03, 0.0], fov: 46, shiftX: 0, shiftY: -0.1 },
    { frame: 'world', position: [0.12, 1.2, 0.78], target: [0.0, 0.95, -0.12], fov: 40, shiftX: 0, shiftY: 0.3 },
    { frame: 'world', position: [1.02, 0.78, 1.72], target: [1.44, 0.26, 0.32], fov: 44, shiftX: 0, shiftY: 0.08 },
    { frame: 'world', position: [-1.05, 0.87, 0.42], target: [-0.41, 0.87, -0.27], fov: 40, shiftX: 0, shiftY: 0.12 },
    { frame: 'world', position: [-0.72, 0.88, 0.36], target: [-0.41, 0.87, -0.27], fov: 40, shiftX: 0, shiftY: 0.12 },
    { frame: 'world', position: [-0.24, 0.93, 0.2], target: [-0.41, 0.86, -0.27], fov: 38, shiftX: 0, shiftY: 0.1 },
    { frame: 'world', position: [1.05, 2.5, 3.55], target: [0.9, 0.5, 0.05], fov: 58, shiftX: 0, shiftY: 0.3 },
  ],
};

export function layoutFor(width: number, height: number): Layout {
  return width < 768 || height > width * 1.05 ? 'phone' : 'desktop';
}
