import { useGLTF } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { startTransition, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useState } from 'react';
import { Box3, Group, HalfFloatType, Mesh, MeshStandardMaterial, Object3D, Vector3, WebGLRenderTarget, type Material, type Texture, type WebGLRenderer } from 'three';
import { KTX2Loader } from 'three/examples/jsm/loaders/KTX2Loader.js';
import { MeshoptDecoder } from 'three/examples/jsm/libs/meshopt_decoder.module.js';
import { asset } from '@/lib/base';
import { changed, runtime, story } from '@/story/state';
import { gpuClass, quality } from './quality';
import { layoutFor } from './shots';
import { LAMP, LAPTOP, TELEVISION, placements, type Placement } from './layout';
import { still as native } from './still';
import { advance, budget, calm, fastPath, level, pause, prefetch, stillsPlanned, upgrades } from './budget';
import { labelProps } from './labels';
import { live, makeLive, obstacles, rimmed } from './touch';
import { quiet } from '@/lib/idle';
import { rendererCaughtUp } from './upload';
import { settle, warmDraw } from './warm';
import { windowLit } from './window';
import { inHeroSet, restOrder, sharpAllowed } from './sharpset';
import { GLTFLoader } from 'three-stdlib';

export { BULB, DESK_TOP, LAMP, LAPTOP, TELEVISION } from './layout';

const heroes = ['desk', 'laptop', 'lamp', 'television'] as const;
const extras = ['notepads', 'stationery', 'clock', 'cassette', 'plant', 'stapler', 'glasses', 'gamepad', 'console'] as const;
type Hero = (typeof heroes)[number];
type Extra = (typeof extras)[number];

function pick<T extends string>(names: readonly T[], folder: string) {
  return Object.fromEntries(names.map((name) => [name, asset(`${folder}/${name}.glb`)])) as Record<T, string>;
}

const full = pick(heroes, 'models');
const query = new URLSearchParams(window.location.search);
const still = query.has('still');
const lite = !still;
const urls = lite ? pick(heroes, 'models/lite') : full;
const sharpExtras = ['clock', 'cassette', 'plant', 'stapler', 'glasses', 'gamepad'] as const;
const extraUrls = { ...pick(extras, 'models'), ...(still ? pick(sharpExtras, 'models/sharp') : {}) };
const ordered = <T extends string>(names: readonly T[], first: boolean) => names.filter((name) => sharpAllowed(name) && inHeroSet(name) === first);
const heroFirst = ordered(heroes, true).map((name) => full[name]);
const sharpFirst = Object.values(pick(ordered(sharpExtras, true), 'models/sharp'));
const sharpModels = pick(sharpExtras, 'models/sharp');
const laterUrl = (name: string) => (heroes.includes(name as Hero) ? (lite ? full[name as Hero] : null) : sharpExtras.includes(name as (typeof sharpExtras)[number]) ? sharpModels[name as (typeof sharpExtras)[number]] : null);

const ktx2 = new KTX2Loader().setTranscoderPath(asset('basis/'));

if (!runtime.abandoned) {
  const support = gpuClass();
  ktx2.detectSupport({
    extensions: {
      has: (name: string) => support.extensions.has(name),
      get: () => ({ getSupportedProfiles: () => (support.extensions.has('astc-hdr') ? ['ldr', 'hdr'] : ['ldr']) }),
    },
  } as never);
  MeshoptDecoder.useWorkers(2);
}

function extend(loader: { setKTX2Loader: (value: never) => unknown; setMeshoptDecoder: (value: never) => unknown }) {
  loader.setKTX2Loader(ktx2 as never);
  loader.setMeshoptDecoder(MeshoptDecoder as never);
}

if (!runtime.abandoned) for (const name of heroes) useGLTF.preload(urls[name], false, true, extend as never);
if (still && !runtime.abandoned) for (const name of extras) useGLTF.preload(extraUrls[name], false, true, extend as never);

const slots = ['map', 'normalMap', 'roughnessMap', 'metalnessMap', 'aoMap', 'emissiveMap'] as const;

