import {
  BufferGeometry,
  Float32BufferAttribute,
  HalfFloatType,
  Mesh,
  OrthographicCamera,
  PMREMGenerator,
  Scene,
  WebGLRenderTarget,
  type Camera,
  type Light,
  type Material,
  type Object3D,
  type Texture,
  type WebGLRenderer,
} from 'three';
import { quiet } from '@/lib/idle';

type Probe = Record<string, unknown> & {
  isShaderMaterial?: boolean;
  isWebGLRenderer?: boolean;
  isTexture?: boolean;
  isRenderTarget?: boolean;
  isBufferGeometry?: boolean;
  isObject3D?: boolean;
};

type Pass = { fullscreenMaterial?: Material | null; renderToScreen?: boolean };

function collect(value: unknown, found: Set<Material>, seen: Set<unknown>, depth: number) {
  if (!value || typeof value !== 'object' || seen.has(value) || depth > 8 || ArrayBuffer.isView(value)) return;
  if (typeof EventTarget !== 'undefined' && value instanceof EventTarget) return;
  seen.add(value);
  const probe = value as Probe;
  if (probe.isShaderMaterial) {
    found.add(value as Material);
    return;
  }
  if (probe.isObject3D) {
    collect(probe.material, found, seen, depth + 1);
    return;
  }
  if (probe.isWebGLRenderer || probe.isTexture || probe.isRenderTarget || probe.isBufferGeometry) return;
  if (value instanceof Map || value instanceof Set) {
    for (const item of value.values()) collect(item, found, seen, depth + 1);
    return;
  }
  for (const key of Object.keys(probe)) collect(probe[key], found, seen, depth + 1);
}

function stage(materials: Iterable<Material>, triangle: BufferGeometry) {
  const scene = new Scene();
  for (const material of materials) scene.add(new Mesh(triangle, material));
  return scene;
}

export function precompile(gl: WebGLRenderer, scene: Scene, camera: Camera, composer: { passes?: Pass[] } | null, extra: Promise<unknown>[] = []) {
  const materials = new Set<Material>();
  if (composer) collect(composer, materials, new Set(), 0);
  const final = (composer?.passes ?? []).filter((pass) => pass.renderToScreen && pass.fullscreenMaterial).map((pass) => pass.fullscreenMaterial as Material);
  const triangle = new BufferGeometry();
  triangle.setAttribute('position', new Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
  triangle.setAttribute('uv', new Float32BufferAttribute([0, 0, 2, 0, 0, 2], 2));
  const flat = new OrthographicCamera(-1, 1, 1, -1, 0, 1);
  const target = new WebGLRenderTarget(1, 1, { type: HalfFloatType });
  const previous = gl.getRenderTarget();
  gl.setRenderTarget(target);
  const pending = [gl.compileAsync(scene, camera), gl.compileAsync(stage(materials, triangle), flat), ...extra];
  gl.setRenderTarget(null);
  pending.push(gl.compileAsync(stage(final, triangle), flat));
  gl.setRenderTarget(previous);
  return Promise.all(pending).finally(() => {
    target.dispose();
    triangle.dispose();
  });
}

const waiting = new WeakMap<Scene, () => void>();

export function environmentReady(scene: Scene) {
  if (scene.environment) return Promise.resolve();
  return new Promise<void>((resolve) => waiting.set(scene, resolve));
}

export function applyEnvironment(gl: WebGLRenderer, scene: Scene, texture: Texture) {
  const generator = new PMREMGenerator(gl);
  const target = generator.fromEquirectangular(texture);
  generator.dispose();
  scene.environment = target.texture;
  waiting.get(scene)?.();
  waiting.delete(scene);
  return () => {
    if (scene.environment === target.texture) scene.environment = null;
    target.dispose();
  };
}

type Program = { getUniforms(): unknown; getAttributes(): unknown };

export async function settle(gl: WebGLRenderer, root: Object3D) {
  const materials = new Set<Material>();
  root.traverse((child) => {
    const mesh = child as Mesh;
    if (!mesh.isMesh) return;
    for (const material of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) materials.add(material);
  });
  const programs = new Set<Program>();
  for (const material of materials) {
    const program = (gl.properties.get(material) as { currentProgram?: Program }).currentProgram;
    if (program) programs.add(program);
  }
  for (const program of programs) {
    await quiet();
    program.getUniforms();
    program.getAttributes();
  }
}

const WARM_LAYER = 31;

export function warmDraw(gl: WebGLRenderer, scene: Scene, camera: Camera, root: Object3D) {
  const view = camera.clone();
  view.layers.set(WARM_LAYER);
  scene.traverse((object) => {
    if ((object as Light).isLight) object.layers.enable(WARM_LAYER);
  });
  const masks = new Map<Object3D, [number, boolean]>();
  root.traverse((object) => {
    masks.set(object, [object.layers.mask, object.frustumCulled]);
    object.layers.set(WARM_LAYER);
    object.frustumCulled = false;
  });
  const parent = root.parent;
  if (!parent) scene.add(root);
  const hidden: Object3D[] = [];
  for (let at: Object3D | null = root; at; at = at.parent) {
    if (!at.visible) hidden.push(at);
    at.visible = true;
  }
  const shadows = gl.shadowMap.needsUpdate;
  gl.shadowMap.needsUpdate = false;
  const target = new WebGLRenderTarget(1, 1, { type: HalfFloatType });
  const previous = gl.getRenderTarget();
  gl.setRenderTarget(target);
  gl.render(scene, view);
  gl.setRenderTarget(previous);
  target.dispose();
  gl.shadowMap.needsUpdate = shadows;
  for (const object of hidden) object.visible = false;
  if (!parent) scene.remove(root);
  for (const [object, [mask, culled]] of masks) {
    object.layers.mask = mask;
    object.frustumCulled = culled;
  }
}
