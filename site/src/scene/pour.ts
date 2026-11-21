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

function lipOf(dir: Vector3, out: Vector3) {
  return out.set(dir.x * LIP.r, LIP.y, dir.z * LIP.r);
}

function lipNow(press: Press, holder: Object3D, out: Vector3) {
  return lipOf(press.dir, out).applyQuaternion(holder.quaternion).add(holder.position);
}

function tilt(press: Press, theta: number) {
  scratch.axis.crossVectors(scratch.up, press.dir).normalize();
  return scratch.q.setFromAxisAngle(scratch.axis, theta);
}

function poseAt(press: Press, theta: number, holder: Object3D) {
  const q = tilt(press, theta);
  const lip = lipOf(press.dir, scratch.a).applyQuaternion(q);
  holder.quaternion.copy(q);
  holder.position.copy(press.lip).sub(lip);
}

function travel(press: Press, u: number, back: boolean, holder: Object3D, step: number) {
  along(press, back ? 1 - ease(u) : ease(u), scratch.b);
  const endY = press.lip.y - LIP.y;
  const v = back ? 1 - u : u;
  const low = back ? DESK_TOP : press.from.y;
  const top = Math.max(DESK_TOP + LIFT, endY);
  const y = low + (top - low) * smooth(0, 0.3, v) + (endY - top) * smooth(0.7, 1, v);
  const lastX = holder.position.x;
  const lastZ = holder.position.z;
  holder.position.set(scratch.b.x, Math.max(y, DESK_TOP), scratch.b.z);
  const vx = step > 0 ? (holder.position.x - lastX) / step : 0;
  const vz = step > 0 ? (holder.position.z - lastZ) / step : 0;
  holder.rotation.set(Math.max(-0.1, Math.min(0.1, vz * 0.05)), 0, Math.max(-0.1, Math.min(0.1, -vx * 0.05)));
}

function sourceAbove(camera: Camera, x: number, z: number, bottom: number) {
  const point = new Vector3();
  let lo = bottom + 0.05;
  let hi = bottom + 3;
  for (let k = 0; k < 24; k++) {
    const mid = (lo + hi) / 2;
    point.set(x, mid, z).project(camera);
    if (point.y > 1.06 && point.z < 1) hi = mid;
    else lo = mid;
  }
  return hi;
}

const view = { camera: null as Camera | null };