function standardMaterials(root: Object3D) {
  const found = new Map<string, Set<MeshStandardMaterial>>();
  root.traverse((child) => {
    const mesh = child as Mesh;
    const material = mesh.material as MeshStandardMaterial | undefined;
    if (!mesh.isMesh || !material?.isMeshStandardMaterial) return;
    if (!found.has(material.name)) found.set(material.name, new Set());
    found.get(material.name)?.add(material);
  });
  return found;
}

async function swapTextures(gl: WebGLRenderer, scene: Object3D, source: Object3D, cancelled: () => boolean) {
  const sources = standardMaterials(source);
  const targets = standardMaterials(scene);
  const swaps: [MeshStandardMaterial, (typeof slots)[number], Texture][] = [];
  for (const [materialName, found] of sources) {
    const from = [...found][0];
    for (const material of targets.get(materialName) ?? []) {
      for (const slot of slots) {
        const next = from[slot];
        if (next && material[slot] && material[slot] !== next) swaps.push([material, slot, next]);
      }
    }
  }
  const uploaded = new Set<Texture>();
  for (const [, , texture] of swaps) {
    if (uploaded.has(texture)) continue;
    uploaded.add(texture);
    await calm();
    await rendererCaughtUp(gl);
    if (cancelled()) return false;
    gl.initTexture(texture);
  }
  const replaced = new Set<Texture>();
  for (const [material, slot, texture] of swaps) {
    const previous = material[slot];
    if (previous) replaced.add(previous);
    material[slot] = texture;
  }
  for (const texture of replaced) if (!uploaded.has(texture)) texture.dispose();
  if (!stillsPlanned()) changed();
  return true;
}

export async function sharpenModel(gl: WebGLRenderer, scene: Object3D, name: string, cancelled: () => boolean) {
  const url = laterUrl(name);
  if (!url) return false;
  const loader = new GLTFLoader();
  extend(loader as never);
  const gltf = await loader.loadAsync(url);
  if (cancelled()) return false;
  return swapTextures(gl, scene, gltf.scene, cancelled);
}

function Sharpen({ url, onDone }: { url: string; onDone: () => void }) {
  const gl = useThree((state) => state.gl);
  const scene = useThree((state) => state.scene);
  const gltf = useGLTF(url, false, true, extend);
  useEffect(() => {
    let stopped = false;
    void swapTextures(gl, scene, gltf.scene, () => stopped).then((done) => {
      if (done && !stopped) onDone();
    });
    return () => {
      stopped = true;
    };
  }, [gl, scene, gltf, onDone]);
  return null;
}

function whenReady(start: () => void, test: () => boolean, never: () => boolean = () => false) {
  let stopped = false;
  void (async () => {
    await new Promise((resolve) => window.setTimeout(resolve, 0));
    while (!stopped && !test()) {
      if (never()) return;
      await pause(200);
    }
    if (!stopped) start();
  })();
  return () => {
    stopped = true;
  };
}

function Sharpener({ list, when, done, width = 1 }: { list: string[]; when: () => boolean; done: () => void; width?: number }) {
  const [index, setIndex] = useState(-1);
  const [ended, setEnded] = useState(0);
  const step = useCallback(() => {
    const go = () => setIndex((value) => value + 1);
    if (fastPath()) startTransition(go);
    else go();
  }, []);
  const next = useCallback(() => {
    setEnded((value) => value + 1);
    void calm().then(step);
  }, [step]);
  useEffect(
    () =>
      whenReady(
        () => {
          for (let i = 0; i < width; i++) void calm().then(step);
        },
        when,
        () => level() !== 'all',
      ),
    [when, step, width],
  );
  useEffect(() => {
    if (index >= 0 && ended >= list.length) done();
  }, [index, ended, list, done]);
  return (
    <Suspense fallback={null}>
      {list.slice(0, Math.max(0, index + 1)).map((url) => (
        <Sharpen key={url} url={url} onDone={next} />
      ))}
    </Suspense>
  );
}

const desktop = () => layoutFor(window.innerWidth, window.innerHeight) !== 'phone';
export const heroesWhen = () => lite && runtime.props && (quality.tier === 'full' || native.ready) && desktop() && level() === 'all';
const restShown = () => native.ready || (!stillsPlanned() && native.held);
export const extrasWhen = () => runtime.painted && restShown() && desktop() && level() === 'all';
const heroesFirstWhen = () => heroesWhen() && budget.first;
const extrasFirstWhen = () => extrasWhen() && budget.stage >= 1;
const FIRST_WIDTH = 1;
const heroesFirstDone = () => advance(1);
const extrasFirstDone = () => advance(2);

