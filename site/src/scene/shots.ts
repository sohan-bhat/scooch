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
    { frame: 'world', position: [-0.95, 1.66, 2.1], target: [0.26, 0.9, -0.12], fov: 30, shiftX: 0.4, shiftY: 0.02 },
    { frame: 'world', position: [-0.2, 1.86, 1.62], target: [0.28, 0.92, -0.05], fov: 30, shiftX: 0.12, shiftY: 0 },
    { frame: 'world', position: [0.98, 1.13, 0.9], target: [0.45, 1.02, 0.12], fov: 32, shiftX: -0.42, shiftY: 0 },
    { frame: 'glass', position: [0, 0, 0.38], target: [0, 0, 0], fov: 32, shiftX: 0, shiftY: 0 },
    { frame: 'glass', position: [0, 0.36, 0.86], target: [0, 0.27, 0.05], fov: 34, shiftX: 0.2, shiftY: 0 },
    { frame: 'space', position: [0.04, -1.0, 0.96], target: [0.02, 0.02, 0.0], fov: 34, shiftX: 0.38, shiftY: 0.02 },
    { frame: 'space', position: [0.2, -1.06, 1.0], target: [0.04, 0.02, 0.0], fov: 34, shiftX: 0.37, shiftY: 0.02 },
    { frame: 'space', position: [-0.06, -1.2, 1.14], target: [0.05, 0.02, 0.0], fov: 34, shiftX: 0.36, shiftY: 0.02 },
    { frame: 'world', position: [-1.4, 0.9, 0.78], target: [0.3, 0.95, -0.12], fov: 30, shiftX: -0.46, shiftY: 0 },
    { frame: 'world', position: [-0.05, 2.6, 2.5], target: [0.08, 0.85, -0.12], fov: 33, shiftX: -0.44, shiftY: 0 },
  ],
  phone: [
    { frame: 'world', position: [-0.5, 1.78, 2.15], target: [0.3, 0.93, -0.04], fov: 48, shiftX: 0, shiftY: 0.36 },
    { frame: 'world', position: [0.15, 2.02, 1.75], target: [0.36, 0.96, 0.0], fov: 46, shiftX: 0, shiftY: 0.34 },
    { frame: 'world', position: [0.75, 1.15, 0.9], target: [0.48, 1.03, 0.12], fov: 44, shiftX: 0, shiftY: 0.36 },
    { frame: 'glass', position: [0, -0.02, 0.5], target: [0, -0.02, 0], fov: 44, shiftX: 0, shiftY: 0.4 },
    { frame: 'glass', position: [0, 0.5, 1.45], target: [0, 0.27, 0.05], fov: 46, shiftX: 0, shiftY: 0.38 },
    { frame: 'space', position: [0.0, -1.5, 1.8], target: [0.0, 0.02, 0.0], fov: 46, shiftX: 0, shiftY: 0.34 },
    { frame: 'space', position: [0.3, -1.42, 1.72], target: [0.03, 0.03, 0.0], fov: 46, shiftX: 0, shiftY: 0.34 },
    { frame: 'space', position: [-0.26, -1.4, 1.68], target: [0.0, 0.03, 0.0], fov: 46, shiftX: 0, shiftY: 0.34 },
    { frame: 'world', position: [-1.0, 0.98, 1.35], target: [0.32, 0.95, -0.04], fov: 46, shiftX: 0, shiftY: 0.36 },
    { frame: 'world', position: [0.25, 2.65, 2.45], target: [0.25, 0.85, -0.05], fov: 48, shiftX: 0, shiftY: 0.36 },
  ],
};

export function layoutFor(width: number, height: number): Layout {
  return width < 768 || height > width * 1.05 ? 'phone' : 'desktop';
}
