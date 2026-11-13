import { ContactShadows } from '@react-three/drei';
import { useFrame, useLoader, useThree } from '@react-three/fiber';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  Color,
  DirectionalLight,
  FogExp2,
  Group,
  Matrix4,
  Mesh,
  MeshStandardMaterial,
  Object3D,
  PerspectiveCamera,
  PointLight,
  Quaternion,
  RectAreaLight,
  SpotLight,
  Vector2,
  Vector3,
} from 'three';
import { HDRLoader } from 'three/examples/jsm/loaders/HDRLoader.js';
import { asset } from '@/lib/base';
import { tokenColor } from '@/lib/tokens';
import type { DemoManifest } from '@/story/demo';
import { pointer, runtime, story } from '@/story/state';
import { STREET, blindsTexture, projector } from './Blinds';
import { BULB, DESK_TOP, Desk, LAMP, type Screens } from './Desk';
import { useInside, type Stage } from './Inside';
import { quality } from './quality';
import { useRoomMaterial } from './Room';
import { Rig, type ScreenFrame } from './rig';
import { measureScreen, useScreens } from './screen/useScreen';
import { keys, layoutFor } from './shots';
import { applyEnvironment, environmentReady, precompile } from './warm';

const SPOT = 4;
const BULB_GLOW = 7;
const RECT = 2.4;
const SCREEN_SPOT = 0.09;
const INNER = 0.5;
const STREET_LEVEL = 26;
const up = new Vector3(0, 1, 0);

function toWorld(local: Vector3, position: readonly [number, number, number], rotation: number) {
  return local.clone().applyAxisAngle(up, rotation).add(new Vector3(...position));
}

export const bulbWorld = toWorld(BULB, LAMP.position, LAMP.rotation);
const wallCenter = new Vector2(bulbWorld.x + 0.05, bulbWorld.y - 0.2);
const floorCenter = new Vector2(bulbWorld.x, bulbWorld.z + 0.4);

type Glass = { mesh: Mesh; frame: Group; screen: ScreenFrame; center: Vector3; matrix: Matrix4; quaternion: Quaternion };
type Built = { laptop: Glass; television: Glass };

function glassOf(mesh: Mesh, frame: Group, screen: ScreenFrame): Glass {
  frame.updateWorldMatrix(true, false);
  const center = screen.center.clone().applyMatrix4(frame.matrixWorld);
  const normal = screen.normal.clone().transformDirection(frame.matrixWorld);
  const upward = screen.up.clone().transformDirection(frame.matrixWorld);
  const right = new Vector3().crossVectors(upward, normal).normalize();
  const basis = new Matrix4().makeBasis(right, new Vector3().crossVectors(normal, right), normal);
  const quaternion = new Quaternion().setFromRotationMatrix(basis);
  const matrix = new Matrix4().compose(center, quaternion, new Vector3(1, 1, 1));
  return { mesh, frame, screen, center, matrix, quaternion };
}

function stageFor(glass: Glass): Stage {
  const width = glass.screen.width * 0.94;
  const height = glass.screen.height * 0.93;
  const normal = new Vector3(0, 0, 1).applyQuaternion(glass.quaternion);
  const tilt = new Quaternion().setFromAxisAngle(new Vector3(1, 0, 0), -1.0);
  return {
    width,
    height,
    glass: { position: glass.center.clone().addScaledVector(normal, 0.006), quaternion: glass.quaternion.clone(), scale: 1 },
    space: { position: glass.center.clone().addScaledVector(normal, 0.12).addScaledVector(up, 0.3), quaternion: glass.quaternion.clone().multiply(tilt), scale: 2.6 },
  };
}

function flicker(t: number) {
  if (t < 0.15) return 0;
  if (t < 0.22) return 0.9;
  if (t < 0.3) return 0.12;
  if (t < 0.36) return 0.75;
  if (t < 0.46) return 0.2;
  return Math.min(0.55 + (t - 0.46) * 1.5, 1);
}

function ramp(t: number, start: number, length: number) {
  const x = Math.min(Math.max((t - start) / length, 0), 1);
  return x * x * (3 - 2 * x);
}

export function Surroundings() {
  const texture = useLoader(HDRLoader, asset('hdri/studio.hdr'));
  const gl = useThree((state) => state.gl);
  const scene = useThree((state) => state.scene);
  useLayoutEffect(() => {
    scene.environmentRotation.set(0, Math.PI * 0.7, 0);
    scene.environmentIntensity = 0.22;
    return applyEnvironment(gl, scene, texture);
  }, [gl, scene, texture]);
  return null;
}

