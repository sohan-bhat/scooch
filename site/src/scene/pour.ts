import { Box3, BufferAttribute, BufferGeometry, Group, InstancedMesh, Matrix4, Mesh, Quaternion, SphereGeometry, Vector3, type Camera, type MeshStandardMaterial, type Object3D } from 'three';
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
const RINGS = 120;
const SIDES = 8;
const STEP = 1 / 240;
const DRY = 6000;
const TILT = 0.025;

type Shape = { points: Float32Array; weights: Float32Array; total: number; rim: { r: number; y: number }; spill: Float32Array; scratch: Float32Array };
type Plant = { x: number; z: number; soil: number; rim: number; radius: number; root: Object3D };
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
  landed: 0,
  wet: { x: 0, y: 0, z: 0, peak: 0, at: 0 },
  wetUniform: { value: [0, 0, 0, 0] as number[] },
  ripple: 0,
  flowing: 0,
  dip: 0,
  surface: 0,
  beadMesh: null as InstancedMesh | null,
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
    const ax = plant.x + dir.x * (reach + 0.03);
    const az = plant.z + dir.z * (reach + 0.03);
    if (gx < DESK.left || gx > DESK.right || gz < DESK.back || gz > DESK.front) continue;
    if (blocks.some((block) => inside(block, gx, gz, OUTER + 0.008) || inside(block, ax, az, OUTER))) continue;
    const side = Math.abs(forward.x * dir.z - forward.z * dir.x);
    const behind = Math.max(0, forward.x * dir.x + forward.z * dir.z);
    const toward = (dir.x * (from.x - plant.x) + dir.z * (from.z - plant.z)) / Math.max(Math.hypot(from.x - plant.x, from.z - plant.z), 1e-6);
    const score = side * 1.4 - behind * 1.5 + toward * 0.8;
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
  resetStream();
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
  const pre: [number, number] = [p.x + dir.x * (p.radius * 1.12 + LIP.r + 0.03), p.z + dir.z * (p.radius * 1.12 + LIP.r + 0.03)];
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

const CAP = 640;
const DROPS = 64;
const pool = {
  t: new Float64Array(CAP),
  x: new Float32Array(CAP),
  y: new Float32Array(CAP),
  z: new Float32Array(CAP),
  vx: new Float32Array(CAP),
  vy: new Float32Array(CAP),
  vz: new Float32Array(CAP),
  q: new Float32Array(CAP),
  head: 0,
  count: 0,
};
const last = { on: false, index: CAP };
const spray = {
  t: new Float64Array(16),
  x: new Float32Array(16),
  y: new Float32Array(16),
  z: new Float32Array(16),
  vx: new Float32Array(16),
  vy: new Float32Array(16),
  vz: new Float32Array(16),
  r: new Float32Array(16),
  floor: new Float32Array(16),
  alive: new Uint8Array(16),
  next: 0,
};
const drips = { t: new Float64Array(4), x: new Float32Array(4), y: new Float32Array(4), z: new Float32Array(4), vx: new Float32Array(4), vz: new Float32Array(4), alive: new Uint8Array(4), done: new Uint8Array(4) };

function resetStream() {
  pool.head = 0;
  pool.count = 0;
  last.on = false;
  state.landed = 0;
  spray.alive.fill(0);
  drips.alive.fill(0);
  drips.done.fill(0);
}

function emit(t: number, x: number, y: number, z: number, vx: number, vy: number, vz: number, q: number) {
  if (pool.count >= CAP) return;
  const i = (pool.head + pool.count) % CAP;
  pool.t[i] = t;
  pool.x[i] = x;
  pool.y[i] = y;
  pool.z[i] = z;
  pool.vx[i] = vx;
  pool.vy[i] = vy;
  pool.vz[i] = vz;
  pool.q[i] = q;
  pool.count += 1;
}

