import { useGLTF } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { DoubleSide, Group, LatheGeometry, Mesh, MeshDepthMaterial, MeshStandardMaterial, Object3D, Quaternion, ShaderChunk, Vector2, Vector3, Vector4, type Camera } from 'three';
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

if (!runtime.abandoned) useGLTF.preload(url, false, true);

const wrapped = ShaderChunk.lights_physical_pars_fragment.replace(
  'reflectedLight.directDiffuse += irradiance * diffuseBRDF * ( 1.0 - F );',
  'reflectedLight.directDiffuse += mix( irradiance, saturate( ( dot( geometryNormal, directLight.direction ) + 0.75 ) / 1.75 ) * directLight.color, uJuice ) * diffuseBRDF * ( 1.0 - F );',
);

const edge =
  'if ( uJuice < 0.5 ) {\nfloat edge = pow(1.0 - abs(dot(normal, normalize(vViewPosition))), 2.2);\ndiffuseColor.a = mix(0.05, 0.5, edge);\noutgoingLight = min(outgoingLight + vec3(0.62, 0.66, 0.68) * edge * 0.09, vec3(0.5));\n}\n#include <opaque_fragment>';

const lit = ShaderChunk.lights_fragment_begin
  .replaceAll('directLight.visible && receiveShadow', 'directLight.visible && receiveShadow && !juiceCap')
  .replace('vec3 geometryPosition = - vViewPosition;', 'vec3 geometryPosition = juiceView;')
  .replace('normalize( vViewPosition );', 'normalize( - juiceView );');
const surface = { uSurface: { value: new Vector4(0, 1, 0, 1e3) }, uRipple: { value: new Vector4() }, uAxis: { value: new Vector4(0, 0, 0.04, 0) } };
const worldVarying = ['#include <project_vertex>', '#include <project_vertex>\nvJuiceWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;'] as const;
const cut = 'if ( uCut > 0.5 && dot( vJuiceWorld, uSurface.xyz ) > uSurface.w ) discard;';
const top =
  '#include <normal_fragment_maps>\nvec3 juiceView = - vViewPosition;\nbool juiceCap = uCut > 0.5 && !gl_FrontFacing;\n#ifdef FLIP_SIDED\njuiceCap = uCut > 0.5;\n#endif\nif ( juiceCap ) {\nvec3 ray = vJuiceWorld - cameraPosition;\nvec3 cap = cameraPosition + ray * clamp( ( uSurface.w - dot( uSurface.xyz, cameraPosition ) ) / dot( uSurface.xyz, ray ), 0.0, 1.0 );\nvec2 out2 = cap.xz - uAxis.xy;\nfloat r = length( out2 ) + 1e-5;\nfloat f = clamp( r / uAxis.z, 0.0, 1.0 );\nvec2 q = cap.xz - uRipple.xy;\nfloat d = length( q ) + 1e-4;\nfloat w = uRipple.w * sin( d * 300.0 - uRipple.z ) * exp( -d * 45.0 );\nvec3 n = uSurface.xyz * ( 1.0 - 0.389 * f ) + vec3( out2.x, 0.0, out2.y ) / r * 0.791 * f + vec3( q.x, 0.0, q.y ) / d * w;\nnormal = normalize( ( viewMatrix * vec4( n, 0.0 ) ).xyz );\njuiceView = ( viewMatrix * vec4( cap, 1.0 ) ).xyz;\n}';

