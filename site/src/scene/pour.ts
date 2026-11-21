import { Box3, BufferAttribute, BufferGeometry, Group, Mesh, Quaternion, SphereGeometry, Vector3, type Camera, type MeshStandardMaterial, type Object3D } from 'three';
import { DESK_TOP } from './layout';
import { play } from './sound';
import { glass, radiusAt, tumblerMaterial } from './Tumbler';
import { live, obstacles, things } from './touch';

const G = 9.81;
const DESK = { left: -0.98, right: 0.98, back: -0.36, front: 0.42 };
const LIP = { r: 0.0455, y: 0.0985 };
const OUTER = 0.0465;
const LIFT = 0.09;
const POT = { soil: 0.14, rim: 0.157, radius: 0.084 };
const RINGS = 72;
const SIDES = 8;
const STEP = 1 / 120;
const DRY = 6000;
const TILT = 0.025;

type Shape = { points: Float32Array; weights: Float32Array; total: number; rim: { r: number; y: number }; spill: Float32Array; scratch: Float32Array };
type Plant = { x: number; z: number; soil: number; rim: number; radius: number; root: Object3D };
type Drop = { t: number; x: number; y: number; z: number; vx: number; vy: number; vz: number; q: number };
type Block = { cx: number; cz: number; cos: number; sin: number; hx: number; hz: number; top: number };
type Press = {
  kind: 'pour' | 'fill' | 'still';
  start: number;
  total: number;
  from: Vector3;
  path: [number, number][];
  lengths: number[];
  go: number;
  tip: number;
  flow: number;
  untip: number;
  lip: Vector3;
  dir: Vector3;
  rise: number;
  v0: number;
  theta: number;
  volume: number;
  loss: number;
  source: Vector3;
  emitted: number;
  emitEnd: number;
  landY: number;
  landed: boolean;
  set: boolean;
  drips: number[];
};

const state = {
  shape: null as Shape | null,
  press: null as Press | null,
  drops: [] as Drop[],
  head: null as Drop | null,
  beads: [] as (Drop & { mesh: Mesh; alive: boolean; land: number })[],
  landed: 0,
  wet: { x: 0, y: 0, z: 0, peak: 0, at: 0 },
  wetUniform: { value: [0, 0, 0, 0] as number[] },
  ripple: 0,
  fill: false,
  dripped: false,
  sent: 1,
  patched: null as Object3D | null,
  stream: null as Mesh | null,
};

const scratch = {
  a: new Vector3(),
  b: new Vector3(),
  c: new Vector3(),
  q: new Quaternion(),
  up: new Vector3(0, 1, 0),
  axis: new Vector3(),
  side: new Vector3(1, 0, 0),
};

const reduced = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const ease = (u: number) => (u <= 0 ? 0 : u >= 1 ? 1 : u * u * (3 - 2 * u));
const smooth = (a: number, b: number, u: number) => ease((u - a) / (b - a));

function shapeOf(profile: [number, number][], floor: number): Shape {
  const top = profile[profile.length - 1][1];
  const slices = 40;
  const rings = 5;
  const spokes = 12;
  const count = slices * rings * spokes;
  const points = new Float32Array(count * 3);
  const weights = new Float32Array(count);
  let i = 0;
  let total = 0;
  for (let k = 0; k < slices; k++) {
    const y0 = floor + ((top - floor) * k) / slices;
    const y1 = floor + ((top - floor) * (k + 1)) / slices;
    const y = (y0 + y1) / 2;
    const radius = radiusAt(profile, y) * 0.97;
    const area = (Math.PI * radius * radius * (y1 - y0)) / (rings * spokes);
    for (let j = 0; j < rings; j++) {
      const r = radius * Math.sqrt((j + 0.5) / rings);
      for (let a = 0; a < spokes; a++) {
        const phi = ((a + (j % 2) * 0.5) / spokes) * Math.PI * 2;
        points[i * 3] = r * Math.cos(phi);
        points[i * 3 + 1] = y;
        points[i * 3 + 2] = r * Math.sin(phi);
        weights[i] = area;
        total += area;
        i += 1;
      }
    }
  }
  const shape: Shape = { points, weights, total, rim: { r: radiusAt(profile, top) * 0.97, y: top }, spill: new Float32Array(73), scratch: new Float32Array(count) };
  for (let t = 0; t < shape.spill.length; t++) {
    const theta = t * TILT;
    const nx = Math.sin(theta);
    const ny = Math.cos(theta);
    shape.spill[t] = below(shape, nx, ny, 0, ny * shape.rim.y - shape.rim.r * nx);
  }
  return shape;
}