function emitPour(press: Press, now: number, holder: Object3D) {
  const t = (now - press.start) / 1000;
  const begin = press.go + press.tip;
  const lip = lipNow(press, holder, scratch.c);
  while (press.emitted * STEP + begin <= Math.min(t, press.emitEnd)) {
    const te = press.emitted * STEP + begin;
    const u = (te - begin) / press.flow;
    const rate = (press.loss * 1.6 * (1 - u) ** 0.6) / press.flow;
    emit(press.start / 1000 + te, lip.x, lip.y, lip.z, press.dir.x * press.v0, -0.05, press.dir.z * press.v0, Math.max(rate, 0));
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
    const rate = ((-press.loss / press.flow) * fillShape(te / press.flow)) / state.sent;
    emit(press.start / 1000 + te, press.source.x, press.source.y, press.source.z, 0, -0.1, 0, Math.max(rate, 0));
    press.emitted += 1;
  }
}

function at(i: number, time: number, out: Vector3) {
  const a = Math.max(time - pool.t[i], 0);
  return out.set(pool.x[i] + pool.vx[i] * a, pool.y[i] + pool.vy[i] * a - 0.5 * G * a * a, pool.z[i] + pool.vz[i] * a);
}

function velocity(i: number, time: number, out: Vector3) {
  const a = Math.max(time - pool.t[i], 0);
  return out.set(pool.vx[i], pool.vy[i] - G * a, pool.vz[i]);
}

function surfaceY(x: number, z: number) {
  const plane = glass.plane;
  return (plane.w - plane.x * x - plane.z * z) / Math.max(plane.y, 1e-3);
}

export function makeStream() {
  const geometry = new BufferGeometry();
  const positions = new Float32Array(RINGS * SIDES * 3);
  const normals = new Float32Array(RINGS * SIDES * 3);
  const thin = new Float32Array(RINGS * SIDES);
  const index: number[] = [];
  for (let r = 0; r < RINGS - 1; r++) {
    for (let k = 0; k < SIDES; k++) {
      const a = r * SIDES + k;
      const b = r * SIDES + ((k + 1) % SIDES);
      index.push(a, a + SIDES, b, b, a + SIDES, b + SIDES);
    }
  }
  geometry.setAttribute('position', new BufferAttribute(positions, 3));
  geometry.setAttribute('normal', new BufferAttribute(normals, 3));
  geometry.setAttribute('aThin', new BufferAttribute(thin, 1));
  geometry.setIndex(index);
  geometry.setDrawRange(0, 0);
  const material = tumblerMaterial(2);
  const mesh = new Mesh(geometry, material);
  mesh.name = 'stream';
  mesh.frustumCulled = false;
  mesh.visible = false;
  const bead = new SphereGeometry(1, 12, 8);
  bead.setAttribute('aThin', new BufferAttribute(new Float32Array(bead.attributes.position.count).fill(0.8), 1));
  const beads = new InstancedMesh(bead, material, DROPS);
  beads.name = 'drops';
  beads.frustumCulled = false;
  beads.count = 0;
  const group = new Group();
  group.add(mesh, beads);
  state.stream = mesh;
  state.beadMesh = beads;
  return group;
}

const ringP = new Float32Array(RINGS * 3);
const ringV = new Float32Array(RINGS * 3);
const ringR = new Float32Array(RINGS);
const ring = { p: new Vector3(), v: new Vector3(), n: new Vector3(), b1: new Vector3(), b2: new Vector3(), m: new Matrix4(), q: new Quaternion(), s: new Vector3(), up: new Vector3(0, 1, 0) };

function setRing(k: number, p: Vector3, v: Vector3, r: number) {
  ringP[k * 3] = p.x;
  ringP[k * 3 + 1] = p.y;
  ringP[k * 3 + 2] = p.z;
  ringV[k * 3] = v.x;
  ringV[k * 3 + 1] = v.y;
  ringV[k * 3 + 2] = v.z;
  ringR[k] = r;
}

