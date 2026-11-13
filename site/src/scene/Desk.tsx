import { useGLTF } from '@react-three/drei';
import { useThree } from '@react-three/fiber';
import { Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useState } from 'react';
import { Box3, Group, HalfFloatType, Mesh, MeshStandardMaterial, Object3D, Vector3, WebGLRenderTarget, type Texture, type WebGLRenderer } from 'three';
import { KTX2Loader } from 'three/examples/jsm/loaders/KTX2Loader.js';
import { asset } from '@/lib/base';
import { runtime } from '@/story/state';
import { gpuClass, quality } from './quality';
import { layoutFor } from './shots';
import { LAMP, LAPTOP, TELEVISION, placements, type Placement } from './layout';

export { BULB, DESK_TOP, LAMP, LAPTOP, TELEVISION } from './layout';

const heroes = ['desk', 'laptop', 'lamp', 'television'] as const;
const extras = ['notepads', 'stationery', 'clock', 'cassette', 'plant', 'stapler', 'glasses', 'watch', 'gamepad'] as const;
type Hero = (typeof heroes)[number];
type Extra = (typeof extras)[number];

function pick<T extends string>(names: readonly T[], folder: string) {
  return Object.fromEntries(names.map((name) => [name, asset(`${folder}/${name}.glb`)])) as Record<T, string>;
}

const full = pick(heroes, 'models');
const query = new URLSearchParams(window.location.search);
const still = query.has('still');
const hero = query.has('hero');
const lite = !still;
const urls = lite ? pick(heroes, 'models/lite') : full;
const extraUrls = pick(extras, 'models');

const support = gpuClass();
const ktx2 = new KTX2Loader().setTranscoderPath(asset('basis/'));
ktx2.detectSupport({
  extensions: {
    has: (name: string) => support.extensions.has(name),
    get: () => ({ getSupportedProfiles: () => (support.extensions.has('astc-hdr') ? ['ldr', 'hdr'] : ['ldr']) }),
  },
} as never);

function extend(loader: { setKTX2Loader: (value: never) => unknown }) {
  loader.setKTX2Loader(ktx2 as never);
}

for (const name of heroes) useGLTF.preload(urls[name], false, true, extend as never);
if (still) for (const name of extras) useGLTF.preload(extraUrls[name], false, true, extend as never);

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

function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(resolve));
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
    await nextFrame();
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
  return true;
}

function Sharpen({ name, onDone }: { name: Hero; onDone: () => void }) {
  const gl = useThree((state) => state.gl);
  const scene = useThree((state) => state.scene);
  const gltf = useGLTF(full[name], false, true, extend);
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

function whenReady(start: () => void, test: () => boolean) {
  let timer = 0;
  const check = () => {
    if (test()) start();
    else timer = window.setTimeout(check, 200);
  };
  timer = window.setTimeout(check, 0);
  return () => window.clearTimeout(timer);
}

function Sharpener() {
  const [index, setIndex] = useState(-1);
  const next = useCallback(() => setIndex((value) => value + 1), []);
  useEffect(() => {
    if (!lite) return;
    return whenReady(
      () => {
        runtime.sharpening = true;
        setIndex(0);
      },
      () => runtime.props && quality.tier === 'full' && layoutFor(window.innerWidth, window.innerHeight) !== 'phone',
    );
  }, []);
  useEffect(() => {
    if (index >= heroes.length) runtime.sharpening = false;
  }, [index]);
  useEffect(
    () => () => {
      runtime.sharpening = false;
    },
    [],
  );
  if (index < 0 || index >= heroes.length) return null;
  return (
    <Suspense fallback={null}>
      <Sharpen key={heroes[index]} name={heroes[index]} onDone={next} />
    </Suspense>
  );
}

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

function Extras({ phone, onExtras }: { phone: boolean; onExtras: () => void }) {
  const gl = useThree((state) => state.gl);
  const scene = useThree((state) => state.scene);
  const camera = useThree((state) => state.camera);
  const scenes = extras.map((name) => useGLTF(extraUrls[name], false, true, extend).scene);
  const group = useMemo(() => {
    const loaded = Object.fromEntries(extras.map((name, index) => [name, { scene: scenes[index] }])) as Record<Extra, { scene: Group }>;
    const placed = new Group();
    placed.name = 'extras';
    for (const model of Object.values(loaded)) prepare(model.scene);
    for (const placement of placements) {
      const source = loaded[placement.model as Extra];
      if (!source) continue;
      const object = placement.node ? source.scene.getObjectByName(placement.node) : source.scene;
      if (!object) continue;
      const holder = placeOnDesk(object, placement, LAPTOP.position[1]);
      holder.userData.phone = placement.phone !== false;
      placed.add(holder);
    }
    return placed;
  }, scenes);

  useLayoutEffect(() => {
    for (const child of group.children) child.visible = !phone || child.userData.phone;
  }, [group, phone]);

  useEffect(() => {
    let stopped = false;
    group.visible = false;
    const target = new WebGLRenderTarget(1, 1, { type: HalfFloatType });
    const compileEach = async () => {
      const waits: Promise<unknown>[] = [];
      for (const holder of group.children) {
        await nextFrame();
        if (stopped) return;
        const previous = gl.getRenderTarget();
        gl.setRenderTarget(target);
        waits.push(gl.compileAsync(holder, camera, scene));
        gl.setRenderTarget(previous);
      }
      await Promise.all(waits);
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
        for (const texture of textures) {
          await nextFrame();
          if (stopped) return;
          gl.initTexture(texture);
        }
        if (stopped) return;
        group.visible = true;
        runtime.props = true;
        onExtras();
      });
    return () => {
      stopped = true;
    };
  }, [group, gl, scene, camera, onExtras]);

  return <primitive object={group} />;
}

function LateExtras({ phone, onExtras }: { phone: boolean; onExtras: () => void }) {
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
      <Extras phone={phone} onExtras={onExtras} />
    </Suspense>
  );
}

export type Screens = {
  laptop: { mesh: Mesh; frame: Group };
  television: { mesh: Mesh; frame: Group };
};

export function Desk({ onScreens, onExtras, phone }: { onScreens: (screens: Screens) => void; onExtras: () => void; phone: boolean }) {
  const desk = useGLTF(urls.desk, false, true, extend);
  const laptop = useGLTF(urls.laptop, false, true, extend);
  const lamp = useGLTF(urls.lamp, false, true, extend);
  const television = useGLTF(urls.television, false, true, extend);

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
      {hero ? null : <LateExtras phone={phone} onExtras={onExtras} />}
      <Sharpener />
    </group>
  );
}