function below(shape: Shape, nx: number, ny: number, nz: number, c: number) {
  const { points, weights } = shape;
  let volume = 0;
  for (let i = 0; i < weights.length; i++) if (nx * points[i * 3] + ny * points[i * 3 + 1] + nz * points[i * 3 + 2] < c) volume += weights[i];
  return volume;
}

function solve(shape: Shape, normal: Vector3, volume: number) {
  const { points, weights, scratch: along } = shape;
  let lo = Infinity;
  let hi = -Infinity;
  for (let i = 0; i < weights.length; i++) {
    const s = normal.x * points[i * 3] + normal.y * points[i * 3 + 1] + normal.z * points[i * 3 + 2];
    along[i] = s;
    if (s < lo) lo = s;
    if (s > hi) hi = s;
  }
  if (volume <= 0) return lo - 0.001;
  if (volume >= shape.total) return hi;
  for (let k = 0; k < 22; k++) {
    const mid = (lo + hi) / 2;
    let v = 0;
    for (let i = 0; i < weights.length; i++) if (along[i] < mid) v += weights[i];
    if (v < volume) lo = mid;
    else hi = mid;
  }
  return (lo + hi) / 2;
}

function thetaFor(shape: Shape, volume: number) {
  const { spill } = shape;
  for (let t = 1; t < spill.length; t++) {
    if (spill[t] <= volume) {
      const a = spill[t - 1];
      const b = spill[t];
      return (t - 1 + (a - volume) / Math.max(a - b, 1e-12)) * TILT;
    }
  }
  return (spill.length - 1) * TILT;
}

function shown(object: Object3D) {
  let at: Object3D | null = object;
  while (at) {
    if (!at.visible) return false;
    at = at.parent;
  }
  return true;
}

function plantNow(): Plant | null {
  const root = things.plant;
  if (!root || !shown(root)) return null;
  root.updateWorldMatrix(true, true);
  const base = root.getWorldPosition(new Vector3());
  const scale = root.getWorldScale(new Vector3()).x;
  const v = new Vector3();
  let sx = 0;
  let sz = 0;
  let n = 0;
  root.traverse((child) => {
    const mesh = child as Mesh;
    if (!mesh.isMesh) return;
    const position = mesh.geometry.attributes.position;
    for (let i = 0; i < position.count; i += 3) {
      v.fromBufferAttribute(position, i).applyMatrix4(mesh.matrixWorld);
      if (v.y > base.y + 0.012 * scale) continue;
      sx += v.x;
      sz += v.z;
      n += 1;
    }
  });
  return { x: n ? sx / n : base.x, z: n ? sz / n : base.z, soil: base.y + POT.soil * scale, rim: base.y + POT.rim * scale, radius: POT.radius * scale, root };
}

function blocksExcept(skip: Object3D[]) {
  const out: Block[] = [];
  const v = new Vector3();
  const q = new Quaternion();
  const center = new Vector3();
  for (const object of obstacles) {
    if (skip.includes(object) || !shown(object)) continue;
    object.updateWorldMatrix(true, true);
    object.getWorldPosition(center);
    object.getWorldQuaternion(q);
    const yaw = Math.atan2(2 * (q.w * q.y + q.x * q.z), 1 - 2 * (q.y * q.y + q.x * q.x));
    const cos = Math.cos(yaw);
    const sin = Math.sin(yaw);
    let minX = Infinity;
    let maxX = -Infinity;
    let minZ = Infinity;
    let maxZ = -Infinity;
    let top = -Infinity;
    object.traverse((child) => {
      const mesh = child as Mesh;
      if (!mesh.isMesh || !shown(mesh)) return;
      if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
      const box = mesh.geometry.boundingBox as Box3;
      for (let k = 0; k < 8; k++) {
        v.set(k & 1 ? box.max.x : box.min.x, k & 2 ? box.max.y : box.min.y, k & 4 ? box.max.z : box.min.z).applyMatrix4(mesh.matrixWorld);
        const dx = v.x - center.x;
        const dz = v.z - center.z;
        const lx = dx * cos - dz * sin;
        const lz = dx * sin + dz * cos;
        minX = Math.min(minX, lx);
        maxX = Math.max(maxX, lx);
        minZ = Math.min(minZ, lz);
        maxZ = Math.max(maxZ, lz);
        top = Math.max(top, v.y);
      }
    });
    if (!Number.isFinite(top) || top < DESK_TOP + 0.05) continue;
    const mx = (minX + maxX) / 2;
    const mz = (minZ + maxZ) / 2;
    out.push({ cx: center.x + mx * cos + mz * sin, cz: center.z - mx * sin + mz * cos, cos, sin, hx: (maxX - minX) / 2, hz: (maxZ - minZ) / 2, top });
  }
  return out;
}