export function pressGlass(now: number) {
  const camera = view.camera;
  const parts = glass.parts;
  if (!parts || !camera || now < live.pourUntil) return false;
  if (!state.shape) state.shape = shapeOf(parts.profile, parts.floor);
  const holder = parts.holder;
  const full = parts.full;
  const empty = live.juice <= 0;
  const plant = empty ? null : plantNow();
  const base: Press = {
    kind: 'still',
    start: now,
    total: 0.5,
    from: holder.position.clone(),
    path: [],
    lengths: [0],
    go: 0,
    tip: 0,
    flow: 0,
    untip: 0,
    lip: new Vector3(),
    dir: new Vector3(1, 0, 0),
    rise: 0,
    v0: 0,
    theta: 0,
    volume: glass.level * full,
    loss: empty ? -(full - glass.level * full) : Math.min(full / 4, glass.level * full),
    source: new Vector3(),
    emitted: 0,
    emitEnd: 0,
    landY: 0,
    landed: false,
    set: false,
    drips: [],
  };
  live.juice = empty ? 4 : Math.max(0, live.juice - 1);
  state.drops = [];
  state.head = null;
  state.landed = 0;
  state.fill = false;
  state.dripped = false;
  state.wet.peak = wetAmount(now);
  state.wet.at = now;
  if (reduced() || (!empty && !plant)) {
    state.press = base;
    live.pourUntil = now + 600;
    live.fadeUntil = Math.max(live.fadeUntil, now + 700);
    if (plant) {
      state.wet = { x: plant.x, y: plant.soil, z: plant.z, peak: 1, at: now };
      live.fadeUntil = Math.max(live.fadeUntil, now + DRY);
    }
    play(empty ? 'pour' : 'stream');
    return true;
  }
  if (empty) {
    const top = holder.position.y + 0.1;
    const y = sourceAbove(camera, holder.position.x, holder.position.z, top);
    const fall = Math.sqrt((2 * (y - (holder.position.y + parts.floor))) / G);
    const flow = 1.1;
    const press: Press = { ...base, kind: 'fill', total: fall * 2 + flow + 0.4, flow, source: new Vector3(holder.position.x, y, holder.position.z), emitEnd: flow };
    let sent = 0;
    for (let k = 0; k * STEP <= flow; k++) sent += fillShape((k * STEP) / flow) * STEP;
    state.sent = sent / flow;
    state.fill = true;
    state.press = press;
    live.pourUntil = now + press.total * 1000;
    live.fadeUntil = Math.max(live.fadeUntil, live.pourUntil + 300);
    return true;
  }
  const p = plant as Plant;
  const blocks = blocksExcept([p.root]);
  const dir = choose(p, holder.position, camera, blocks);
  const lip = new Vector3(p.x + dir.x * p.radius * 1.12, Math.max(p.rim + 0.105, DESK_TOP + LIFT + LIP.y), p.z + dir.z * p.radius * 1.12);
  const pre: [number, number] = [p.x + dir.x * (p.radius * 1.12 + LIP.r + 0.07), p.z + dir.z * (p.radius * 1.12 + LIP.r + 0.07)];
  const stop: [number, number] = [lip.x + dir.x * LIP.r, lip.z + dir.z * LIP.r];
  const plantBlock = blocksExcept(obstacles.filter((object) => object !== p.root));
  const path = rounded([...route([holder.position.x, holder.position.z], pre, [...blocks, ...plantBlock]), stop], blocks);
  const lengths = measure(path);
  const go = Math.min(Math.max(0.32 + lengths[lengths.length - 1] * 0.3, 0.4), 0.62);
  const shape = state.shape;
  const theta = thetaFor(shape, base.volume);
  const land = new Vector3(p.x + dir.x * p.radius * 0.5, p.soil, p.z + dir.z * p.radius * 0.5);
  const fall = Math.sqrt((2 * Math.max(lip.y - land.y, 0.02)) / G);
  const pour = new Vector3(-dir.x, 0, -dir.z);
  const press: Press = {
    ...base,
    kind: 'pour',
    path,
    lengths,
    go,
    tip: 0.28,
    flow: 0.5,
    untip: 0.25,
    lip,
    dir: pour,
    v0: Math.hypot(land.x - lip.x, land.z - lip.z) / fall,
    theta,
    landY: p.soil,
    total: go * 2 + 0.28 + 0.5 + 0.25,
  };
  press.emitEnd = go + press.tip + press.flow;
  press.drips = [0.03, 0.09, 0.16, 0.24].map((t) => press.emitEnd + t);
  state.press = press;
  live.pourUntil = now + press.total * 1000;
  live.fadeUntil = Math.max(live.fadeUntil, live.pourUntil + 200);
  state.wet.x = land.x;
  state.wet.y = p.soil;
  state.wet.z = land.z;
  play('lift');
  return true;
}