function build(count: number, side: Vector3) {
  const mesh = state.stream;
  if (!mesh) return;
  mesh.visible = count >= 2;
  if (count < 2) return;
  const geometry = mesh.geometry;
  const positions = geometry.attributes.position.array as Float32Array;
  const normals = geometry.attributes.normal.array as Float32Array;
  const thin = geometry.attributes.aThin.array as Float32Array;
  for (let i = 0; i < count; i++) {
    ring.v.set(ringV[i * 3], ringV[i * 3 + 1], ringV[i * 3 + 2]).normalize();
    ring.b1.copy(side).addScaledVector(ring.v, -side.dot(ring.v));
    if (ring.b1.lengthSq() < 1e-8) ring.b1.set(0, 0, 1);
    ring.b1.normalize();
    ring.b2.crossVectors(ring.v, ring.b1);
    const r = ringR[i];
    const clear = 1 - Math.min(Math.max((r - 0.0012) / 0.0035, 0), 1);
    for (let k = 0; k < SIDES; k++) {
      const angle = (k / SIDES) * Math.PI * 2;
      ring.n.copy(ring.b1).multiplyScalar(Math.cos(angle)).addScaledVector(ring.b2, Math.sin(angle));
      const j = i * SIDES + k;
      normals[j * 3] = ring.n.x;
      normals[j * 3 + 1] = ring.n.y;
      normals[j * 3 + 2] = ring.n.z;
      positions[j * 3] = ringP[i * 3] + ring.n.x * r;
      positions[j * 3 + 1] = ringP[i * 3 + 1] + ring.n.y * r;
      positions[j * 3 + 2] = ringP[i * 3 + 2] + ring.n.z * r;
      thin[j] = clear;
    }
  }
  geometry.attributes.position.needsUpdate = true;
  geometry.attributes.normal.needsUpdate = true;
  geometry.attributes.aThin.needsUpdate = true;
  geometry.setDrawRange(0, (count - 1) * SIDES * 6);
}

function placeBead(slot: number, p: Vector3, v: Vector3, r: number) {
  const beads = state.beadMesh;
  if (!beads || slot >= DROPS) return slot;
  ring.v.copy(v);
  const length = ring.v.length();
  if (length > 1e-4) ring.q.setFromUnitVectors(ring.up, ring.v.multiplyScalar(1 / length));
  else ring.q.identity();
  const stretch = 1 + Math.min(length * 0.12, 0.5);
  ring.s.set(r, r * stretch, r);
  ring.m.compose(p, ring.q, ring.s);
  beads.setMatrixAt(slot, ring.m);
  return slot + 1;
}

