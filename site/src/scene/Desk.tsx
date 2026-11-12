import { useGLTF } from '@react-three/drei';
import { useThree } from '@react-three/fiber';
import { useLayoutEffect, useMemo } from 'react';
import { Box3, Group, Mesh, MeshStandardMaterial, Object3D, Vector3, type WebGLRenderer } from 'three';
import { KTX2Loader } from 'three/examples/jsm/loaders/KTX2Loader.js';
import { asset } from '@/lib/base';

export const DESK_TOP = 0.787;
export const LAPTOP = { position: [0.13, DESK_TOP, 0.05] as const, rotation: -0.21, scale: 0.5, lid: -0.16 };
export const LAMP = { position: [0.04, DESK_TOP, -0.44] as const, rotation: Math.PI + 0.38 };
export const BULB = new Vector3(-0.005, 0.69, -0.177);

const urls = {
  desk: asset('models/desk.glb'),
  laptop: asset('models/laptop.glb'),
  lamp: asset('models/lamp.glb'),
  notepads: asset('models/notepads.glb'),
  stationery: asset('models/stationery.glb'),
};

const ktx2 = new KTX2Loader().setTranscoderPath(asset('basis/'));
let supported = false;

function useKtx2(gl: WebGLRenderer) {
  if (!supported) {
    ktx2.detectSupport(gl);
    supported = true;
  }
  return (loader: { setKTX2Loader: (value: never) => unknown }) => {
    loader.setKTX2Loader(ktx2 as never);
  };
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
  const extend = useKtx2(useThree((state) => state.gl));
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
    </group>
  );
}
