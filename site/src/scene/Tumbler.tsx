import { useGLTF } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import { DataTexture, Group, LatheGeometry, LinearFilter, Mesh, MeshStandardMaterial, Object3D, RGBAFormat, ShaderChunk, SpotLight, UnsignedByteType, Vector2, Vector3, type Camera } from 'three';
import { asset } from '@/lib/base';
import { tokenColor } from '@/lib/tokens';
import { runtime, story } from '@/story/state';
import { STREET } from './Blinds';
import { DESK_TOP } from './layout';
import { live, makeLive, things } from './touch';

const url = asset('models/tumbler.glb');
const HOME = { x: 0.235, z: 0.15 };
const TOP = 0.082;
const STRIPE = 64;
const query = new URLSearchParams(window.location.search);
const emptyStill = query.has('hero') && !query.has('juice');

useGLTF.preload(url, false, true);

function stripeTexture() {
  const size = 128;
  const data = new Uint8Array(4 * size * 4);
  const band = (v: number, center: number) => Math.min(Math.max((0.11 - Math.abs(v - center)) / 0.03, 0), 1);
  for (let y = 0; y < size; y++) {
    const v = y / (size - 1);
    const value = Math.round(255 * Math.max(band(v, 0.5), band(v, 0.1) * 0.45, band(v, 0.9) * 0.45));
    for (let x = 0; x < 4; x++) data.set([value, value, value, 255], (y * 4 + x) * 4);
  }
  const texture = new DataTexture(data, 4, size, RGBAFormat, UnsignedByteType);
  texture.magFilter = LinearFilter;
  texture.minFilter = LinearFilter;
  texture.needsUpdate = true;
  return texture;
}

function glassMaterial() {
  const material = new MeshStandardMaterial({ color: '#e7eef0', transparent: true, opacity: 1, roughness: 0.2, metalness: 0, depthWrite: false });
  material.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace(
      '#include <opaque_fragment>',
      'float edge = pow(1.0 - abs(dot(normal, normalize(vViewPosition))), 2.2);\ndiffuseColor.a = mix(0.05, 0.5, edge);\noutgoingLight = min(outgoingLight + vec3(0.62, 0.66, 0.68) * edge * 0.09, vec3(0.5));\n#include <opaque_fragment>',
    );
  };
  material.customProgramCacheKey = () => 'tumbler-glass';
  return material;
}

function juiceMaterial() {
  const material = new MeshStandardMaterial({ color: tokenColor('lamp'), roughness: 0.24, metalness: 0 });
  material.onBeforeCompile = (shader) => {
    shader.fragmentShader = shader.fragmentShader.replace(
      '#include <lights_physical_pars_fragment>',
      ShaderChunk.lights_physical_pars_fragment.replace(
        'reflectedLight.directDiffuse += irradiance * diffuseBRDF * ( 1.0 - F );',
        'reflectedLight.directDiffuse += saturate( ( dot( geometryNormal, directLight.direction ) + 0.75 ) / 1.75 ) * directLight.color * diffuseBRDF * ( 1.0 - F );',
      ),
    );
  };
  material.customProgramCacheKey = () => 'tumbler-juice';
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
  const light = useRef<SpotLight>(null);
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
    const glass = new Mesh(source.geometry, glassMaterial());
    source.updateWorldMatrix(true, false);
    glass.applyMatrix4(source.matrixWorld);
    glass.renderOrder = 2;
    const juice = new Mesh(juiceShape(extras.inner, extras.floor, 1), juiceMaterial());
    juice.name = 'juice';
    juice.castShadow = true;
    juice.receiveShadow = true;
    const aim = new Object3D();
    aim.position.y = 0.045;
    holder.add(juice, glass, aim);
    makeLive(holder, 'glass');
    return { holder, aim, juice, glass, floor: extras.floor, profile: extras.inner, level: 1, map: stripeTexture() };
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
      built.map.dispose();
      if (things.glass === built.holder) delete things.glass;
    },
    [built],
  );

  useFrame((_, delta) => {
    const now = performance.now();
    const step = Math.min(delta, 0.05);
    const { holder } = built;
    if (light.current) light.current.intensity = STRIPE * story.blinds * (1 - story.inside);
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
      <spotLight ref={light} target={built.aim} map={built.map} position={STREET.position} color="#ffe0bd" angle={0.1} penumbra={0.6} decay={2} distance={0} intensity={0} />
    </>
  );
}
