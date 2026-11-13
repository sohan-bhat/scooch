import { useGLTF } from '@react-three/drei';
import { useThree } from '@react-three/fiber';
import { Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useState } from 'react';
import { Box3, Group, Mesh, MeshStandardMaterial, Object3D, Vector3, type Texture, type WebGLRenderer } from 'three';
import { KTX2Loader } from 'three/examples/jsm/loaders/KTX2Loader.js';
import { asset } from '@/lib/base';
import { runtime } from '@/story/state';
import { gpuClass } from './quality';
import { layoutFor } from './shots';

export const DESK_TOP = 0.787;
export const LAPTOP = { position: [0.13, DESK_TOP, 0.05] as const, rotation: -0.21, scale: 0.5, lid: -0.16 };
export const LAMP = { position: [0.04, DESK_TOP, -0.44] as const, rotation: Math.PI + 0.38 };
export const BULB = new Vector3(-0.005, 0.69, -0.177);

const names = ['desk', 'laptop', 'lamp', 'notepads', 'stationery'] as const;
type Name = (typeof names)[number];

function pick(folder: string) {
  return Object.fromEntries(names.map((name) => [name, asset(`${folder}/${name}.glb`)])) as Record<Name, string>;
}

const full = pick('models');
const lite = !new URLSearchParams(window.location.search).has('still');
const urls = lite ? pick('models/lite') : full;

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

for (const name of names) useGLTF.preload(urls[name], false, true, extend as never);

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

function Sharpen({ name, onDone }: { name: Name; onDone: () => void }) {
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

function Sharpener() {
  const [index, setIndex] = useState(-1);
  const next = useCallback(() => setIndex((value) => value + 1), []);
  useEffect(() => {
    if (!lite) return;
    let timer = 0;
    const begin = () => {
      if (runtime.ready && layoutFor(window.innerWidth, window.innerHeight) !== 'phone') setIndex(0);
      else timer = window.setTimeout(begin, 400);
    };
    timer = window.setTimeout(begin, 400);
    return () => window.clearTimeout(timer);
  }, []);
  if (index < 0 || index >= names.length) return null;
  return (
    <Suspense fallback={null}>
      <Sharpen key={names[index]} name={names[index]} onDone={next} />
    </Suspense>
  );
}

type Placement = { name: string; x: number; z: number; rotation: number; lift?: number };

const props: Placement[] = [
  { name: 'office_notepads_yellow_pad', x: -0.2, z: 0.13, rotation: 0.26 },
  { name: 'stationery_supplies_pencil_used', x: -0.19, z: 0.15, rotation: -0.62, lift: 0.0102 },
  { name: 'office_notepads_sticky_stack', x: -0.4, z: 0.02, rotation: -0.35 },
  { name: 'stationery_supplies_pencil_new_a', x: -0.05, z: 0.3, rotation: 0.95 },
  { name: 'stationery_supplies_eraser', x: -0.38, z: 0.22, rotation: 0.3 },
  { name: 'stationery_supplies_pen_fancy', x: 0.5, z: 0.1, rotation: -1.3 },
];

function prepare(root: Object3D) {
  root.traverse((child) => {
    const mesh = child as Mesh;
    if (!mesh.isMesh) return;
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    const material = mesh.material as MeshStandardMaterial;
    if (!material.isMeshStandardMaterial) return;
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

function placeOnDesk(object: Object3D, placement: Placement) {
  object.updateWorldMatrix(true, false);
  const box = new Box3().setFromObject(object);
  const center = box.getCenter(new Vector3());
  object.removeFromParent();
  object.position.x -= center.x;
  object.position.y -= box.min.y;
  object.position.z -= center.z;
  const holder = new Group();
  holder.name = `${placement.name}_holder`;
  holder.position.set(placement.x, DESK_TOP + (placement.lift ?? 0), placement.z);
  holder.rotation.y = placement.rotation;
  holder.add(object);
  return holder;
}

export function Desk({ onScreen, showProps }: { onScreen: (mesh: Mesh, frame: Group) => void; showProps: boolean }) {
  const desk = useGLTF(urls.desk, false, true, extend);
  const laptop = useGLTF(urls.laptop, false, true, extend);
  const lamp = useGLTF(urls.lamp, false, true, extend);
  const notepads = useGLTF(urls.notepads, false, true, extend);
  const stationery = useGLTF(urls.stationery, false, true, extend);

  const groups = useMemo(() => {
    for (const model of [desk, laptop, lamp, notepads, stationery]) prepare(model.scene);
    const placed = new Group();
    placed.name = 'props';
    for (const placement of props) {
      const source = notepads.scene.getObjectByName(placement.name) ?? stationery.scene.getObjectByName(placement.name);
      if (source) placed.add(placeOnDesk(source, placement));
    }
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
    return { placed };
  }, [desk, laptop, lamp, notepads, stationery]);

  const frame = useMemo(() => new Group(), []);

  useLayoutEffect(() => {
    frame.updateWorldMatrix(true, true);
    let screen: Mesh | null = null;
    laptop.scene.traverse((child) => {
      const mesh = child as Mesh;
      if (mesh.isMesh && (mesh.material as MeshStandardMaterial).name === 'classic_laptop_screen') screen = mesh;
    });
    if (screen) onScreen(screen, frame);
  }, [laptop, frame, onScreen]);

  return (
    <group>
      <primitive object={desk.scene} />
      <primitive object={frame} position={LAPTOP.position} rotation-y={LAPTOP.rotation}>
        <group scale={LAPTOP.scale}>
          <primitive object={laptop.scene} />
        </group>
      </primitive>
      <group position={LAMP.position} rotation-y={LAMP.rotation}>
        <primitive object={lamp.scene} />
      </group>
      <primitive object={groups.placed} visible={showProps} />
      <Sharpener />
    </group>
  );
}