function drive(now: number, step: number) {
  const press = state.press;
  const parts = glass.parts;
  if (!press || !parts) return false;
  const holder = parts.holder;
  const full = parts.full;
  const t = (now - press.start) / 1000;
  if (press.kind === 'still') {
    const u = Math.min(t / 0.5, 1);
    glass.level = (press.volume - press.loss * ease(u)) / full;
    holder.position.set(live.glass.x, DESK_TOP, live.glass.z);
    holder.rotation.set(0, 0, 0);
    if (u >= 1) state.press = null;
    return true;
  }
  if (press.kind === 'fill') {
    holder.position.set(live.glass.x, DESK_TOP, live.glass.z);
    holder.rotation.set(0, 0, 0);
    glass.level = Math.min(1, (press.volume + state.landed) / full);
    if (t >= press.total) {
      glass.level = 1;
      state.press = null;
      return false;
    }
    return true;
  }
  const { go, tip, flow, untip } = press;
  if (t < go) travel(press, t / go, false, holder, step);
  else if (t < go + tip) poseAt(press, press.theta * ease((t - go) / tip), holder);
  else if (t < go + tip + flow) {
    const u = (t - go - tip) / flow;
    const gone = press.loss * (1 - (1 - u) ** 1.6);
    glass.level = (press.volume - gone) / full;
    poseAt(press, thetaFor(state.shape as Shape, press.volume - gone) + 0.04 * Math.sin(Math.PI * u), holder);
  } else if (t < go + tip + flow + untip) {
    glass.level = (press.volume - press.loss) / full;
    const end = thetaFor(state.shape as Shape, press.volume - press.loss) + 0.0;
    poseAt(press, end * (1 - ease((t - go - tip - flow) / untip)), holder);
  } else if (t < press.total) {
    holder.quaternion.identity();
    travel(press, (t - go - tip - flow - untip) / go, true, holder, step);
  } else {
    holder.position.set(live.glass.x, DESK_TOP, live.glass.z);
    holder.rotation.set(0, 0, 0);
    glass.level = (press.volume - press.loss) / full;
    if (!press.set) {
      press.set = true;
      play('down');
    }
    state.press = null;
    return false;
  }
  return true;
}

function emitPour(press: Press, now: number, holder: Object3D) {
  const t = (now - press.start) / 1000;
  const begin = press.go + press.tip;
  const lip = lipNow(press, holder, scratch.c);
  while (press.emitted * STEP + begin <= Math.min(t, press.emitEnd)) {
    const te = press.emitted * STEP + begin;
    const u = (te - begin) / press.flow;
    const rate = (press.loss * 1.6 * (1 - u) ** 0.6) / press.flow;
    state.drops.push({ t: press.start / 1000 + te, x: lip.x, y: lip.y, z: lip.z, vx: press.dir.x * press.v0, vy: -0.05, vz: press.dir.z * press.v0, q: Math.max(rate, 0) });
    press.emitted += 1;
  }
}

function fillShape(u: number) {
  return Math.max(0, Math.min(1, u / 0.08, (1 - u) / 0.08 + 0.2));
}

function emitFill(press: Press, now: number) {
  const t = (now - press.start) / 1000;
  while (press.emitted * STEP <= Math.min(t, press.emitEnd)) {
    const te = press.emitted * STEP;
    const u = te / press.flow;
    const rate = (-press.loss / press.flow) * fillShape(u) / state.sent;
    state.drops.push({ t: press.start / 1000 + te, x: press.source.x, y: press.source.y, z: press.source.z, vx: 0, vy: -0.1, vz: 0, q: Math.max(rate, 0) });
    press.emitted += 1;
  }
}

function at(drop: Drop, time: number, out: Vector3) {
  const a = Math.max(time - drop.t, 0);
  return out.set(drop.x + drop.vx * a, drop.y + drop.vy * a - 0.5 * G * a * a, drop.z + drop.vz * a);
}

function speed(drop: Drop, time: number) {
  const a = Math.max(time - drop.t, 0);
  return Math.hypot(drop.vx, drop.vy - G * a, drop.vz);
}

function surfaceY(x: number, z: number) {
  const plane = glass.plane;
  return (plane.w - plane.x * x - plane.z * z) / Math.max(plane.y, 1e-3);
}

