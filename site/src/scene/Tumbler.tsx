import { useGLTF } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import { Group, LatheGeometry, Mesh, MeshStandardMaterial, Object3D, ShaderChunk, Vector2, Vector3, type Camera } from 'three';
import { asset } from '@/lib/base';
import { tokenColor } from '@/lib/tokens';
import { runtime, story } from '@/story/state';
import { DESK_TOP } from './layout';
import { live, makeLive, rimmed, things } from './touch';
import { placeStripe, windowLit } from './window';

const url = asset('models/tumbler.glb');
const HOME = { x: 0.235, z: 0.15 };
const TOP = 0.082;
const query = new URLSearchParams(window.location.search);
const emptyStill = query.has('hero') && !query.has('juice');

useGLTF.preload(url, false, true);

const wrapped = ShaderChunk.lights_physical_pars_fragment.replace(
  'reflectedLight.directDiffuse += irradiance * diffuseBRDF * ( 1.0 - F );',
  'reflectedLight.directDiffuse += mix( irradiance, saturate( ( dot( geometryNormal, directLight.direction ) + 0.75 ) / 1.75 ) * directLight.color, uJuice ) * diffuseBRDF * ( 1.0 - F );',
);

const edge =
  'if ( uJuice < 0.5 ) {\nfloat edge = pow(1.0 - abs(dot(normal, normalize(vViewPosition))), 2.2);\ndiffuseColor.a = mix(0.05, 0.5, edge);\noutgoingLight = min(outgoingLight + vec3(0.62, 0.66, 0.68) * edge * 0.09, vec3(0.5));\n}\n#include <opaque_fragment>';

function tumblerMaterial(juice: boolean) {
  const material = juice
    ? new MeshStandardMaterial({ color: tokenColor('lamp'), transparent: true, opacity: 1, roughness: 0.24, metalness: 0 })
    : new MeshStandardMaterial({ color: '#e7eef0', transparent: true, opacity: 1, roughness: 0.2, metalness: 0, depthWrite: false });
  const kind = { value: juice ? 1 : 0 };
  material.onBeforeCompile = (shader) => {
    shader.uniforms.uJuice = kind;
    shader.fragmentShader = `uniform float uJuice;\n${shader.fragmentShader.replace('#include <lights_physical_pars_fragment>', wrapped).replace('#include <opaque_fragment>', edge)}`;
  };
  material.customProgramCacheKey = () => 'tumbler';
  material.defines = { WINDOW_WRAP: '' };
  windowLit(material);
  rimmed(material);
  return material;
}

function radiusAt(profile: [number, number][], y: number) {
  if (y <= profile[0][1]) return profile[0][0];
  for (let n = 1; n < profile.length; n++) {
    const [r1, y1] = profile[n];
    const [r0, y0] = profile[n - 1];
    if (y <= y1) return r0 + ((r1 - r0) * (y - y0)) / (y1 - y0);
  }
  return profile[profile.length - 1][0];
}

function juiceShape(profile: [number, number][], floor: number, level: number) {
  const top = floor + (TOP - floor) * level;
  const points = [new Vector2(0, floor), new Vector2(radiusAt(profile, floor + 0.0004) * 0.97, floor)];
  for (const [, y] of profile) if (y > floor + 0.0004 && y < top) points.push(new Vector2(radiusAt(profile, y) * 0.97, y));
  points.push(new Vector2(radiusAt(profile, top) * 0.97, top), new Vector2(0, top));
  return new LatheGeometry(points, 48);
}

function hull(points: [number, number][]) {
  const sorted = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross = (o: [number, number], a: [number, number], b: [number, number]) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const lower: [number, number][] = [];
  for (const p of sorted) {
    while (lower.length >= 2 && cross(lower[lower.length - 2], lower[lower.length - 1], p) <= 0) lower.pop();
    lower.push(p);
  }
  const upper: [number, number][] = [];
  for (const p of [...sorted].reverse()) {
    while (upper.length >= 2 && cross(upper[upper.length - 2], upper[upper.length - 1], p) <= 0) upper.pop();
    upper.push(p);
  }
  return [...lower.slice(0, -1), ...upper.slice(0, -1)];
}

function outline(holder: Object3D, profile: [number, number][], floor: number, camera: Camera, width: number, height: number) {
  holder.updateWorldMatrix(true, true);
  const point = new Vector3();
  const project = (r: number, y: number, angle: number): [number, number] => {
    point.set(Math.cos(angle) * r, y, Math.sin(angle) * r);
    holder.localToWorld(point).project(camera);
    return [(point.x * 0.5 + 0.5) * width, (0.5 - point.y * 0.5) * height];
  };
  const all: [number, number][] = [];
  const rows = [floor, ...profile.map(([, y]) => y).filter((y) => y > floor && y < TOP), TOP];
  for (const y of rows) for (let a = 0; a < 64; a++) all.push(project(radiusAt(profile, y) * 0.97, y, (a / 64) * Math.PI * 2));
  const ring: [number, number][] = [];
  for (let a = 0; a < 64; a++) ring.push(project(radiusAt(profile, TOP) * 0.97, TOP, (a / 64) * Math.PI * 2));
  const xs = ring.map((p) => p[0]);
  const ys = ring.map((p) => p[1]);
  return {
    hull: hull(all),
    top: { cx: (Math.min(...xs) + Math.max(...xs)) / 2, cy: (Math.min(...ys) + Math.max(...ys)) / 2, rx: (Math.max(...xs) - Math.min(...xs)) / 2, ry: (Math.max(...ys) - Math.min(...ys)) / 2 },
    bottom: Math.max(...all.map((p) => p[1])),
  };
}

