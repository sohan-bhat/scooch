import { ContactShadows, Environment } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  Color,
  DirectionalLight,
  Group,
  Matrix4,
  Mesh,
  MeshStandardMaterial,
  Object3D,
  PerspectiveCamera,
  RectAreaLight,
  SpotLight,
  Vector3,
} from 'three';
import { RectAreaLightUniformsLib } from 'three/examples/jsm/lights/RectAreaLightUniformsLib.js';
import { asset } from '@/lib/base';
import { tokenColor } from '@/lib/tokens';
import type { DemoManifest } from '@/story/demo';
import { pointer, runtime, story } from '@/story/state';
import { BULB, DESK_TOP, Desk, LAMP, LAPTOP } from './Desk';
import { quality } from './quality';
import { Rig, type ScreenFrame } from './rig';
import { measureScreen, useScreen } from './screen/useScreen';
import { layoutFor, shots } from './shots';

RectAreaLightUniformsLib.init();

const SPOT = 4;
const BULB_GLOW = 7;
const RECT = 2.4;
const GLOW = 4.5;
const up = new Vector3(0, 1, 0);

function toWorld(local: Vector3, position: readonly [number, number, number], rotation: number) {
  return local.clone().applyAxisAngle(up, rotation).add(new Vector3(...position));
}

export const bulbWorld = toWorld(BULB, LAMP.position, LAMP.rotation);
const keyboard = toWorld(new Vector3(0.01, 0.05, 0.035), LAPTOP.position, LAPTOP.rotation);

type Built = { mesh: Mesh; frame: Group; screen: ScreenFrame };

