export type Vec3 = [number, number, number];

export type Shot = {
  position?: Vec3;
  target?: Vec3;
  screen?: { fillHeight?: number; fillWidth?: number; lift?: number };
  fov: number;
  shiftX: number;
  shiftY: number;
};

export type Layout = 'desktop' | 'phone';

export const shots: Record<Layout, Shot[]> = {
  desktop: [
    { position: [-0.46, 0.4, 1.9], target: [-0.06, 0.33, -0.12], fov: 25, shiftX: 0.42, shiftY: 0 },
    { position: [-0.1, 0.31, 0.7], target: [0.0, 0.16, -0.08], fov: 30, shiftX: 0.4, shiftY: -0.02 },
    { screen: { fillHeight: 0.7 }, fov: 26, shiftX: 0.3, shiftY: -0.04 },
    { position: [0.66, 0.66, 0.62], target: [0.02, 0.12, -0.04], fov: 30, shiftX: -0.32, shiftY: -0.05 },
    { position: [0.46, 0.32, 1.5], target: [0.02, 0.3, -0.1], fov: 26, shiftX: -0.4, shiftY: 0 },
  ],
  phone: [
    { position: [-0.5, 0.52, 1.05], target: [0.0, 0.14, -0.02], fov: 46, shiftX: 0, shiftY: 0.38 },
    { position: [-0.2, 0.34, 0.66], target: [0.0, 0.15, -0.08], fov: 44, shiftX: 0, shiftY: 0.36 },
    { screen: { fillWidth: 0.94 }, fov: 44, shiftX: 0, shiftY: 0.42 },
    { position: [0.56, 0.7, 0.78], target: [0.02, 0.12, -0.04], fov: 46, shiftX: 0, shiftY: 0.36 },
    { position: [0.12, 0.5, 1.15], target: [0.03, 0.15, -0.02], fov: 46, shiftX: 0, shiftY: 0.38 },
  ],
};

export function layoutFor(width: number, height: number): Layout {
  return width < 768 || height > width * 1.05 ? 'phone' : 'desktop';
}