function streamFrame(now: number) {
  const press = state.press;
  const time = now / 1000;
  const parts = glass.parts;
  if (press && parts && press.kind === 'pour') emitPour(press, now, parts.holder);
  if (press && press.kind === 'fill') emitFill(press, now);
  const fill = state.fill;
  const point = scratch.a;
  const vel = scratch.b;
  if (parts) state.surface = surfaceY(parts.holder.position.x, parts.holder.position.z);
  let landedNow = 0;
  while (pool.count) {
    const i = pool.head;
    at(i, time, point);
    const floor = fill ? surfaceY(point.x, point.z) : state.wet.y;
    if (point.y > floor) break;
    last.on = true;
    last.index = i;
    state.landed += pool.q[i] * STEP;
    pool.head = (pool.head + 1) % CAP;
    pool.count -= 1;
    landedNow += 1;
    if (press && !press.landed) {
      press.landed = true;
      play(fill ? 'pour' : 'stream');
    }
    if (fill) state.dip = Math.min(state.dip + pool.q[i] * STEP * 900, 0.5);
    else {
      state.wet.at = now;
      state.wet.peak = Math.min(1, state.wet.peak + (pool.q[i] * STEP * 0.9) / Math.max(press?.loss ?? 1e-5, 1e-6));
      if (landedNow === 1 && Math.sin(pool.t[i] * 997) > 0.55) splash(point, now, floor);
    }
  }
  if (fill && landedNow) state.ripple = Math.min(state.ripple + landedNow * 0.012, 0.22);
  const sinceStart = press ? (now - press.start) / 1000 : 0;
  const emitting = !!press && ((press.kind === 'pour' && sinceStart < press.emitEnd && sinceStart > press.go + press.tip) || (press.kind === 'fill' && sinceStart < press.emitEnd));
  const endAt = press ? press.start / 1000 + press.emitEnd : time;
  const maxR = fill ? 0.0042 : 0.0072;
  let count = 0;
  let beads = 0;
  const landY = fill ? state.surface : state.wet.y;
  let broken = false;
  for (let k = pool.count - 1; k >= 0 && count < RINGS - 1; k--) {
    const i = (pool.head + k) % CAP;
    at(i, time, point);
    velocity(i, time, vel);
    const speed = Math.max(vel.length(), 0.05);
    const te = pool.t[i];
    let r = Math.min(Math.max(0.7 * Math.sqrt(pool.q[i] / (Math.PI * speed)), 0.0007), maxR);
    r *= 1 + 0.14 * Math.sin(te * 41 + time * 5.3) + 0.06 * Math.sin(te * 97 - time * 8.1);
    const sway = Math.min((time - te) / 0.12, 1) * (fill ? 0.0011 : 0.0006);
    point.x += sway * Math.sin(te * 23 + time * 6.1);
    point.z += sway * Math.cos(te * 19 - time * 4.7);
    const fall = (pool.y[i] - point.y) / Math.max(pool.y[i] - landY, 0.01);
    const lower = fill ? fall > 0.95 && r < 0.0024 : fall > 0.7 && r < 0.0034;
    const tail = !emitting && endAt - te < 0.07;
    if (lower || tail || broken) {
      if (lower) broken = true;
      const step = Math.round(te / STEP);
      if (fill || step % 2 === 0) beads = placeBead(beads, point, vel, r * (1.15 + 0.2 * Math.sin(te * 311)));
      continue;
    }
    setRing(count, point, vel, r);
    count += 1;
  }
  if (count > 0 && last.on && pool.count && !broken) {
    const k = count - 1;
    at(last.index, time, vel);
    const ly = fill ? surfaceY(vel.x, vel.z) : state.wet.y;
    const py = ringP[k * 3 + 1];
    const f = Math.min(Math.max((py - ly) / Math.max(py - vel.y, 1e-6), 0), 1);
    point.set(ringP[k * 3], py, ringP[k * 3 + 2]).lerp(vel, f);
    setRing(count, point, scratch.c.set(ringV[k * 3], ringV[k * 3 + 1], ringV[k * 3 + 2]), ringR[k]);
    count += 1;
  } else if (count > 1) ringR[count - 1] *= 0.45;
  if (count > 1 && !emitting) ringR[0] *= 0.45;
  const side = press && press.kind === 'pour' ? scratch.side.set(-press.dir.z, 0, press.dir.x) : scratch.side.set(1, 0, 0);
  build(count, side);
  beads = sprayFrame(now, beads);
  beads = dripFrame(now, beads);
  if (state.beadMesh) {
    state.beadMesh.count = beads;
    state.beadMesh.instanceMatrix.needsUpdate = true;
  }
  state.flowing = pool.count;
  if (pool.count === 0 && !emitting) {
    last.on = false;
    if (!press) state.fill = false;
  }
}