function toLocal(block: Block, x: number, z: number): [number, number] {
  const dx = x - block.cx;
  const dz = z - block.cz;
  return [dx * block.cos - dz * block.sin, dx * block.sin + dz * block.cos];
}

function toWorld(block: Block, lx: number, lz: number): [number, number] {
  return [block.cx + lx * block.cos + lz * block.sin, block.cz - lx * block.sin + lz * block.cos];
}

function inside(block: Block, x: number, z: number, pad: number) {
  const [lx, lz] = toLocal(block, x, z);
  return Math.abs(lx) < block.hx + pad && Math.abs(lz) < block.hz + pad;
}

function crosses(block: Block, pad: number, ax: number, az: number, bx: number, bz: number) {
  const [px, pz] = toLocal(block, ax, az);
  const [qx, qz] = toLocal(block, bx, bz);
  let t0 = 0;
  let t1 = 1;
  const dx = qx - px;
  const dz = qz - pz;
  const edges: [number, number][] = [
    [-dx, px + block.hx + pad],
    [dx, block.hx + pad - px],
    [-dz, pz + block.hz + pad],
    [dz, block.hz + pad - pz],
  ];
  for (const [p, q] of edges) {
    if (Math.abs(p) < 1e-12) {
      if (q <= 0) return false;
      continue;
    }
    const r = q / p;
    if (p < 0) t0 = Math.max(t0, r);
    else t1 = Math.min(t1, r);
    if (t0 >= t1) return false;
  }
  return t1 - t0 > 1e-6;
}

function escape(from: [number, number], blocks: Block[], pad: number, onDesk: (x: number, z: number) => boolean) {
  let [x, z] = from;
  for (let round = 0; round < 4; round++) {
    const block = blocks.find((item) => inside(item, x, z, pad));
    if (!block) break;
    const [lx, lz] = toLocal(block, x, z);
    const ex = block.hx + pad + 0.004;
    const ez = block.hz + pad + 0.004;
    const exits = [
      [ex - Math.abs(lx), Math.sign(lx || 1) * ex, lz],
      [ez - Math.abs(lz), lx, Math.sign(lz || 1) * ez],
      [ex + Math.abs(lx), -Math.sign(lx || 1) * ex, lz],
      [ez + Math.abs(lz), lx, -Math.sign(lz || 1) * ez],
    ]
      .map(([d, ax, az]) => [d, ...toWorld(block, ax, az)] as [number, number, number])
      .filter(([, wx, wz]) => onDesk(wx, wz))
      .sort((a, b) => a[0] - b[0]);
    if (!exits.length) break;
    x = exits[0][1];
    z = exits[0][2];
  }
  return [x, z] as [number, number];
}

function route(start: [number, number], to: [number, number], blocks: Block[]) {
  const pad = OUTER + 0.012;
  const onDesk = (x: number, z: number) => x >= DESK.left && x <= DESK.right && z >= DESK.back && z <= DESK.front;
  const from = escape(start, blocks, pad, onDesk);
  const lead: [number, number][] = from[0] === start[0] && from[1] === start[1] ? [] : [start];
  const nodes: [number, number][] = [from, to];
  for (const block of blocks) {
    for (const [sx, sz] of [
      [-1, -1],
      [1, -1],
      [-1, 1],
      [1, 1],
    ]) {
      const [x, z] = toWorld(block, sx * (block.hx + pad + 0.025), sz * (block.hz + pad + 0.025));
      if (onDesk(x, z) && !blocks.some((other) => inside(other, x, z, pad))) nodes.push([x, z]);
    }
  }
  const clear = (a: number, b: number) =>
    !blocks.some((block) => {
      const [ax, az] = nodes[a];
      const [bx, bz] = nodes[b];
      if ((a < 2 && inside(block, ax, az, pad)) || (b < 2 && inside(block, bx, bz, pad))) return false;
      return crosses(block, pad, ax, az, bx, bz);
    });
  const cost = nodes.map(() => Infinity);
  const back = nodes.map(() => -1);
  const done = nodes.map(() => false);
  cost[0] = 0;
  for (;;) {
    let at = -1;
    for (let i = 0; i < nodes.length; i++) if (!done[i] && cost[i] < Infinity && (at < 0 || cost[i] < cost[at])) at = i;
    if (at < 0 || at === 1) break;
    done[at] = true;
    for (let next = 0; next < nodes.length; next++) {
      if (done[next] || next === at) continue;
      const step = Math.hypot(nodes[next][0] - nodes[at][0], nodes[next][1] - nodes[at][1]);
      if (cost[at] + step >= cost[next] || !clear(at, next)) continue;
      cost[next] = cost[at] + step;
      back[next] = at;
    }
  }
  if (back[1] < 0) return [...lead, from, to];
  const path: [number, number][] = [];
  for (let at = 1; at >= 0; at = back[at]) path.unshift(nodes[at]);
  return [...lead, ...path];
}