export function Tumbler() {
  const gltf = useGLTF(url, false, true);
  const camera = useThree((state) => state.camera);
  const size = useThree((state) => state.size);
  const shown = useRef(emptyStill ? 0 : 1);
  const built = useMemo(() => {
    let source = null as Mesh | null;
    gltf.scene.traverse((child) => {
      if ((child as Mesh).isMesh && !source) source = child as Mesh;
    });
    if (!source) throw new Error('tumbler has no mesh');
    const extras = source.userData as { floor: number; inner: [number, number][] };
    const holder = new Group();
    holder.name = 'tumbler';
    holder.position.set(HOME.x, DESK_TOP, HOME.z);
    const glass = new Mesh(source.geometry, tumblerMaterial(false));
    source.updateWorldMatrix(true, false);
    glass.applyMatrix4(source.matrixWorld);
    glass.renderOrder = 2;
    const juice = new Mesh(juiceShape(extras.inner, extras.floor, 1), tumblerMaterial(true));
    juice.name = 'juice';
    juice.castShadow = true;
    juice.receiveShadow = true;
    const aim = new Object3D();
    aim.position.set(HOME.x, DESK_TOP + 0.045, HOME.z);
    holder.add(juice, glass);
    makeLive(holder, 'glass');
    placeStripe(aim.position);
    return { holder, juice, glass, floor: extras.floor, profile: extras.inner, level: 1 };
  }, [gltf]);

  useEffect(() => {
    if (runtime.still < 0) return;
    const hook = window as Window & { __juice?: () => ReturnType<typeof outline> };
    hook.__juice = () => outline(built.holder, built.profile, built.floor, camera, size.width, size.height);
    return () => {
      delete hook.__juice;
    };
  }, [built, camera, size]);

  useEffect(
    () => () => {
      built.juice.geometry.dispose();
      if (things.glass === built.holder) delete things.glass;
    },
    [built],
  );

  useFrame((_, delta) => {
    const now = performance.now();
    const step = Math.min(delta, 0.05);
    const { holder } = built;
    if (!live.glass.ready) {
      live.glass.x = HOME.x;
      live.glass.z = HOME.z;
      live.glass.ready = true;
    }
    const lastX = holder.position.x;
    const lastZ = holder.position.z;
    holder.position.x += (live.glass.x - holder.position.x) * (1 - Math.exp(-step * 9));
    holder.position.z += (live.glass.z - holder.position.z) * (1 - Math.exp(-step * 9));
    const lean = holder.userData.lean ?? (holder.userData.lean = { x: 0, z: 0 });
    const speedX = step > 0 ? (holder.position.x - lastX) / step : 0;
    const speedZ = step > 0 ? (holder.position.z - lastZ) / step : 0;
    lean.x += (Math.max(-0.14, Math.min(0.14, speedZ * 0.3)) - lean.x) * (1 - Math.exp(-step * 14));
    lean.z += (Math.max(-0.14, Math.min(0.14, -speedX * 0.3)) - lean.z) * (1 - Math.exp(-step * 14));
    const t = (now - live.sipAt) / 1000;
    const sipping = t > 0 && t < 1.6;
    const lift = sipping ? Math.sin(Math.min(t / 1.6, 1) * Math.PI) : 0;
    const tip = sipping ? Math.max(0, Math.sin(Math.min(t / 1.6, 1) * Math.PI) - 0.35) * 0.75 : 0;
    const moving = Math.hypot(live.glass.x - holder.position.x, live.glass.z - holder.position.z) > 0.002;
    holder.position.y = DESK_TOP + lift * 0.09 + (moving ? 0.005 : 0) * (1 - lift);
    holder.rotation.set(lean.x + tip, 0, lean.z);
    const target = runtime.still >= 0 ? (emptyStill ? 0 : 1) : live.juice / 4;
    const settled = !sipping || t > 0.75;
    if (settled) {
      const rate = target > shown.current ? 2.4 : 5;
      shown.current += (target - shown.current) * (1 - Math.exp(-step * rate));
      if (Math.abs(target - shown.current) < 0.002) shown.current = target;
    }
    const level = Math.round(shown.current * 200) / 200;
    if (level !== built.level) {
      built.level = level;
      built.juice.visible = level > 0.004;
      if (built.juice.visible) {
        const next = juiceShape(built.profile, built.floor, level);
        built.juice.geometry.dispose();
        built.juice.geometry = next;
      }
    }
  });

  return (
    <>
      <primitive object={built.holder} />
    </>
  );
}