export function World({ manifest, onReady }: { manifest: DemoManifest; onReady: () => void }) {
  const { camera, size, scene, gl } = useThree();
  const { material, uniforms, averages } = useScreen(manifest);
  const [built, setBuilt] = useState<Built | null>(null);
  const spot = useRef<SpotLight>(null);
  const rect = useRef<RectAreaLight>(null);
  const rim = useRef<DirectionalLight>(null);
  const glow = useRef<SpotLight>(null);
  const wallTarget = useMemo(() => { const object = new Object3D(); object.position.set(bulbWorld.x, bulbWorld.y - 0.1, -0.98); return object; }, []);
  const target = useMemo(() => new Object3D(), []);
  const bulbs = useRef<MeshStandardMaterial[]>([]);
  const focus = useMemo(() => new Vector3(), []);
  const scratch = useMemo(() => ({ color: new Color(), rest: new Color(), focusA: new Vector3() }), []);
  const room = useMemo(() => tokenColor('room'), []);
  const layout = layoutFor(size.width, size.height);

  const onScreen = useCallback(
    (mesh: Mesh, frame: Group) => {
      const aspect = manifest.screen.width / manifest.screen.height;
      const { origin, screen } = measureScreen(mesh, frame, aspect);
      uniforms.uOrigin.value.set(origin.x, origin.y);
      uniforms.uExtent.value.set(origin.width, origin.height);
      mesh.material = material;
      mesh.castShadow = false;
      setBuilt({ mesh, frame, screen });
    },
    [manifest, material, uniforms],
  );

  const rig = useMemo(() => {
    if (!built) return null;
    built.frame.updateWorldMatrix(true, false);
    return new Rig(shots[layout], new Matrix4().copy(built.frame.matrixWorld), built.screen, size.width / size.height);
  }, [built, layout, size.width, size.height]);

  useLayoutEffect(() => {
    scene.background = room;
    target.position.copy(keyboard);
    scene.add(target);
    scene.add(wallTarget);
    return () => {
      scene.remove(target);
      scene.remove(wallTarget);
    };
  }, [scene, room, target, wallTarget]);

  useEffect(() => {
    if (!built) return;
    const found: MeshStandardMaterial[] = [];
    scene.traverse((child) => {
      const mesh = child as Mesh;
      if (mesh.isMesh && mesh.userData.bulb) found.push(mesh.material as MeshStandardMaterial);
    });
    bulbs.current = found;
    const light = rect.current;
    if (light) {
      const world = built.screen.center.clone().applyMatrix4(built.frame.matrixWorld);
      const normal = built.screen.normal.clone().transformDirection(built.frame.matrixWorld);
      light.position.copy(world).addScaledVector(normal, 0.004);
      light.lookAt(world.clone().add(normal));
      light.width = built.screen.width;
      light.height = built.screen.height;
    }
    const key = spot.current;
    if (key) {
      key.target = target;
      key.shadow.needsUpdate = true;
    }
    gl.shadowMap.needsUpdate = true;
    uniforms.uLamp.value.copy(bulbWorld);
    let frames = 0;
    let handle = requestAnimationFrame(function wait() {
      frames += 1;
      if (frames < 3) handle = requestAnimationFrame(wait);
      else onReady();
    });
    return () => cancelAnimationFrame(handle);
  }, [built, scene, target, gl, uniforms, onReady]);

  useFrame((state, delta) => {
    if (!rig) return;
    const time = state.clock.elapsedTime;
    const idle = runtime.idle;
    const motion = idle ? 1 : 0;
    const distance = rig.apply(
      camera as PerspectiveCamera,
      story.camera,
      story.push,
      time,
      pointer.x * motion,
      pointer.y * motion,
      Math.min(delta, 0.1),
      size.width,
      size.height,
      idle,
    );
    void distance;
    const flicker = idle ? 1 + Math.sin(time * 13.1) * 0.006 + Math.sin(time * 29.7 + 1.1) * 0.004 + Math.sin(time * 7.3) * 0.003 : 1;
    const power = runtime.power;
    const lamp = story.lamp * flicker * power;
    if (spot.current) spot.current.intensity = SPOT * lamp;
    if (glow.current) glow.current.intensity = GLOW * lamp;
    const ambient = Math.min(story.lamp, 1) * power;
    scene.environmentIntensity = 0.05 + 0.17 * ambient;
    if (rim.current) rim.current.intensity = 0.12 + 0.2 * ambient;
    for (const bulb of bulbs.current) bulb.emissiveIntensity = BULB_GLOW * lamp;

    uniforms.uTime.value = time;
    uniforms.uMixAB.value = story.mixAB;
    uniforms.uMixBC.value = story.mixBC;
    uniforms.uSweep.value = story.sweep;
    uniforms.uLampLevel.value = lamp;
    uniforms.uPower.value = Math.min(Math.max((power - 0.35) / 0.65, 0), 1);

    const { frames, drag } = manifest;
    const icons = uniforms.uIcons.value;
    for (let i = 0; i < manifest.icons.length && i < icons.length; i++) {
      const id = manifest.icons[i].id;
      const [x, y, w, h] = frames.before.icons[id];
      if (id === drag.icon) {
        icons[i].set(
          drag.from[0] + (drag.to[0] - drag.from[0]) * story.drag,
          drag.from[1] + (drag.to[1] - drag.from[1]) * story.drag,
          w,
          h,
        );
        uniforms.uGhost.value[i] = story.grab;
      } else {
        icons[i].set(x, y, w, h);
      }
    }
    uniforms.uCursor.value.set(
      story.cursorX + (drag.to[0] - drag.from[0]) * story.drag,
      story.cursorY + (drag.to[1] - drag.from[1]) * story.drag,
      story.cursorAlpha,
    );

    const light = rect.current;
    if (light) {
      scratch.color.copy(averages[0]).lerp(averages[1], story.mixAB).lerp(averages[2], story.mixBC);
      scratch.rest.copy(scratch.color).multiplyScalar(0.35);
      const heat = Math.min(Math.max((story.sweep - 0.15) / 0.6, 0), 1);
      light.color.copy(scratch.color);
      light.intensity = RECT * (1 - heat * 0.55) * (1.5 - 0.5 * Math.min(story.lamp, 1)) * uniforms.uPower.value;
    }

    quality.focus(rig.focusPoint(focus), camera.position, story.focus, keyboard, scratch.focusA, story.dof);
  });

  const phone = layout === 'phone';

  return (
    <>
      <Environment files={asset('hdri/studio.hdr')} environmentIntensity={0.22} environmentRotation={[0, Math.PI * 0.7, 0]} />
      <fogExp2 attach="fog" args={[room, 0.32]} />
      <spotLight
        ref={spot}
        position={bulbWorld}
        color="#ffb46b"
        angle={0.46}
        penumbra={0.6}
        decay={2}
        distance={0}
        intensity={SPOT}
        castShadow
        shadow-mapSize={phone ? [1024, 1024] : [2048, 2048]}
        shadow-radius={5}
        shadow-bias={-0.0003}
        shadow-normalBias={0.012}
        shadow-camera-near={0.2}
        shadow-camera-far={3}
      />
      <spotLight ref={glow} position={bulbWorld} target={wallTarget} color="#ffb46b" angle={1.05} penumbra={1} intensity={GLOW} decay={2} distance={0} />
      <directionalLight ref={rim} position={[1.4, 1.8, -1.6]} color={tokenColor("rim")} intensity={0.3} />
      <rectAreaLight ref={rect} intensity={RECT} />
      <Desk onScreen={onScreen} showProps={!phone} />
      <ContactShadows
        position={[-0.05, DESK_TOP + 0.0015, 0.02]}
        scale={[1.9, 0.92]}
        resolution={512}
        blur={2.4}
        far={0.3}
        opacity={0.62}
        frames={1}
        color="#0a0806"
      />
      <mesh position={[0, 1.6, -0.98]} receiveShadow>
        <planeGeometry args={[9, 3.2]} />
        <meshStandardMaterial color={tokenColor('room-3')} roughness={0.94} metalness={0} />
      </mesh>
      <mesh rotation-x={-Math.PI / 2} position={[0, 0, 0.6]} receiveShadow>
        <planeGeometry args={[9, 4]} />
        <meshStandardMaterial color={tokenColor('room-2')} roughness={0.85} metalness={0} />
      </mesh>
    </>
  );
}