function prepare(root: Object3D) {
  root.traverse((child) => {
    const mesh = child as Mesh;
    if (!mesh.isMesh) return;
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    const material = mesh.material as MeshStandardMaterial;
    if (!material.isMeshStandardMaterial) return;
    if (material.transparent) {
      mesh.castShadow = false;
      material.depthWrite = false;
    }
    material.envMapIntensity = material.name === 'metal_office_desk' ? 0.18 : 1;
    if (material.name === 'metal_office_desk' && !material.userData.toned) {
      material.color.multiplyScalar(0.5);
      material.userData.toned = true;
      material.onBeforeCompile = (shader) => {
        shader.fragmentShader = shader.fragmentShader.replace(
          '#include <roughnessmap_fragment>',
          '#include <roughnessmap_fragment>\n  roughnessFactor = max(roughnessFactor, 0.6);',
        );
      };
      material.customProgramCacheKey = () => 'scooch-desk';
    }
    windowLit(material);
    if (!material.transparent) rimmed(material);
  });
}

function placeOnDesk(object: Object3D, placement: Placement, top: number) {
  object.updateWorldMatrix(true, true);
  const copy = object.clone(true);
  object.matrixWorld.decompose(copy.position, copy.quaternion, copy.scale);
  copy.updateWorldMatrix(false, true);
  const box = new Box3().setFromObject(copy);
  const center = box.getCenter(new Vector3());
  copy.position.x -= center.x;
  copy.position.y -= box.min.y;
  copy.position.z -= center.z;
  const holder = new Group();
  holder.name = `${placement.model}_${placement.node ?? 'whole'}`;
  holder.position.set(placement.x, top + (placement.lift ?? 0), placement.z);
  holder.rotation.set(placement.tilt ?? 0, placement.rotation, placement.roll ?? 0);
  holder.scale.setScalar(placement.scale ?? 1);
  holder.add(copy);
  return holder;
}

export type Console = { mesh: Mesh; frame: Object3D };

const FADE_IN = 200;
const fadeState = { start: 0, opaque: new Map<Material, boolean>(), opacity: new Map<Material, number>() };

function materialsOf(root: Object3D) {
  const found = new Set<Material>();
  root.traverse((child) => {
    const mesh = child as Mesh;
    if (!mesh.isMesh) return;
    for (const material of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
      found.add(material);
      if (!fadeState.opaque.has(material)) {
        fadeState.opaque.set(material, !material.transparent);
        fadeState.opacity.set(material, material.opacity);
      }
    }
  });
  return found;
}

function fadeIn(root: Object3D) {
  for (const material of materialsOf(root)) {
    material.transparent = true;
    material.opacity = 0;
  }
  fadeState.start = performance.now();
  live.fadeUntil = fadeState.start + FADE_IN + 50;
}

function stepFade(root: Object3D) {
  if (!fadeState.start) return;
  const t = Math.min((performance.now() - fadeState.start) / FADE_IN, 1);
  for (const material of materialsOf(root)) {
    material.opacity = (fadeState.opacity.get(material) ?? 1) * t;
    if (t >= 1) material.transparent = fadeState.opaque.get(material) === false;
  }
  if (t >= 1) fadeState.start = 0;
}