export function makeStream() {
  const geometry = new BufferGeometry();
  const positions = new Float32Array(RINGS * SIDES * 3);
  const normals = new Float32Array(RINGS * SIDES * 3);
  const index: number[] = [];
  for (let r = 0; r < RINGS - 1; r++) {
    for (let s = 0; s < SIDES; s++) {
      const a = r * SIDES + s;
      const b = r * SIDES + ((s + 1) % SIDES);
      const c = a + SIDES;
      const d = b + SIDES;
      index.push(a, c, b, b, c, d);
    }
  }
  geometry.setAttribute('position', new BufferAttribute(positions, 3));
  geometry.setAttribute('normal', new BufferAttribute(normals, 3));
  geometry.setIndex(index);
  geometry.setDrawRange(0, 0);
  const material = tumblerMaterial(2);
  const mesh = new Mesh(geometry, material);
  mesh.name = 'stream';
  mesh.frustumCulled = false;
  mesh.visible = false;
  const group = new Group();
  group.add(mesh);
  const bead = new SphereGeometry(1, 10, 8);
  state.beads = [0, 1, 2, 3].map(() => {
    const drop = new Mesh(bead, material);
    drop.visible = false;
    drop.frustumCulled = false;
    group.add(drop);
    return { t: 0, x: 0, y: 0, z: 0, vx: 0, vy: 0, vz: 0, q: 0, mesh: drop, alive: false, land: 0 };
  });
  state.stream = mesh;
  return group;
}

const ring = { p: new Vector3(), v: new Vector3(), n: new Vector3(), b1: new Vector3(), b2: new Vector3() };

function build(list: { p: Vector3; v: Vector3; r: number }[], side: Vector3) {
  const mesh = state.stream;
  if (!mesh) return;
  const count = Math.min(list.length, RINGS);
  mesh.visible = count >= 2;
  if (count < 2) return;
  const geometry = mesh.geometry;
  const positions = geometry.attributes.position.array as Float32Array;
  const normals = geometry.attributes.normal.array as Float32Array;
  for (let i = 0; i < count; i++) {
    const { p, v, r } = list[i];
    ring.v.copy(v).normalize();
    ring.b1.copy(side).addScaledVector(ring.v, -side.dot(ring.v));
    if (ring.b1.lengthSq() < 1e-8) ring.b1.set(0, 0, 1);
    ring.b1.normalize();
    ring.b2.crossVectors(ring.v, ring.b1);
    for (let s = 0; s < SIDES; s++) {
      const angle = (s / SIDES) * Math.PI * 2;
      ring.n.copy(ring.b1).multiplyScalar(Math.cos(angle)).addScaledVector(ring.b2, Math.sin(angle));
      const k = (i * SIDES + s) * 3;
      normals[k] = ring.n.x;
      normals[k + 1] = ring.n.y;
      normals[k + 2] = ring.n.z;
      positions[k] = p.x + ring.n.x * r;
      positions[k + 1] = p.y + ring.n.y * r;
      positions[k + 2] = p.z + ring.n.z * r;
    }
  }
  geometry.attributes.position.needsUpdate = true;
  geometry.attributes.normal.needsUpdate = true;
  geometry.setDrawRange(0, (count - 1) * SIDES * 6);
}

const rings = Array.from({ length: RINGS }, () => ({ p: new Vector3(), v: new Vector3(), r: 0 }));

