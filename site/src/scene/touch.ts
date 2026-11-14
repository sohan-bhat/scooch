import { Color, Mesh, type Material, type MeshStandardMaterial, type Object3D, type Vector2, type Vector4, type WebGLRenderer } from 'three';
import type { Frame } from './screen/scenery';

export const things: Record<string, Object3D | undefined> = {};
export const obstacles: Object3D[] = [];

export const CHANNELS = ['main', 'swans', 'moon', 'city'];

export const hooks = {
  drawHeld: (_gl: WebGLRenderer, _delta: number, _part?: { width: number; height: number }) => false,
  showing: () => [] as string[],
  current: () => 'main',
  frame: null as Frame | null,
  icons: null as Vector4[] | null,
  origin: null as Vector2 | null,
  extent: null as Vector2 | null,
  place: () => 0,
};

export const live = {
  quiet: false,
  lastScroll: -1e9,
  hover: '',
  lamp: 1,
  lampTarget: 1,
  aim: 0,
  aimTarget: 0,
  offAt: -1e9,
  onAt: -1e9,
  power: true,
  channel: -1,
  noiseAt: -1e9,
  tape: false,
  tapeAt: 0,
  ringUntil: 0,
  stapleAt: -1e9,
  sipAt: -1e9,
  juice: 4,
  glass: { x: 0, z: 0, ready: false },
  look: { x: 0, y: 0, tx: 0, ty: 0 },
  sway: { x: 0, z: 0, vx: 0, vz: 0 },
  icon: { index: -1, x: 0, y: 0 },
};

export type Live = typeof live;

const rimColor = new Color('#ffd9a8');
const rims = new WeakMap<Object3D, { value: number }[]>();

function withRim(material: MeshStandardMaterial, level: { value: number }) {
  if (material.userData.rim) return material.userData.rim as { value: number };
  material.userData.rim = level;
  const previous = material.onBeforeCompile;
  const key = material.customProgramCacheKey();
  material.onBeforeCompile = (shader, renderer) => {
    previous.call(material, shader, renderer);
    shader.uniforms.uRimLevel = level;
    shader.uniforms.uRimColor = { value: rimColor };
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nuniform float uRimLevel;\nuniform vec3 uRimColor;')
      .replace(
        '#include <opaque_fragment>',
        'if ( uRimLevel > 0.0 ) outgoingLight += uRimColor * pow(1.0 - saturate(dot(normal, normalize(vViewPosition))), 3.0) * uRimLevel;\n#include <opaque_fragment>',
      );
  };
  material.customProgramCacheKey = () => `${key}|rim`;
  return level;
}

export function rimmed(material: MeshStandardMaterial) {
  return withRim(material, { value: 0 });
}

export function makeLive(root: Object3D, id: string) {
  root.userData.live = id;
  things[id] = root;
  const levels: { value: number }[] = [];
  const seen = new Set<Material>();
  root.traverse((child) => {
    const mesh = child as Mesh;
    if (!mesh.isMesh) return;
    const materials = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
    for (const material of materials) {
      if (seen.has(material) || !(material as MeshStandardMaterial).isMeshStandardMaterial || (material.transparent && !material.userData.rim)) continue;
      seen.add(material);
      levels.push(withRim(material as MeshStandardMaterial, { value: 0 }));
    }
  });
  rims.set(root, levels);
}

export function rimOf(root: Object3D | undefined) {
  return root ? (rims.get(root) ?? []) : [];
}

export function liveId(object: Object3D | null) {
  let at: Object3D | null = object;
  while (at) {
    if (typeof at.userData.live === 'string') return at.userData.live as string;
    at = at.parent;
  }
  return '';
}

export function release(now: number) {
  live.lampTarget = 1;
  live.aimTarget = 0;
  live.channel = -1;
  if (!live.power) {
    live.power = true;
    live.onAt = now;
  }
  live.tape = false;
  live.ringUntil = 0;
  live.look.tx = 0;
  live.look.ty = 0;
  live.icon.index = -1;
}
