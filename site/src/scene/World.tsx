import { ContactShadows } from '@react-three/drei';
import { useFrame, useLoader, useThree } from '@react-three/fiber';
import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import {
  Color,
  DataTexture,
  DirectionalLight,
  FogExp2,
  Group,
  HalfFloatType,
  Matrix4,
  Mesh,
  MeshStandardMaterial,
  Object3D,
  PerspectiveCamera,
  PointLight,
  Quaternion,
  RectAreaLight,
  RGBAFormat,
  SpotLight,
  type Texture,
  UnsignedByteType,
  Vector2,
  Vector3,
  WebGLRenderTarget,
} from 'three';
import { HDRLoader } from 'three/examples/jsm/loaders/HDRLoader.js';
import { asset } from '@/lib/base';
import { quiet } from '@/lib/idle';
import { stage as loadStage } from '@/lib/load';
import { tokenColor } from '@/lib/tokens';
import type { DemoManifest } from '@/story/demo';
import { pointer, runtime, story } from '@/story/state';
import { STREET, blindsTexture, projector } from './Blinds';
import { fontsReady, osdPainter, paintedTexture, wallTexture } from './captions';
import { stickyNote } from './labels';
import { Tumbler } from './Tumbler';
import { CHANNELS, hooks, live } from './touch';
import { BULB, DESK_TOP, Desk, LAMP, type Console, type Screens } from './Desk';
import { createCrtMaterial } from './screen/crt';
import { useInside, type Stage } from './Inside';
import { quality } from './quality';
import { useRoomMaterial } from './Room';
import { Rig, type ScreenFrame } from './rig';
import { createFrame, loadScenery, streamScenery, type Frame } from './screen/scenery';
import { measureScreen, useScreens } from './screen/useScreen';
import { keys, layoutFor } from './shots';
import { applyEnvironment, environmentReady, precompile } from './warm';