function streamFrame(now: number) {
  const press = state.press;
  const time = now / 1000;
  const parts = glass.parts;
  if (press && parts && press.kind === 'pour') emitPour(press, now, parts.holder);
  if (press && press.kind === 'fill') emitFill(press, now);
  const fill = state.fill;
  const point = scratch.a;
  let landedNow = 0;
  while (state.drops.length) {
    const oldest = state.drops[0];
    at(oldest, time, point);
    const floor = fill ? surfaceY(point.x, point.z) : state.wet.y;
    if (point.y > floor) break;
    state.head = state.drops.shift() as Drop;
    state.landed += state.head.q * STEP;
    landedNow += 1;
    if (press && !press.landed) {
      press.landed = true;
      play(fill ? 'pour' : 'stream');
    }
    if (!fill) {
      state.wet.at = now;
      state.wet.peak = Math.min(1, state.wet.peak + (state.head.q * STEP * 0.9) / Math.max(press?.loss ?? 1e-5, 1e-6));
    }
  }
  if (fill && landedNow) state.ripple = Math.min(state.ripple + landedNow * 0.03, 0.22);
  const emitting = !!press && ((press.kind === 'pour' && (now - press.start) / 1000 < press.emitEnd && (now - press.start) / 1000 > press.go + press.tip) || (press.kind === 'fill' && (now - press.start) / 1000 < press.emitEnd));
  let count = 0;
  const maxR = fill ? 0.004 : 0.007;
  const radius = (drop: Drop) => Math.min(Math.max(0.7 * Math.sqrt(drop.q / (Math.PI * Math.max(speed(drop, time), 0.05))), 0.0007), maxR);
  for (let i = state.drops.length - 1; i >= 0 && count < RINGS - 1; i--) {
    const drop = state.drops[i];
    const r = rings[count];
    at(drop, time, r.p);
    const a = Math.max(time - drop.t, 0);
    r.v.set(drop.vx, drop.vy - G * a, drop.vz);
    r.r = radius(drop);
    count += 1;
  }
  if (count > 0 && state.head) {
    const last = rings[count - 1];
    const below = at(state.head, time, scratch.b);
    const floor = fill ? surfaceY(below.x, below.z) : state.wet.y;
    const k = Math.min(Math.max((last.p.y - floor) / Math.max(last.p.y - below.y, 1e-6), 0), 1);
    const r = rings[count];
    r.p.copy(last.p).lerp(below, k);
    r.v.copy(last.v);
    r.r = last.r;
    count += 1;
  } else if (count > 1) {
    rings[count - 1].r *= 0.4;
  }
  if (count > 1 && !emitting) rings[0].r *= 0.4;
  const side = press && press.kind === 'pour' ? scratch.side.set(-press.dir.z, 0, press.dir.x) : scratch.side.set(1, 0, 0);
  build(rings.slice(0, count), side);
  if (state.drops.length === 0 && !emitting) {
    state.head = null;
    if (!press) state.fill = false;
  }
}

function beadFrame(now: number) {
  const press = state.press;
  const parts = glass.parts;
  const time = now / 1000;
  if (press && parts && press.kind === 'pour') {
    const t = (now - press.start) / 1000;
    press.drips.forEach((when, index) => {
      const bead = state.beads[index];
      if (!bead || bead.alive || bead.land || t < when) return;
      const lip = lipNow(press, parts.holder, scratch.c);
      Object.assign(bead, { t: time, x: lip.x, y: lip.y - 0.003, z: lip.z, vx: press.dir.x * 0.04, vy: 0, vz: press.dir.z * 0.04, alive: true, land: 0 });
      bead.mesh.scale.setScalar(index % 2 ? 0.0024 : 0.003);
    });
  }
  for (const bead of state.beads) {
    if (!bead.alive) continue;
    at(bead, time, bead.mesh.position);
    bead.mesh.visible = true;
    if (bead.mesh.position.y <= state.wet.y) {
      bead.alive = false;
      bead.land = now;
      bead.mesh.visible = false;
      state.wet.at = now;
      state.wet.peak = Math.min(1, state.wet.peak + 0.03);
      if (!state.dripped) {
        state.dripped = true;
        play('drips');
      }
    }
  }
  if (!press) for (const bead of state.beads) if (!bead.alive) bead.land = 0;
}

function wetAmount(now: number) {
  return state.wet.peak * Math.max(0, 1 - Math.max(0, now - state.wet.at - 400) / DRY);
}