export function tumblerMaterial(kind: 0 | 1 | 2) {
  const juice = kind > 0;
  const material = juice
    ? new MeshStandardMaterial({ color: tokenColor('lamp'), transparent: true, opacity: 1, roughness: 0.24, metalness: 0, side: DoubleSide })
    : new MeshStandardMaterial({ color: '#e7eef0', transparent: true, opacity: 1, roughness: 0.2, metalness: 0, depthWrite: false });
  const uniforms = { uJuice: { value: juice ? 1 : 0 }, uCut: { value: kind === 1 ? 1 : 0 }, ...surface };
  material.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, uniforms);
    shader.vertexShader = `varying vec3 vJuiceWorld;\n${shader.vertexShader.replace(...worldVarying)}`;
    shader.fragmentShader = `uniform float uJuice;\nuniform float uCut;\nuniform vec4 uSurface;\nuniform vec4 uRipple;\nuniform vec4 uAxis;\nvarying vec3 vJuiceWorld;\n${shader.fragmentShader
      .replace('#include <lights_physical_pars_fragment>', wrapped)
      .replace('#include <opaque_fragment>', edge)
      .replace('#include <clipping_planes_fragment>', `#include <clipping_planes_fragment>\n${cut}`)
      .replace('#include <normal_fragment_maps>', top)
      .replace('#include <lights_fragment_begin>', lit)}`;
  };
  material.customProgramCacheKey = () => 'tumbler';
  material.defines = { WINDOW_WRAP: '' };
  windowLit(material);
  const lightWindow = material.onBeforeCompile;
  material.onBeforeCompile = (shader, renderer) => {
    lightWindow.call(material, shader, renderer);
    shader.fragmentShader = shader.fragmentShader.replace('uWindowPosition + vViewPosition', 'uWindowPosition - juiceView').replace('vec4(-vViewPosition, 1.0)', 'vec4(juiceView, 1.0)');
  };
  rimmed(material);
  return material;
}

function juiceDepth() {
  const material = new MeshDepthMaterial();
  material.onBeforeCompile = (shader) => {
    shader.uniforms.uSurface = surface.uSurface;
    shader.uniforms.uCut = { value: 1 };
    shader.vertexShader = `varying vec3 vJuiceWorld;\n${shader.vertexShader.replace(...worldVarying)}`;
    shader.fragmentShader = `uniform vec4 uSurface;\nuniform float uCut;\nvarying vec3 vJuiceWorld;\n${shader.fragmentShader.replace('#include <clipping_planes_fragment>', `#include <clipping_planes_fragment>\n${cut}`)}`;
  };
  material.customProgramCacheKey = () => 'juice-depth';
  return material;
}

export const glass = {
  level: 1,
  ripple: surface.uRipple.value,
  plane: surface.uSurface.value,
  solve: null as null | ((normal: Vector3, volume: number) => number),
  drive: null as null | ((now: number, step: number) => boolean),
  parts: null as null | { holder: Group; juice: Mesh; profile: [number, number][]; floor: number; table: [number, number][]; full: number },
};

export function radiusAt(profile: [number, number][], y: number) {
  if (y <= profile[0][1]) return profile[0][0];
  for (let n = 1; n < profile.length; n++) {
    const [r1, y1] = profile[n];
    const [r0, y0] = profile[n - 1];
    if (y <= y1) return r0 + ((r1 - r0) * (y - y0)) / (y1 - y0);
  }
  return profile[profile.length - 1][0];
}

function inside(profile: [number, number][], floor: number) {
  const points = [new Vector2(0, floor), new Vector2(radiusAt(profile, floor + 0.0004) * 0.97, floor)];
  for (const [r, y] of profile) if (y > floor + 0.0004) points.push(new Vector2(r * 0.97, y));
  return new LatheGeometry(points, 48);
}

export function volumes(profile: [number, number][], floor: number) {
  const table: [number, number][] = [[floor, 0]];
  let total = 0;
  let last = floor;
  for (const [, y] of profile) {
    if (y <= floor) continue;
    const a = radiusAt(profile, last) * 0.97;
    const b = radiusAt(profile, y) * 0.97;
    total += (Math.PI * (y - last) * (a * a + a * b + b * b)) / 3;
    table.push([y, total]);
    last = y;
  }
  return table;
}

function lookup(table: [number, number][], value: number, from: 0 | 1) {
  const to = 1 - from;
  if (value <= table[0][from]) return table[0][to];
  for (let n = 1; n < table.length; n++) {
    const a = table[n - 1];
    const b = table[n];
    if (value <= b[from]) return a[to] + ((b[to] - a[to]) * (value - a[from])) / Math.max(b[from] - a[from], 1e-9);
  }
  return table[table.length - 1][to];
}

export const heightOf = (table: [number, number][], volume: number) => lookup(table, volume, 1);
export const volumeAt = (table: [number, number][], height: number) => lookup(table, height, 0);

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

const up = new Vector3(0, 1, 0);
const normal = new Vector3();
const local = new Vector3();
const point = new Vector3();
const turn = new Quaternion();
const motion = { x: 0, z: 0, vx: 0, vz: 0, sx: 0, sz: 0, wx: 0, wz: 0, ready: false };

export function Tumbler() {
  const gltf = useGLTF(url, false, true);
  const camera = useThree((state) => state.camera);
  const size = useThree((state) => state.size);
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
    const cup = new Mesh(source.geometry, tumblerMaterial(0));
    source.updateWorldMatrix(true, false);
    cup.applyMatrix4(source.matrixWorld);
    cup.renderOrder = 2;
    const juice = new Mesh(inside(extras.inner, extras.floor), tumblerMaterial(1));
    juice.name = 'juice';
    juice.castShadow = true;
    juice.receiveShadow = true;
    juice.customDepthMaterial = juiceDepth();
    const aim = new Object3D();
    aim.position.set(HOME.x, DESK_TOP + 0.045, HOME.z);
    holder.add(juice, cup);
    makeLive(holder, 'glass');
    placeStripe(aim.position);
    const table = volumes(extras.inner, extras.floor);
    const parts = { holder, juice, profile: extras.inner, floor: extras.floor, table, full: volumeAt(table, TOP) };
    glass.parts = parts;
    glass.level = emptyStill ? 0 : 1;
    return parts;
  }, [gltf]);

  useEffect(() => {
    if (runtime.still < 0 && !query.has('poster')) return;
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
      if (glass.parts === built) glass.parts = null;
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
    if (!glass.drive?.(now, step)) {
      const lastX = holder.position.x;
      const lastZ = holder.position.z;
      holder.position.x += (live.glass.x - holder.position.x) * (1 - Math.exp(-step * 9));
      holder.position.z += (live.glass.z - holder.position.z) * (1 - Math.exp(-step * 9));
      const lean = holder.userData.lean ?? (holder.userData.lean = { x: 0, z: 0 });
      const speedX = step > 0 ? (holder.position.x - lastX) / step : 0;
      const speedZ = step > 0 ? (holder.position.z - lastZ) / step : 0;
      lean.x += (Math.max(-0.14, Math.min(0.14, speedZ * 0.3)) - lean.x) * (1 - Math.exp(-step * 14));
      lean.z += (Math.max(-0.14, Math.min(0.14, -speedX * 0.3)) - lean.z) * (1 - Math.exp(-step * 14));
      const moving = Math.hypot(live.glass.x - holder.position.x, live.glass.z - holder.position.z) > 0.002;
      holder.position.y = DESK_TOP + (moving ? 0.005 : 0);
      holder.rotation.set(lean.x, 0, lean.z);
      const target = emptyStill ? 0 : runtime.still >= 0 ? 1 : live.juice / 4;
      glass.level += (target - glass.level) * (1 - Math.exp(-step * (target > glass.level ? 2.4 : 5)));
      if (Math.abs(target - glass.level) < 0.002) glass.level = target;
    }
    holder.updateWorldMatrix(true, false);
    point.setFromMatrixPosition(holder.matrixWorld);
    if (!motion.ready || step <= 0) {
      motion.x = point.x;
      motion.z = point.z;
      motion.ready = true;
    } else {
      const vx = (point.x - motion.x) / step;
      const vz = (point.z - motion.z) / step;
      const ax = Math.max(-40, Math.min(40, (vx - motion.vx) / step));
      const az = Math.max(-40, Math.min(40, (vz - motion.vz) / step));
      motion.x = point.x;
      motion.z = point.z;
      motion.vx = vx;
      motion.vz = vz;
      motion.wx += (320 * (ax * 0.045 - motion.sx) - 7 * motion.wx) * step;
      motion.wz += (320 * (az * 0.045 - motion.sz) - 7 * motion.wz) * step;
      motion.sx = Math.max(-0.35, Math.min(0.35, motion.sx + motion.wx * step));
      motion.sz = Math.max(-0.35, Math.min(0.35, motion.sz + motion.wz * step));
      if (Math.abs(motion.sx) + Math.abs(motion.sz) + (Math.abs(motion.wx) + Math.abs(motion.wz)) * 0.05 < 1e-4) motion.sx = motion.sz = motion.wx = motion.wz = 0;
    }
    live.slosh = Math.abs(motion.sx) + Math.abs(motion.sz) + Math.abs(surface.uRipple.value.w);
    const juice = built.juice;
    juice.visible = glass.level > 0.004;
    normal.set(motion.sx, 1, motion.sz).normalize();
    turn.setFromRotationMatrix(holder.matrixWorld).invert();
    local.copy(normal).applyQuaternion(turn);
    const volume = glass.level * built.full;
    const height = heightOf(built.table, volume);
    const constant = glass.solve && local.y < 0.9999 ? glass.solve(local, volume) : local.y * height;
    point.copy(local).multiplyScalar(constant);
    holder.localToWorld(point);
    surface.uSurface.value.set(normal.x, normal.y, normal.z, normal.dot(point));
    point.set(0, Math.max(height, built.floor), 0);
    holder.localToWorld(point);
    surface.uAxis.value.set(point.x, point.z, radiusAt(built.profile, height) * 0.97, 0);
  });

  return (
    <>
      <primitive object={built.holder} />
    </>
  );
}