const Lively = lazy(() => import('./Lively'));

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
const lampAim = new Vector3(LAMP.position[0] + 0.12, DESK_TOP + 0.05, LAMP.position[2] + 0.42);
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
  const writing = useRef<((amount: number) => void) | null>(null);
  const aimed = useRef(0);
  const blindsMatrix = useMemo(() => projector(), []);
  const [lively, setLively] = useState(false);
  useEffect(() => {
    if (runtime.still >= 0) return;
    let stopped = false;
    let timer = 0;
    const wake = async () => {
      if (stopped) return;
      if (!runtime.shown) {
        timer = window.setTimeout(() => void wake(), 200);
        return;
      }
      await quiet();
      if (!stopped) setLively(true);
    };
    void wake();
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, []);
  const placeholder = useMemo(() => {
    const texture = new DataTexture(new Uint8Array(4), 1, 1, RGBAFormat, UnsignedByteType);
    texture.needsUpdate = true;
    return texture;
  }, []);
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
      hooks.frame = screens.main;
      hooks.icons = screens.crt.uniforms.uIcons.value;
      hooks.origin = screens.crt.uniforms.uOrigin.value;
      hooks.extent = screens.crt.uniforms.uExtent.value;
      hooks.place = () => story.place;
      found.television.mesh.castShadow = false;
      setBuilt({
        laptop: glassOf(found.laptop.mesh, found.laptop.frame, laptop.screen),
        television: glassOf(found.television.mesh, found.television.frame, television.screen),
      });
    },
    [manifest, screens],
  );

  const stage = useMemo(() => (built ? stageFor(built.television) : null), [built]);
  const consoleScreen = useMemo(() => {
    const crt = createCrtMaterial();
    crt.uniforms.uAtlas.value = screens.atlas;
    crt.uniforms.uRim.value = tokenColor('rim');
    crt.uniforms.uLamp.value.copy(bulbWorld);
    const moon = manifest.pictures.find((picture) => picture.id === 'moon');
    manifest.icons.forEach((icon, index) => {
      const rect = moon?.icons[icon.id];
      if (rect) crt.uniforms.uIcons.value[index]?.set(...rect);
    });
    return crt;
  }, [screens, manifest]);
  const [consoleGlass, setConsoleGlass] = useState<Glass | null>(null);
  const onConsole = useCallback(
    (found: Console) => {
      const geometry = found.mesh.geometry;
      if (!geometry.boundingBox) geometry.computeBoundingBox();
      const box = geometry.boundingBox!;
      const aspect = (box.max.x - box.min.x) / (box.max.y - box.min.y);
      const measured = measureScreen(found.mesh, found.frame as Group, aspect, [1, 1]);
      consoleScreen.uniforms.uOrigin.value.set(measured.origin.x, measured.origin.y);
      consoleScreen.uniforms.uExtent.value.set(measured.origin.width, measured.origin.height);
      found.mesh.material = consoleScreen.material;
      found.mesh.castShadow = false;
      setConsoleGlass(glassOf(found.mesh, found.frame as Group, measured.screen));
    },
    [consoleScreen],
  );
  const pads = useMemo(() => (built ? { laptop: built.laptop, console: consoleGlass } : null), [built, consoleGlass]);
  const gallery = useRef<Record<string, Frame>>({});
  useEffect(() => {
    hooks.showing = () => {
      const busy: string[] = [];
      if (second.current && screens.lcd.uniforms.uPower.value > 0.05) busy.push('swans');
      if (consoleScreen.uniforms.uOn.value > 0.05) busy.push('moon');
      return busy;
    };
    hooks.current = () => (story.city > 0.5 && gallery.current.city ? 'city' : 'main');
  }, [screens, consoleScreen]);
  const inside = useInside(manifest, screens.main, screens.saliency, screens.atlas, stage, gallery, pads);
  const [insideIn, setInsideIn] = useState(runtime.still >= 0);
  const second = useRef<Frame | null>(null);

  useEffect(() => {
    let stopped = false;
    let timer = 0;
    const begin = async () => {
      if (stopped) return;
      if (!runtime.shown && runtime.still < 0) {
        timer = window.setTimeout(() => void begin(), 200);
        return;
      }
      const upload = async (textures: Texture[]) => {
        for (const texture of textures) {
          await quiet();
          if (stopped) return;
          gl.initTexture(texture);
        }
      };
      const ready = new Set(screens.main.textures());
      await streamScenery(screens.main.scenery);
      await upload(screens.main.textures().filter((texture) => !ready.has(texture)));
      const buffer = gl.getDrawingBufferSize(new Vector2());
      const limit = Math.round(Math.max(buffer.x, buffer.y) * 0.8);
      const order = ['swans', 'wide', 'tall', 'city', 'moon'];
      const rank = (id: string) => (order.includes(id) ? order.indexOf(id) : order.length);
      for (const picture of [...manifest.pictures].sort((a, b) => rank(a.id) - rank(b.id))) {
        const { width, height } = picture.scenery;
        const scale = Math.min(1, limit / Math.max(width, height));
        const view = { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) };
        const loaded = await loadScenery(picture.scenery, false, Math.round(Math.max(view.width, view.height) * 1.25));
        if (stopped) return;
        const frame = createFrame(loaded, view);
        await quiet();
        await frame.compile(gl);
        await upload(frame.textures());
        if (stopped) return;
        await frame.renderIdle(gl, 0);
        if (stopped) return;
        gallery.current[picture.id] = frame;
        if (picture.id === 'swans') {
          second.current = frame;
          screens.lcd.uniforms.uFrame.value = frame.texture;
          runtime.scenery = true;
        }
      }
      runtime.scenery = true;
      runtime.gallery = true;
    };
    timer = window.setTimeout(() => void begin(), 0);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [screens, manifest, gl]);

  useEffect(() => {
    if (!inside || insideIn) return;
    let stopped = false;
    let timer = 0;
    const begin = () => {
      if (stopped) return;
      if (!runtime.ready) {
        timer = window.setTimeout(begin, 200);
        return;
      }
      const target = new WebGLRenderTarget(1, 1, { type: HalfFloatType });
      const previous = gl.getRenderTarget();
      gl.setRenderTarget(target);
      const pending = gl.compileAsync(inside.group, camera, scene);
      gl.setRenderTarget(previous);
      void pending
        .catch(() => undefined)
        .then(() => {
          target.dispose();
          if (!stopped) setInsideIn(true);
        });
    };
    timer = window.setTimeout(begin, 0);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [inside, insideIn, gl, camera, scene]);

  const rig = useMemo(() => {
    if (!built || !stage) return null;
    const space = new Matrix4().compose(stage.space.position, stage.space.quaternion, new Vector3(1, 1, 1));
    return new Rig(keys[layout], { world: new Matrix4(), glass: built.television.matrix, space }, size.width / size.height);
  }, [built, stage, layout, size.width, size.height]);

  useLayoutEffect(() => {
    scene.background = room;
    scene.fog = fog;
    targets.lamp.position.copy(lampAim);
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
      if (quality.tier === 'full') {
        street.current.map = blinds;
        street.current.shadow.needsUpdate = true;
      }
    }
    wall.uniforms.uBlindsMatrix.value.copy(blindsMatrix);
    floor.uniforms.uBlindsMatrix.value.copy(blindsMatrix);
    wall.uniforms.uCaption.value = placeholder;
    screens.crt.uniforms.uOsd.value = placeholder;
    gl.shadowMap.autoUpdate = false;
    gl.shadowMap.needsUpdate = true;
    screens.crt.uniforms.uLamp.value.copy(bulbWorld);
    screens.lcd.uniforms.uLamp.value.copy(bulbWorld);
    let frames = 0;
    let handle = 0;
    let cancelled = false;
    const wait = () => {
      frames += 1;
      if (frames === 1) {
        performance.mark('scooch:frame');
        loadStage(0.95);
      }
      if (frames < 2) handle = requestAnimationFrame(wait);
      else onReady();
    };
    performance.mark('scooch:assets');
    loadStage(0.15);
    environmentReady(scene)
      .then(() => loadStage(0.35))
      .then(() => precompile(gl, scene, camera, quality.composer, [screens.main.compile(gl)]))
      .catch(() => undefined)
      .then(() => {
        if (cancelled) return;
        performance.mark('scooch:compiled');
        loadStage(0.8);
        runtime.compiled = true;
        runtime.resume?.();
        handle = requestAnimationFrame(wait);
      });
    return () => {
      cancelled = true;
      cancelAnimationFrame(handle);
    };
  }, [built, scene, camera, targets, gl, screens, blinds, blindsMatrix, wall, floor, onReady, placeholder]);

  useEffect(() => {
    if (!built) return;
    let stopped = false;
    let timer = 0;
    let note: Mesh | null = null;
    const paint = async () => {
      if (stopped) return;
      if (!runtime.shown && runtime.still < 0) {
        timer = window.setTimeout(() => void paint(), 200);
        return;
      }
      await fontsReady();
      await quiet();
      if (stopped) return;
      const osd = paintedTexture(1024, 768, osdPainter);
      await quiet();
      gl.initTexture(osd.texture);
      screens.crt.uniforms.uOsd.value = osd.texture;
      const caption = wallTexture(blindsMatrix);
      await caption.paint();
      if (stopped) return;
      await quiet();
      gl.initTexture(caption.texture);
      wall.uniforms.uCaption.value = caption.texture;
      const tv = built.television;
      const sticky = stickyNote({
        center: tv.center,
        normal: new Vector3(0, 0, 1).applyQuaternion(tv.quaternion),
        up: new Vector3(0, 1, 0).applyQuaternion(tv.quaternion),
        right: new Vector3(1, 0, 0).applyQuaternion(tv.quaternion),
        width: tv.screen.width,
        height: tv.screen.height,
      });
      await sticky.ready(gl);
      if (stopped) return;
      await quiet();
      const target = new WebGLRenderTarget(1, 1, { type: HalfFloatType });
      const previous = gl.getRenderTarget();
      gl.setRenderTarget(target);
      const pending = gl.compileAsync(sticky.note, camera, scene);
      gl.setRenderTarget(previous);
      await pending.catch(() => undefined);
      target.dispose();
      if (stopped) return;
      note = sticky.note;
      scene.add(note);
      writing.current = sticky.write;
      runtime.painted = true;
    };
    timer = window.setTimeout(() => void paint(), 0);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
      if (note) scene.remove(note);
      writing.current = null;
    };
  }, [built, gl, camera, scene, screens, wall, blindsMatrix]);

  const shadowFrames = useRef(0);
  const shadowProps = useRef(false);
  const readyAt = useRef(-1);

  useFrame((state, delta) => {
    if (!rig || !built) return;
    if (runtime.props && !shadowProps.current) {
      shadowProps.current = true;
      shadowFrames.current = 0;
    }
    if (shadowFrames.current < 4) {
      gl.shadowMap.needsUpdate = true;
      shadowFrames.current += 1;
    }
    const time = state.clock.elapsedTime;
    const idle = runtime.idle;
    const motion = idle ? 1 : 0;
    rig.apply(camera as PerspectiveCamera, story.camera, time, pointer.x * motion + live.look.x, pointer.y * motion + live.look.y, Math.min(delta, 0.1), size.width, size.height, idle);

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
    const lamp = story.lamp * live.lamp * introLamp * hum * away;
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
    const ambient = Math.min(story.lamp * live.lamp, 1) * introLamp * away;
    scene.environmentIntensity = (0.05 + 0.17 * ambient) * (0.3 + 0.7 * away);
    if (rim.current) rim.current.intensity = (0.12 + 0.2 * ambient) * away;
    for (const bulb of bulbs.current) bulb.emissiveIntensity = BULB_GLOW * story.lamp * live.lamp * introLamp * hum;
    if (Math.abs(live.aim - aimed.current) > 0.0005 && spot.current) {
      aimed.current = live.aim;
      const cos = Math.cos(live.aim);
      const sin = Math.sin(live.aim);
      const turn = (point: Vector3, out: Vector3) => {
        const x = point.x - LAMP.position[0];
        const z = point.z - LAMP.position[2];
        return out.set(LAMP.position[0] + x * cos + z * sin, point.y, LAMP.position[2] - x * sin + z * cos);
      };
      turn(bulbWorld, spot.current.position);
      turn(lampAim, targets.lamp.position);
      targets.lamp.updateMatrixWorld();
      if (inner.current) inner.current.position.copy(spot.current.position);
      gl.shadowMap.needsUpdate = true;
    }
    fog.density = 0.32 + story.inside * 0.25;

    const tubeOn = Math.min(story.tubeOn, introTube);
    const crt = screens.crt.uniforms;
    const city = gallery.current.city;
    const channel = story.city > 0.5 && city ? city.texture : screens.main.texture;
    if (crt.uFrame.value !== channel) crt.uFrame.value = channel;
    const big = consoleScreen.uniforms;
    const moon = gallery.current.moon;
    if (moon && big.uFrame.value !== moon.texture) big.uFrame.value = moon.texture;
    big.uTime.value = time;
    big.uOn.value = moon ? Math.min(Math.max((story.fly - 1.7) / 0.3, 0), 1) : 0;
    big.uLampLevel.value = story.lamp * introLamp;
    crt.uOsdLevel.value = story.osd;
    writing.current?.(story.written);
    wall.uniforms.uCaptionLevel.value = phone ? 0 : story.wall;
    wall.uniforms.uDrift.value.set(story.drift * 0.018, -story.drift * 0.028);
    floor.uniforms.uDrift.value.copy(wall.uniforms.uDrift.value);
    crt.uTime.value = time;
    if (!hooks.drawHeld(gl, Math.min(delta, 0.05))) screens.main.render(gl, story.place);
    for (const frame of Object.values(gallery.current)) frame.render(gl, 0);
    const now = performance.now();
    crt.uOn.value = live.power ? Math.min(tubeOn, Math.min(Math.max((now - live.onAt) / 1100, 0), 1)) : tubeOn;
    crt.uOff.value = live.power ? story.tubeOff : Math.max(story.tubeOff, Math.min(Math.max((now - live.offAt) / 700, 0), 1));
    crt.uStatic.value = Math.max(0, 1 - (now - live.noiseAt) / 380);
    const tuned = live.channel >= 0 ? CHANNELS[live.channel] : '';
    const tunedFrame = tuned === 'main' ? screens.main : tuned ? gallery.current[tuned] : null;
    if (tunedFrame && crt.uFrame.value !== tunedFrame.texture) crt.uFrame.value = tunedFrame.texture;
    const tunedIcons = tuned && tuned !== 'main' ? manifest.pictures.find((picture) => picture.id === tuned)?.icons : null;
    crt.uLampLevel.value = lamp;
    const { frames, drag } = manifest;
    for (let i = 0; i < manifest.icons.length && i < crt.uIcons.value.length; i++) {
      const id = manifest.icons[i].id;
      const [x, y, w, h] = tunedIcons?.[id] ?? frames.before.icons[id];
      if (tunedIcons) {
        crt.uIcons.value[i].set(x, y, w, h);
      } else if (live.icon.index === i) {
        crt.uIcons.value[i].set(live.icon.x, live.icon.y, w, h);
      } else if (id === drag.icon) {
        crt.uIcons.value[i].set(drag.from[0] + (drag.to[0] - drag.from[0]) * story.drag, drag.from[1] + (drag.to[1] - drag.from[1]) * story.drag, w, h);
      } else {
        crt.uIcons.value[i].set(x, y, w, h);
      }
    }

    const lcdPower = second.current ? introLcd * away * away : 0;
    const lcd = screens.lcd.uniforms;
    lcd.uTime.value = time;
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
      laptopLight.color.copy(lightFor(screens.secondColor, 0));
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
        castShadow={quality.tier === 'full'}
        shadow-mapSize={[1536, 1536]}
        shadow-radius={3}
        shadow-bias={-0.0004}
        shadow-normalBias={0.02}
        shadow-camera-near={1.2}
        shadow-camera-far={6}
      />
      {quality.tier === 'full' ? <pointLight ref={inner} position={bulbWorld} color="#ffb46b" intensity={INNER} distance={0.32} decay={2} /> : null}
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
      <Desk onScreens={onScreens} onExtras={onExtras} onConsole={onConsole} phone={phone} />
      <Tumbler />
      {lively ? (
        <Suspense fallback={null}>
          <Lively />
        </Suspense>
      ) : null}
      <ContactShadows key={extrasIn ? 'extras' : 'bare'} position={[0, DESK_TOP + 0.0015, 0.0]} scale={[2.0, 0.95]} resolution={512} blur={2.4} far={0.3} opacity={0.62} frames={1} color="#0a0806" />
      <mesh position={[0, 1.6, -0.98]} material={wall}>
        <planeGeometry args={[9, 3.2]} />
      </mesh>
      <mesh rotation-x={-Math.PI / 2} position={[0, 0, 0.6]} material={floor}>
        <planeGeometry args={[9, 4]} />
      </mesh>
      {inside && insideIn ? <primitive object={inside.group} /> : null}
    </>
  );
}
