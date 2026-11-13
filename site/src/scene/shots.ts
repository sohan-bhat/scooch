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
    { frame: 'world', position: [-1.2, 1.52, 1.95], target: [0.12, 0.9, -0.12], fov: 30, shiftX: 0.3, shiftY: 0 },
    { frame: 'world', position: [-0.2, 1.86, 1.62], target: [0.28, 0.92, -0.05], fov: 30, shiftX: 0.12, shiftY: 0 },
    { frame: 'world', position: [0.95, 1.12, 0.86], target: [0.44, 1.02, 0.12], fov: 32, shiftX: -0.3, shiftY: 0 },
    { frame: 'glass', position: [0, 0, 0.38], target: [0, 0, 0], fov: 32, shiftX: 0, shiftY: 0 },
    { frame: 'space', position: [0.0, -0.62, 0.62], target: [0.0, 0.02, 0.0], fov: 34, shiftX: 0.3, shiftY: 0.02 },
    { frame: 'space', position: [0.34, -0.55, 0.55], target: [0.04, 0.04, 0.0], fov: 34, shiftX: 0.3, shiftY: 0.02 },
    { frame: 'space', position: [-0.28, -0.52, 0.5], target: [0.02, 0.04, 0.0], fov: 34, shiftX: 0.3, shiftY: 0.02 },
    { frame: 'world', position: [-1.35, 0.9, 0.8], target: [0.35, 0.95, -0.1], fov: 30, shiftX: -0.3, shiftY: 0 },
    { frame: 'world', position: [0.1, 2.55, 2.45], target: [0.12, 0.85, -0.1], fov: 33, shiftX: -0.3, shiftY: 0 },
  ],
  phone: [
    { frame: 'world', position: [-0.75, 1.62, 1.55], target: [0.2, 0.92, -0.02], fov: 48, shiftX: 0, shiftY: 0.36 },
    { frame: 'world', position: [0.0, 1.9, 1.35], target: [0.3, 0.95, 0.0], fov: 46, shiftX: 0, shiftY: 0.34 },
    { frame: 'world', position: [0.75, 1.15, 0.9], target: [0.48, 1.03, 0.12], fov: 44, shiftX: 0, shiftY: 0.36 },
    { frame: 'glass', position: [0, -0.02, 0.5], target: [0, -0.02, 0], fov: 44, shiftX: 0, shiftY: 0.4 },
    { frame: 'space', position: [0.0, -0.75, 0.85], target: [0.0, 0.0, 0.0], fov: 46, shiftX: 0, shiftY: 0.36 },
    { frame: 'space', position: [0.25, -0.72, 0.8], target: [0.02, 0.02, 0.0], fov: 46, shiftX: 0, shiftY: 0.36 },
    { frame: 'space', position: [-0.2, -0.7, 0.78], target: [0.0, 0.02, 0.0], fov: 46, shiftX: 0, shiftY: 0.36 },
    { frame: 'world', position: [-0.95, 0.95, 1.0], target: [0.3, 0.95, -0.05], fov: 46, shiftX: 0, shiftY: 0.36 },
    { frame: 'world', position: [0.15, 2.4, 2.0], target: [0.2, 0.85, -0.05], fov: 48, shiftX: 0, shiftY: 0.36 },
  ],
};

export function layoutFor(width: number, height: number): Layout {
  return width < 768 || height > width * 1.05 ? 'phone' : 'desktop';
}