function Extras({ phone, onExtras, onConsole }: { phone: boolean; onExtras: () => void; onConsole: (found: Console) => void }) {
  const gl = useThree((state) => state.gl);
  const scene = useThree((state) => state.scene);
  const camera = useThree((state) => state.camera);
  const scenes = extras.map((name) => useGLTF(extraUrls[name], false, true, extend).scene);
  const group = useMemo(() => {
    const loaded = Object.fromEntries(extras.map((name, index) => [name, { scene: scenes[index] }])) as Record<Extra, { scene: Group }>;
    const placed = new Group();
    placed.name = 'extras';
    placed.visible = false;
    for (const model of Object.values(loaded)) prepare(model.scene);
    for (const spot of placements) {
      const source = loaded[spot.model as Extra];
      if (!source) continue;
      const object = spot.node ? source.scene.getObjectByName(spot.node) : source.scene;
      if (!object) continue;
      const placement = phone && typeof spot.phone === 'object' ? { ...spot, ...spot.phone } : spot;
      const holder = placeOnDesk(object, placement, placement.floor ? 0 : LAPTOP.position[1]);
      holder.userData.phone = spot.phone !== false;
      holder.userData.floor = !!placement.floor;
      placed.add(holder);
    }
    const named: Record<string, string> = {
      cassette_whole: 'cassette',
      clock_whole: 'clock',
      stapler_whole: 'stapler',
      plant_whole: 'plant',
    };
    for (const holder of placed.children) {
      const id = named[holder.name];
      if (!holder.userData.floor) obstacles.push(holder);
      if (id) makeLive(holder, id);
    }
    return placed;
  }, scenes);
  const labels = useMemo(() => labelProps(group), [group]);
  useEffect(() => {
    const holder = group.getObjectByName('console_whole');
    let screen: Mesh | null = null;
    holder?.traverse((child) => {
      const mesh = child as Mesh;
      if (mesh.isMesh && (mesh.material as MeshStandardMaterial).name === 'television_01_screen') screen = mesh;
    });
    if (holder && screen) onConsole({ mesh: screen, frame: holder });
  }, [group, onConsole]);
  useFrame(() => {
    labels.update(story.typed);
    stepFade(group);
  });

  useLayoutEffect(() => {
    for (const child of group.children) child.visible = !phone || child.userData.phone;
  }, [group, phone]);

  useEffect(() => {
    let stopped = false;
    group.visible = false;
    const sharpLabels = (wait: () => Promise<void>) => labels.sharpen(gl, wait);
    const target = new WebGLRenderTarget(1, 1, { type: HalfFloatType });
    const compileEach = async () => {
      const waits: Promise<unknown>[] = [];
      const meshes: Mesh[] = [];
      group.traverse((child) => {
        if ((child as Mesh).isMesh) meshes.push(child as Mesh);
      });
      for (const see of [true, false]) {
        for (const material of materialsOf(group)) material.transparent = see || fadeState.opaque.get(material) === false;
        for (const mesh of meshes) {
          await quiet();
          if (stopped) return;
          const previous = gl.getRenderTarget();
          gl.setRenderTarget(target);
          waits.push(gl.compileAsync(mesh, camera, scene));
          gl.setRenderTarget(previous);
        }
        await Promise.all(waits);
      }
      await settle(gl, group);
    };
    const pending = compileEach();
    const textures = new Set<Texture>();
    group.traverse((child) => {
      const material = (child as Mesh).material as MeshStandardMaterial | undefined;
      if (!material?.isMeshStandardMaterial) return;
      for (const slot of slots) if (material[slot]) textures.add(material[slot]);
    });
    void pending
      .catch(() => undefined)
      .then(async () => {
        target.dispose();
        await labels.ready(gl);
        if (still) await labels.sharpen(gl, quiet);
        for (const texture of textures) {
          await quiet();
          await rendererCaughtUp(gl);
          if (stopped) return;
          gl.initTexture(texture);
        }
        for (const holder of [...group.children]) {
          await quiet();
          if (stopped) return;
          warmDraw(gl, scene, camera, holder);
        }
        if (stopped) return;
        if (sharpAllowed('labels')) upgrades.add(sharpLabels);
        if (!runtime.posterUp) fadeIn(group);
        group.visible = true;
        runtime.props = true;
        onExtras();
      });
    return () => {
      stopped = true;
      upgrades.delete(sharpLabels);
    };
  }, [group, gl, scene, camera, onExtras, labels]);

  return <primitive object={group} />;
}

function LateExtras({ phone, onExtras, onConsole }: { phone: boolean; onExtras: () => void; onConsole: (found: Console) => void }) {
  const [go, setGo] = useState(still);
  useEffect(() => {
    if (go) return;
    return whenReady(
      () => {
        for (const name of extras) useGLTF.preload(extraUrls[name], false, true, extend as never);
        setGo(true);
      },
      () => runtime.ready,
    );
  }, [go]);
  if (!go) return null;
  return (
    <Suspense fallback={null}>
      <Extras phone={phone} onExtras={onExtras} onConsole={onConsole} />
    </Suspense>
  );
}