function wetFrame(now: number) {
  const wet = state.wet;
  const amount = wetAmount(now);
  if (amount <= 0 && wet.peak > 0) wet.peak = 0;
  const uniform = state.wetUniform.value;
  uniform[0] = wet.x;
  uniform[1] = wet.y;
  uniform[2] = wet.z;
  uniform[3] = amount;
  if (amount > 0) live.fadeUntil = Math.max(live.fadeUntil, now + 100);
}

function rippleFrame(step: number) {
  const ripple = glass.ripple;
  const parts = glass.parts;
  if (state.ripple <= 1e-5) {
    state.ripple = 0;
    ripple.w = 0;
    return;
  }
  if (parts) {
    ripple.x = parts.holder.position.x;
    ripple.y = parts.holder.position.z;
  }
  ripple.z += step * 28;
  ripple.w = state.ripple;
  state.ripple *= Math.exp(-step * (state.drops.length ? 1.5 : 4.5));
}

const wetCode = {
  vertex: ['#include <project_vertex>', '#include <project_vertex>\nvWetWorld = (modelMatrix * vec4(transformed, 1.0)).xyz;'] as const,
  fragment:
    '#include <normal_fragment_maps>\n{\nfloat wetUp = smoothstep(0.35, 0.75, dot(normal, (viewMatrix * vec4(0.0, 1.0, 0.0, 0.0)).xyz));\nfloat wetNear = 1.0 - smoothstep(0.012, 0.04, distance(vWetWorld.xz, uWet.xz));\nfloat wetBand = step(uWet.y - 0.03, vWetWorld.y) * step(vWetWorld.y, uWet.y + 0.02);\nfloat wet = uWet.w * wetUp * wetNear * wetBand;\ndiffuseColor.rgb *= 1.0 - 0.55 * wet;\nroughnessFactor = mix(roughnessFactor, 0.35, wet);\n}',
};

export function soak(gl: { compileAsync: (scene: Object3D, camera: Camera) => Promise<unknown> }, camera: Camera) {
  const root = things.plant;
  if (!root || state.patched === root) return;
  state.patched = root;
  const seen = new Set<MeshStandardMaterial>();
  root.traverse((child) => {
    const mesh = child as Mesh;
    if (!mesh.isMesh) return;
    const material = mesh.material as MeshStandardMaterial;
    if (!material.isMeshStandardMaterial || seen.has(material)) return;
    seen.add(material);
    const previous = material.onBeforeCompile;
    const key = material.customProgramCacheKey();
    material.onBeforeCompile = (shader, renderer) => {
      previous.call(material, shader, renderer);
      shader.uniforms.uWet = state.wetUniform;
      shader.vertexShader = `varying vec3 vWetWorld;\n${shader.vertexShader.replace(...wetCode.vertex)}`;
      shader.fragmentShader = `uniform vec4 uWet;\nvarying vec3 vWetWorld;\n${shader.fragmentShader.replace('#include <normal_fragment_maps>', wetCode.fragment)}`;
    };
    material.customProgramCacheKey = () => `${key}|wet`;
    material.needsUpdate = true;
  });
  void gl.compileAsync(root, camera).catch(() => undefined);
}

export function pourFrame(now: number, step: number, gl: { compileAsync: (scene: Object3D, camera: Camera) => Promise<unknown> }) {
  if (things.plant && state.patched !== things.plant && view.camera) soak(gl, view.camera);
  streamFrame(now);
  beadFrame(now);
  wetFrame(now);
  rippleFrame(step);
}

export function installPour(camera: Camera) {
  view.camera = camera;
  glass.drive = drive;
  glass.solve = (normal, volume) => {
    const parts = glass.parts;
    if (!parts) return normal.y * volume;
    if (!state.shape) state.shape = shapeOf(parts.profile, parts.floor);
    return solve(state.shape, normal, volume);
  };
  return () => {
    glass.drive = null;
    glass.solve = null;
    view.camera = null;
  };
}
