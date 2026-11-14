import type { EffectComposer, Pass } from 'postprocessing';
import { Box3, Frustum, Matrix4, Texture, Vector2, type Camera, type Object3D, type Texture as TextureType, type WebGLRenderer, type WebGLRenderTarget } from 'three';
import { quiet } from '@/lib/idle';
import { changed, runtime } from '@/story/state';

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
  const size = gl.getDrawingBufferSize(new Vector2());
  const largest = context.getParameter(context.MAX_TEXTURE_SIZE) as number;
  return size.x <= largest && size.y <= largest;
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
    if (still.off) return false;
    gl.initRenderTarget(target);
    if (context.getError() === context.OUT_OF_MEMORY) {
      nativeOff();
      return false;
    }
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
