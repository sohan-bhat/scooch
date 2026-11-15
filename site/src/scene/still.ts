import type { EffectComposer, Pass } from 'postprocessing';
import { Box3, Frustum, Matrix4, Texture, Vector2, Vector3, type Camera, type Object3D, type Texture as TextureType, type WebGLRenderer, type WebGLRenderTarget } from 'three';
import { quiet } from '@/lib/idle';
import { changed, runtime } from '@/story/state';
import { quality } from './quality';

export type Phase = 'live' | 'capture' | 'screen' | 'bands' | 'post' | 'show' | 'still';

const KEY = 'scooch:native';

export const still = {
  phase: 'live' as Phase,
  band: 0,
  pass: 0,
  bands: 4,
  live: null as TextureType | null,
  image: null as TextureType | null,
  shownAt: 0,
  inputAt: 0,
  frozenAt: 0,
  second: -1,
  tick: false,
  showing: false,
  full: false,
  row: 0,
  ready: false,
  off: false,
  box: null as { x: number; y: number; w: number; h: number } | null,
  stale: false,
  refresh: false,
  builtAt: 0,
  rebuilds: 0,
  arrived: false,
  startedAt: 0,
  target: 0,
  from: null as TextureType | null,
  held: false,
  lost: false,
};

const descriptor = Object.getOwnPropertyDescriptor(Texture.prototype, 'needsUpdate');
if (descriptor?.set) {
  const set = descriptor.set;
  Object.defineProperty(Texture.prototype, 'needsUpdate', {
    ...descriptor,
    set(value: boolean) {
      set.call(this, value);
      if (value) changed();
    },
  });
}

export function nativeAllowed(gl: WebGLRenderer) {
  try {
    if (window.sessionStorage.getItem(KEY) === 'off') return false;
  } catch {
    return false;
  }
  const memory = (navigator as Navigator & { deviceMemory?: number }).deviceMemory;
  if (memory !== undefined && memory < 4) return false;
  const context = gl.getContext();
  const size = gl.getSize(new Vector2()).multiplyScalar(quality.nativeDpr);
  const largest = context.getParameter(context.MAX_TEXTURE_SIZE) as number;
  return Math.ceil(size.x) <= largest && Math.ceil(size.y) <= largest;
}

export function nativeOff() {
  still.off = true;
  still.ready = false;
  try {
    window.sessionStorage.setItem(KEY, 'off');
  } catch {
    return;
  }
}

function targets(value: unknown, found: Set<WebGLRenderTarget>, seen: Set<unknown>, depth: number) {
  if (!value || typeof value !== 'object' || seen.has(value) || depth > 6 || ArrayBuffer.isView(value)) return;
  seen.add(value);
  const probe = value as Record<string, unknown> & { isWebGLRenderTarget?: boolean; isTexture?: boolean; isObject3D?: boolean; isMaterial?: boolean; isWebGLRenderer?: boolean };
  if (probe.isWebGLRenderTarget) {
    found.add(value as WebGLRenderTarget);
    return;
  }
  if (probe.isTexture || probe.isObject3D || probe.isMaterial || probe.isWebGLRenderer) return;
  if (Array.isArray(value)) {
    for (const item of value) targets(item, found, seen, depth + 1);
    return;
  }
  for (const key of Object.keys(probe)) targets(probe[key], found, seen, depth + 1);
}

const unused = new Set(['Bloom.Target', 'Blur.Target.A', 'Blur.Target.B', 'Luminance', 'Luminance.Previous', 'Luminance.Adapted']);

export async function allocate(gl: WebGLRenderer, composer: EffectComposer) {
  if (!composer.outputBuffer.depthTexture) composer.outputBuffer.depthBuffer = false;
  const found = new Set<WebGLRenderTarget>();
  targets(composer.inputBuffer, found, new Set(), 0);
  targets(composer.outputBuffer, found, new Set(), 0);
  const seen = new Set<unknown>();
  for (const pass of composer.passes) targets(pass as Pass, found, seen, 0);
  const context = gl.getContext();
  for (const target of found) {
    if (unused.has(target.texture.name)) continue;
    await quiet();
    if (still.off || context.isContextLost()) return false;
    gl.initRenderTarget(target);
  }
  return true;
}

const frustum = new Frustum();
const projection = new Matrix4();
const box = new Box3();

export function inView(object: Object3D | undefined, camera: Camera) {
  if (!object) return false;
  projection.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
  frustum.setFromProjectionMatrix(projection);
  return frustum.intersectsBox(box.setFromObject(object));
}

const corner = new Vector3();
const seen = new Vector3();

export function screenBox(object: Object3D | undefined, camera: Camera, width: number, height: number, pad = 24) {
  if (!object) return null;
  box.setFromObject(object);
  if (box.isEmpty()) return null;
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (let i = 0; i < 8; i++) {
    corner.set(i & 1 ? box.max.x : box.min.x, i & 2 ? box.max.y : box.min.y, i & 4 ? box.max.z : box.min.z);
    seen.copy(corner).applyMatrix4(camera.matrixWorldInverse);
    if (seen.z > -0.01) return null;
    corner.project(camera);
    x0 = Math.min(x0, (corner.x * 0.5 + 0.5) * width);
    x1 = Math.max(x1, (corner.x * 0.5 + 0.5) * width);
    y0 = Math.min(y0, (corner.y * 0.5 + 0.5) * height);
    y1 = Math.max(y1, (corner.y * 0.5 + 0.5) * height);
  }
  const x = Math.max(0, Math.floor(x0 - pad));
  const y = Math.max(0, Math.floor(y0 - pad));
  const w = Math.min(width, Math.ceil(x1 + pad)) - x;
  const h = Math.min(height, Math.ceil(y1 + pad)) - y;
  if (w <= 0 || h <= 0) return null;
  return { x, y, w, h };
}

export function freeze(time: number) {
  if (runtime.frozen !== null) return;
  runtime.frozen = time;
  still.frozenAt = performance.now();
}

export function thaw() {
  if (runtime.frozen === null) return;
  runtime.timeShift += (performance.now() - still.frozenAt) / 1000;
  runtime.frozen = null;
}