function splash(at: Vector3, now: number, floor: number) {
  for (let n = 0; n < 3; n++) {
    const i = spray.next;
    spray.next = (spray.next + 1) % 16;
    const angle = (now * 0.013 + n * 2.1) % (Math.PI * 2);
    const speed = 0.12 + 0.1 * ((n * 0.37 + now * 0.0007) % 1);
    spray.t[i] = now / 1000;
    spray.x[i] = at.x;
    spray.y[i] = floor + 0.002;
    spray.z[i] = at.z;
    spray.vx[i] = Math.cos(angle) * speed;
    spray.vz[i] = Math.sin(angle) * speed;
    spray.vy[i] = 0.35 + 0.25 * ((n * 0.61 + now * 0.0003) % 1);
    spray.r[i] = 0.0011 + 0.0007 * n;
    spray.floor[i] = floor;
    spray.alive[i] = 1;
  }
}

function sprayFrame(now: number, slot: number) {
  const time = now / 1000;
  for (let i = 0; i < 16; i++) {
    if (!spray.alive[i]) continue;
    const a = time - spray.t[i];
    ring.p.set(spray.x[i] + spray.vx[i] * a, spray.y[i] + spray.vy[i] * a - 0.5 * G * a * a, spray.z[i] + spray.vz[i] * a);
    if (ring.p.y < spray.floor[i] || a > 0.4) {
      spray.alive[i] = 0;
      continue;
    }
    slot = placeBead(slot, ring.p, scratch.b.set(spray.vx[i], spray.vy[i] - G * a, spray.vz[i]), spray.r[i]);
  }
  return slot;
}

function dripFrame(now: number, slot: number) {
  const press = state.press;
  const parts = glass.parts;
  const time = now / 1000;
  if (press && parts && press.kind === 'pour') {
    const t = (now - press.start) / 1000;
    for (let k = 0; k < 4; k++) {
      if (drips.alive[k] || drips.done[k] || t < press.drips[k]) continue;
      const lip = lipNow(press, parts.holder, scratch.c);
      drips.t[k] = time;
      drips.x[k] = lip.x;
      drips.y[k] = lip.y - 0.003;
      drips.z[k] = lip.z;
      drips.vx[k] = press.dir.x * 0.04;
      drips.vz[k] = press.dir.z * 0.04;
      drips.alive[k] = 1;
    }
  }
  for (let k = 0; k < 4; k++) {
    if (!drips.alive[k]) continue;
    const a = time - drips.t[k];
    ring.p.set(drips.x[k] + drips.vx[k] * a, drips.y[k] - 0.5 * G * a * a, drips.z[k] + drips.vz[k] * a);
    if (ring.p.y <= state.wet.y) {
      drips.alive[k] = 0;
      drips.done[k] = 1;
      state.wet.at = now;
      state.wet.peak = Math.min(1, state.wet.peak + 0.03);
      if (!state.dripped) {
        state.dripped = true;
        play('drips');
      }
      continue;
    }
    slot = placeBead(slot, ring.p, scratch.b.set(drips.vx[k], -G * a, drips.vz[k]), k % 2 ? 0.0024 : 0.003);
  }
  return slot;
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
  glass.axis.w = state.dip > 1e-4 ? state.dip : 0;
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
  state.ripple *= Math.exp(-step * (pool.count ? 1.5 : 4.5));
  state.dip *= Math.exp(-step * (pool.count ? 3 : 7));
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
  wetFrame(now);
  rippleFrame(step);
}

export function warmStream(gl: { compileAsync: (scene: Object3D, camera: Camera) => Promise<unknown> }, camera: Camera, group: Object3D) {
  const mesh = state.stream;
  const beads = state.beadMesh;
  if (!mesh || !beads) return;
  mesh.visible = true;
  ring.m.makeScale(1e-7, 1e-7, 1e-7);
  beads.setMatrixAt(0, ring.m);
  beads.instanceMatrix.needsUpdate = true;
  beads.count = 1;
  void gl.compileAsync(group, camera).catch(() => undefined).finally(() => {
    if (!pool.count) {
      mesh.visible = false;
      beads.count = 0;
    }
  });
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