export function World({ manifest, onReady }: { manifest: DemoManifest; onReady: () => void }) {
  const { camera, size, scene, gl } = useThree();
  const screens = useScreens(manifest);
  const blinds = useMemo(() => blindsTexture(), []);
  const blindsMatrix = useMemo(() => projector(), []);
  const [built, setBuilt] = useState<Built | null>(null);
  const [extrasIn, setExtrasIn] = useState(false);
  const onExtras = useCallback(() => setExtrasIn(true), []);
  const spot = useRef<SpotLight>(null);
  const street = useRef<SpotLight>(null);
  const laptopRect = useRef<RectAreaLight>(null);
  const televisionRect = useRef<RectAreaLight>(null);
  const laptopSpot = useRef<SpotLight>(null);
  const televisionSpot = useRef<SpotLight>(null);
  const rim = useRef<DirectionalLight>(null);
  const inner = useRef<PointLight>(null);
  const targets = useMemo(() => ({ lamp: new Object3D(), street: new Object3D(), laptop: new Object3D(), television: new Object3D() }), []);
  const bulbs = useRef<MeshStandardMaterial[]>([]);
  const scratch = useMemo(
    () => ({ color: new Color(), white: new Color(1, 1, 1), focus: new Vector3(), lampColor: new Color('#ffb46b'), street: tokenColor('rim') }),
    [],
  );
  const room = useMemo(() => tokenColor('room'), []);
  const fog = useMemo(() => new FogExp2(room.getHex(), 0.32), [room]);
  const layout = layoutFor(size.width, size.height);
  const phone = layout === 'phone';
  const wall = useRoomMaterial('wall', wallCenter, blinds);
  const floor = useRoomMaterial('floor', floorCenter, blinds);

  const onScreens = useCallback(
    (found: Screens) => {
      const aspect = manifest.screen.width / manifest.screen.height;
      const laptop = measureScreen(found.laptop.mesh, found.laptop.frame, aspect);
      screens.lcd.uniforms.uOrigin.value.set(laptop.origin.x, laptop.origin.y);
      screens.lcd.uniforms.uExtent.value.set(laptop.origin.width, laptop.origin.height);
      found.laptop.mesh.material = screens.lcd.material;
      found.laptop.mesh.castShadow = false;
      const box = found.television.mesh.geometry.boundingBox ?? (found.television.mesh.geometry.computeBoundingBox(), found.television.mesh.geometry.boundingBox!);
      const glassAspect = (box.max.x - box.min.x) / (box.max.y - box.min.y);
      const television = measureScreen(found.television.mesh, found.television.frame, glassAspect, [1, 1]);
      screens.crt.uniforms.uOrigin.value.set(television.origin.x, television.origin.y);
      screens.crt.uniforms.uExtent.value.set(television.origin.width, television.origin.height);
      found.television.mesh.material = screens.crt.material;
      found.television.mesh.castShadow = false;
      setBuilt({
        laptop: glassOf(found.laptop.mesh, found.laptop.frame, laptop.screen),
        television: glassOf(found.television.mesh, found.television.frame, television.screen),
      });
    },
    [manifest, screens],
  );

  const stage = useMemo(() => (built ? stageFor(built.television) : null), [built]);
  const inside = useInside(manifest, screens.photo, screens.saliency, screens.atlas, stage);

  const rig = useMemo(() => {
    if (!built || !stage) return null;
    const space = new Matrix4().compose(stage.space.position, stage.space.quaternion, new Vector3(1, 1, 1));
    return new Rig(keys[layout], { world: new Matrix4(), glass: built.television.matrix, space }, size.width / size.height);
  }, [built, stage, layout, size.width, size.height]);

  useLayoutEffect(() => {
    scene.background = room;
    scene.fog = fog;
    targets.lamp.position.set(LAMP.position[0] + 0.12, DESK_TOP + 0.05, LAMP.position[2] + 0.42);
    targets.street.position.copy(STREET.target);
    for (const target of Object.values(targets)) scene.add(target);
    return () => {
      for (const target of Object.values(targets)) scene.remove(target);
      scene.fog = null;
    };
  }, [scene, room, fog, targets]);

  useEffect(() => {
    if (!built) return;
    const found: MeshStandardMaterial[] = [];
    scene.traverse((child) => {
      const mesh = child as Mesh;
      if (mesh.isMesh && mesh.userData.bulb) found.push(mesh.material as MeshStandardMaterial);
    });
    bulbs.current = found;
    const placeScreenLight = (glass: Glass, rect: RectAreaLight | null, glow: SpotLight | null, target: Object3D) => {
      const normal = new Vector3(0, 0, 1).applyQuaternion(glass.quaternion);
      if (rect) {
        rect.position.copy(glass.center).addScaledVector(normal, 0.004);
        rect.lookAt(glass.center.clone().add(normal));
        rect.width = glass.screen.width;
        rect.height = glass.screen.height;
      }
      if (glow) {
        glow.position.copy(glass.center).addScaledVector(normal, 0.02);
        target.position.copy(glass.center).addScaledVector(normal, 1).add(new Vector3(0, -0.45, 0));
        glow.target = target;
      }
    };
    placeScreenLight(built.laptop, laptopRect.current, laptopSpot.current, targets.laptop);
    placeScreenLight(built.television, televisionRect.current, televisionSpot.current, targets.television);
    if (spot.current) {
      spot.current.target = targets.lamp;
      spot.current.shadow.needsUpdate = true;
    }
    if (street.current) {
      street.current.target = targets.street;
      street.current.map = blinds;
      street.current.shadow.needsUpdate = true;
    }
    wall.uniforms.uBlindsMatrix.value.copy(blindsMatrix);
    floor.uniforms.uBlindsMatrix.value.copy(blindsMatrix);
    gl.shadowMap.autoUpdate = false;
    gl.shadowMap.needsUpdate = true;
    screens.crt.uniforms.uLamp.value.copy(bulbWorld);
    screens.lcd.uniforms.uLamp.value.copy(bulbWorld);
    let frames = 0;
    let handle = 0;
    let cancelled = false;
    const wait = () => {
      frames += 1;
      if (frames === 1) performance.mark('scooch:frame');
      if (frames < 2) handle = requestAnimationFrame(wait);
      else onReady();
    };
    performance.mark('scooch:assets');
    environmentReady(scene)
      .then(() => precompile(gl, scene, camera, quality.composer))
      .catch(() => undefined)
      .then(() => {
        if (cancelled) return;
        performance.mark('scooch:compiled');
        runtime.compiled = true;
        runtime.resume?.();
        handle = requestAnimationFrame(wait);
      });
    return () => {
      cancelled = true;
      cancelAnimationFrame(handle);
    };
  }, [built, scene, camera, targets, gl, screens, blinds, blindsMatrix, wall, floor, onReady]);

  const shadowFrames = useRef(0);
  const shadowProps = useRef(false);
  const readyAt = useRef(-1);

  useFrame((state, delta) => {
    if (!rig || !built) return;
    if (runtime.props && !shadowProps.current) {
      shadowProps.current = true;
      shadowFrames.current = 0;
    }
    if (shadowFrames.current < 24) {
      gl.shadowMap.needsUpdate = true;
      shadowFrames.current += 1;
    }
    const time = state.clock.elapsedTime;
    const idle = runtime.idle;
    const motion = idle ? 1 : 0;
    rig.apply(camera as PerspectiveCamera, story.camera, time, pointer.x * motion, pointer.y * motion, Math.min(delta, 0.1), size.width, size.height, idle);

    if (runtime.introAt === null && runtime.ready && runtime.still < 0) {
      if (readyAt.current < 0) readyAt.current = time;
      if (story.progress > 0.05) runtime.introAt = time - 10;
      else if (runtime.props || time - readyAt.current > 1.6) runtime.introAt = time + 0.12;
      if (runtime.introAt !== null) performance.mark('scooch:intro');
    }
    let intro = 1;
    if (runtime.still >= 0) intro = runtime.hero ? 0 : 1;
    else if (runtime.introAt === null) intro = 0;
    else intro = time - runtime.introAt;
    const still = runtime.still >= 0;
    const introLamp = still ? intro : intro >= 1.6 ? 1 : flicker(intro);
    const introStreet = still ? intro : ramp(intro, 0.5, 1.3);
    const introTube = still ? intro : ramp(intro, 0.9, 1.0);
    const introLcd = still ? intro : ramp(intro, 1.3, 0.6);

    const away = 1 - story.inside;
    const hum = idle ? 1 + Math.sin(time * 13.1) * 0.006 + Math.sin(time * 29.7 + 1.1) * 0.004 + Math.sin(time * 7.3) * 0.003 : 1;
    const lamp = story.lamp * introLamp * hum * away;
    if (spot.current) spot.current.intensity = SPOT * lamp;
    if (inner.current) inner.current.intensity = INNER * lamp;
    wall.uniforms.uLevel.value = lamp;
    floor.uniforms.uLevel.value = lamp * 0.35;
    const streetLevel = story.blinds * away;
    wall.uniforms.uBlindsLevel.value = streetLevel;
    floor.uniforms.uBlindsLevel.value = streetLevel * 0.6;
    wall.uniforms.uFade.value = 0.25 + 0.75 * away;
    floor.uniforms.uFade.value = 0.25 + 0.75 * away;
    if (street.current) street.current.intensity = STREET_LEVEL * streetLevel * introStreet;
    const ambient = Math.min(story.lamp, 1) * introLamp * away;
    scene.environmentIntensity = (0.05 + 0.17 * ambient) * (0.3 + 0.7 * away);
    if (rim.current) rim.current.intensity = (0.12 + 0.2 * ambient) * away;
    for (const bulb of bulbs.current) bulb.emissiveIntensity = BULB_GLOW * story.lamp * introLamp * hum;
    fog.density = 0.32 + story.inside * 0.9;

    const tubeOn = Math.min(story.tubeOn, introTube);
    const crt = screens.crt.uniforms;
    crt.uTime.value = time;
    crt.uPlace.value = story.place;
    crt.uOn.value = tubeOn;
    crt.uOff.value = story.tubeOff;
    crt.uLampLevel.value = lamp;
    const { frames, drag } = manifest;
    for (let i = 0; i < manifest.icons.length && i < crt.uIcons.value.length; i++) {
      const id = manifest.icons[i].id;
      const [x, y, w, h] = frames.before.icons[id];
      if (id === drag.icon) {
        crt.uIcons.value[i].set(drag.from[0] + (drag.to[0] - drag.from[0]) * story.drag, drag.from[1] + (drag.to[1] - drag.from[1]) * story.drag, w, h);
      } else {
        crt.uIcons.value[i].set(x, y, w, h);
      }
    }

    const lcdPower = introLcd * away * away;
    const lcd = screens.lcd.uniforms;
    lcd.uTime.value = time;
    lcd.uPlace.value = story.frost;
    lcd.uPower.value = lcdPower;
    lcd.uLampLevel.value = lamp;

    const lightFor = (averages: Color[], place: number) => {
      const step = Math.min(Math.max(place, 0), averages.length - 1);
      const index = Math.min(Math.floor(step), averages.length - 2);
      return scratch.color.copy(averages[index]).lerp(averages[index + 1], step - index).lerp(scratch.white, 0.3);
    };
    const tubeLight = tubeOn * (1 - Math.min(story.tubeOff * 1.4, 1));
    const laptopLight = laptopRect.current ?? laptopSpot.current;
    if (laptopLight) {
      laptopLight.color.copy(lightFor(screens.frost, story.frost));
      laptopLight.intensity = (laptopRect.current ? RECT : SCREEN_SPOT) * (1.5 - 0.5 * Math.min(story.lamp, 1)) * lcdPower;
    }
    const televisionLight = televisionRect.current ?? televisionSpot.current;
    if (televisionLight) {
      televisionLight.color.copy(lightFor(screens.balloon, story.place));
      televisionLight.intensity = (televisionRect.current ? RECT * 1.3 : SCREEN_SPOT * 1.4) * (1.5 - 0.5 * Math.min(story.lamp, 1)) * tubeLight * away;
    }

    scratch.focus.copy(built.laptop.center).lerp(built.television.center, story.focus);
    quality.focus(scratch.focus, camera.position, 1, scratch.focus, scratch.focus, story.dof);
  });

  return (
    <>
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
      <spotLight
        ref={street}
        position={STREET.position}
        color={scratch.street}
        angle={STREET.angle}
        penumbra={0.25}
        decay={2}
        distance={0}
        intensity={0}
        castShadow
        shadow-mapSize={phone ? [1024, 1024] : [1536, 1536]}
        shadow-radius={3}
        shadow-bias={-0.0004}
        shadow-normalBias={0.02}
        shadow-camera-near={1.2}
        shadow-camera-far={6}
      />
      <pointLight ref={inner} position={bulbWorld} color="#ffb46b" intensity={INNER} distance={0.32} decay={2} />
      <directionalLight ref={rim} position={[1.4, 1.8, -1.6]} color={scratch.street} intensity={0.3} />
      {quality.tier === 'full' ? (
        <>
          <rectAreaLight ref={laptopRect} intensity={RECT} />
          <rectAreaLight ref={televisionRect} intensity={RECT} />
        </>
      ) : (
        <>
          <spotLight ref={laptopSpot} angle={1.1} penumbra={1} decay={2} distance={0} intensity={0} />
          <spotLight ref={televisionSpot} angle={1.1} penumbra={1} decay={2} distance={0} intensity={0} />
        </>
      )}
      <Desk onScreens={onScreens} onExtras={onExtras} phone={phone} />
      <ContactShadows key={extrasIn ? 'extras' : 'bare'} position={[0, DESK_TOP + 0.0015, 0.0]} scale={[2.0, 0.95]} resolution={512} blur={2.4} far={0.3} opacity={0.62} frames={1} color="#0a0806" />
      <mesh position={[0, 1.6, -0.98]} material={wall}>
        <planeGeometry args={[9, 3.2]} />
      </mesh>
      <mesh rotation-x={-Math.PI / 2} position={[0, 0, 0.6]} material={floor}>
        <planeGeometry args={[9, 4]} />
      </mesh>
      {inside ? <primitive object={inside.group} /> : null}
    </>
  );
}