function rounded(path: [number, number][], blocks: Block[]) {
  const smooth = soften(path);
  const pad = OUTER + 0.004;
  for (let i = 1; i < smooth.length; i++) {
    const [ax, az] = smooth[i - 1];
    const [bx, bz] = smooth[i];
    if (blocks.some((block) => !inside(block, ax, az, pad) && !inside(block, bx, bz, pad) && crosses(block, pad, ax, az, bx, bz))) return path;
  }
  return smooth;
}

function soften(path: [number, number][]) {
  let points = path;
  for (let round = 0; round < 2; round++) {
    if (points.length < 3) break;
    const next: [number, number][] = [points[0]];
    for (let i = 0; i < points.length - 1; i++) {
      const [ax, az] = points[i];
      const [bx, bz] = points[i + 1];
      if (i > 0) next.push([ax * 0.75 + bx * 0.25, az * 0.75 + bz * 0.25]);
      if (i < points.length - 2) next.push([ax * 0.25 + bx * 0.75, az * 0.25 + bz * 0.75]);
    }
    next.push(points[points.length - 1]);
    points = next;
  }
  return points;
}

function measure(path: [number, number][]) {
  const lengths = [0];
  for (let i = 1; i < path.length; i++) lengths.push(lengths[i - 1] + Math.hypot(path[i][0] - path[i - 1][0], path[i][1] - path[i - 1][1]));
  return lengths;
}

function along(press: Press, u: number, out: Vector3) {
  const { path, lengths } = press;
  const total = lengths[lengths.length - 1];
  const s = u * total;
  let i = 1;
  while (i < lengths.length - 1 && lengths[i] < s) i += 1;
  const span = Math.max(lengths[i] - lengths[i - 1], 1e-9);
  const k = Math.min(Math.max((s - lengths[i - 1]) / span, 0), 1);
  out.set(path[i - 1][0] + (path[i][0] - path[i - 1][0]) * k, 0, path[i - 1][1] + (path[i][1] - path[i - 1][1]) * k);
  return out;
}

function choose(plant: Plant, from: Vector3, camera: Camera, blocks: Block[]) {
  const forward = camera.getWorldDirection(new Vector3());
  forward.y = 0;
  forward.normalize();
  const lipOut = plant.radius * 1.12;
  const reach = lipOut + LIP.r;
  let best: { dir: Vector3; score: number } | null = null;
  for (let k = 0; k < 24; k++) {
    const angle = (k / 24) * Math.PI * 2;
    const dir = new Vector3(Math.cos(angle), 0, Math.sin(angle));
    const gx = plant.x + dir.x * reach;
    const gz = plant.z + dir.z * reach;
    const ax = plant.x + dir.x * (reach + 0.07);
    const az = plant.z + dir.z * (reach + 0.07);
    const pad = OUTER + 0.008;
    if (gx < DESK.left || gx > DESK.right || gz < DESK.back || gz > DESK.front) continue;
    if (blocks.some((block) => inside(block, gx, gz, pad) || inside(block, ax, az, pad))) continue;
    const side = Math.abs(forward.x * dir.z - forward.z * dir.x);
    const front = -(forward.x * dir.x + forward.z * dir.z);
    const score = side * 1.4 + front * 0.5 - Math.hypot(gx - from.x, gz - from.z) * 0.6;
    if (!best || score > best.score) best = { dir, score };
  }
  return best?.dir ?? new Vector3(from.x - plant.x, 0, from.z - plant.z).normalize();
}