export type Screens = {
  laptop: { mesh: Mesh; frame: Group };
  television: { mesh: Mesh; frame: Group };
};

export function Desk({ onScreens, onExtras, onConsole, phone }: { onScreens: (screens: Screens) => void; onExtras: () => void; onConsole: (found: Console) => void; phone: boolean }) {
  const desk = useGLTF(urls.desk, false, true, extend);
  const laptop = useGLTF(urls.laptop, false, true, extend);
  const lamp = useGLTF(urls.lamp, false, true, extend);
  const television = useGLTF(urls.television, false, true, extend);

  useEffect(
    () =>
      whenReady(
        () => {
          if (desktop() && level() === 'all') prefetch([...(lite ? heroFirst : []), ...sharpFirst]);
        },
        () => runtime.shown,
      ),
    [],
  );
  useEffect(
    () =>
      whenReady(
        () => {
          if (desktop() && level() === 'all') prefetch(restOrder().flatMap((name) => laterUrl(name) ?? []));
        },
        () => runtime.shown && budget.hero,
      ),
    [],
  );

  useMemo(() => {
    for (const model of [desk, laptop, lamp, television]) prepare(model.scene);
    lamp.scene.traverse((child) => {
      const mesh = child as Mesh;
      if (!mesh.isMesh) return;
      const material = mesh.material as MeshStandardMaterial;
      if (material.name === 'desk_lamp_arm_01_light') {
        material.emissive.set('#ffb46b');
        material.emissiveIntensity = 6;
        material.toneMapped = true;
        mesh.castShadow = false;
        mesh.userData.bulb = true;
      }
    });
    const pivot = laptop.scene.getObjectByName('classic_laptop_screen_pivot');
    if (pivot) pivot.rotation.x = LAPTOP.lid;
    makeLive(lamp.scene, 'lamp');
    makeLive(television.scene, 'tv');
    makeLive(laptop.scene, 'laptop');
    obstacles.push(lamp.scene, television.scene, laptop.scene);
  }, [desk, laptop, lamp, television]);

  const laptopFrame = useMemo(() => new Group(), []);
  const televisionFrame = useMemo(() => new Group(), []);

  useLayoutEffect(() => {
    laptopFrame.updateWorldMatrix(true, true);
    televisionFrame.updateWorldMatrix(true, true);
    const find = (root: Object3D, name: string) => {
      let found: Mesh | null = null;
      root.traverse((child) => {
        const mesh = child as Mesh;
        if (mesh.isMesh && (mesh.material as MeshStandardMaterial).name === name) found = mesh;
      });
      return found as Mesh | null;
    };
    const laptopGlass = find(laptop.scene, 'classic_laptop_screen');
    const televisionGlass = find(television.scene, 'television_02_screen');
    if (laptopGlass && televisionGlass) {
      onScreens({ laptop: { mesh: laptopGlass, frame: laptopFrame }, television: { mesh: televisionGlass, frame: televisionFrame } });
    }
  }, [laptop, television, laptopFrame, televisionFrame, onScreens]);

  return (
    <group>
      <primitive object={desk.scene} />
      <primitive object={laptopFrame} position={LAPTOP.position} rotation-y={LAPTOP.rotation}>
        <group scale={LAPTOP.scale}>
          <primitive object={laptop.scene} />
        </group>
      </primitive>
      <group position={LAMP.position} rotation-y={LAMP.rotation}>
        <primitive object={lamp.scene} />
      </group>
      <primitive object={televisionFrame} position={TELEVISION.position} rotation-y={TELEVISION.rotation}>
        <primitive object={television.scene} />
      </primitive>
      <LateExtras phone={phone} onExtras={onExtras} onConsole={onConsole} />
      <Sharpener list={heroFirst} when={heroesFirstWhen} done={heroesFirstDone} width={FIRST_WIDTH} />
      <Sharpener list={sharpFirst} when={extrasFirstWhen} done={extrasFirstDone} width={FIRST_WIDTH} />
    </group>
  );
}
