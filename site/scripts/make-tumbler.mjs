import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Document, NodeIO } from '@gltf-transform/core';
import { ALL_EXTENSIONS } from '@gltf-transform/extensions';
import { meshopt, weld } from '@gltf-transform/functions';
import { MeshoptEncoder } from 'meshoptimizer';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = { url: 'https://opengameart.org/sites/default/files/kitchenthings.blend', object: 'OBGlass' };
const cached = join(site, '.cache', 'opengameart', 'kitchenthings.blend');
const target = join(site, 'public', 'models', 'tumbler.glb');
const HEIGHT = 0.1;

async function download() {
  if (await stat(cached).then(() => true, () => false)) return;
  const response = await fetch(source.url, { headers: { 'User-Agent': 'scooch-site-build/1.0' } });
  if (!response.ok) throw new Error(`${response.status} ${source.url}`);
  await mkdir(dirname(cached), { recursive: true });
  await writeFile(cached, Buffer.from(await response.arrayBuffer()));
}

function blendMeshes(buffer) {
  if (buffer.toString('latin1', 0, 7) !== 'BLENDER') throw new Error('not a blend file');
  const wide = buffer[7] === 45;
  const little = buffer[8] === 118;
  const size = wide ? 8 : 4;
  const i32 = (at) => (little ? buffer.readInt32LE(at) : buffer.readInt32BE(at));
  const u32 = (at) => (little ? buffer.readUInt32LE(at) : buffer.readUInt32BE(at));
  const i16 = (at) => (little ? buffer.readInt16LE(at) : buffer.readInt16BE(at));
  const f32 = (at) => (little ? buffer.readFloatLE(at) : buffer.readFloatBE(at));
  const pointer = (at) => (wide ? (little ? buffer.readBigUInt64LE(at) : buffer.readBigUInt64BE(at)) : BigInt(u32(at)));
  const blocks = [];
  let at = 12;
  while (at < buffer.length) {
    const code = buffer.toString('latin1', at, at + 4).replace(/\0/g, '');
    const length = i32(at + 4);
    const old = pointer(at + 8);
    const sdna = i32(at + 8 + size);
    const count = i32(at + 12 + size);
    const body = at + 16 + size;
    blocks.push({ code, length, old, sdna, count, body });
    at = body + length;
    if (code === 'ENDB') break;
  }
  const dna = blocks.find((block) => block.code === 'DNA1');
  let p = dna.body + 4;
  const align = (value) => (value + 3) & ~3;
  const strings = () => {
    const count = i32(p + 4);
    p += 8;
    const list = [];
    for (let n = 0; n < count; n++) {
      const end = buffer.indexOf(0, p);
      list.push(buffer.toString('latin1', p, end));
      p = end + 1;
    }
    p = align(p);
    return list;
  };
  const names = strings();
  const types = strings();
  p += 4;
  const lengths = types.map((_, n) => i16(p + n * 2));
  p = align(p + types.length * 2);
  const structCount = i32(p + 4);
  p += 8;
  const structs = [];
  for (let n = 0; n < structCount; n++) {
    const type = types[i16(p)];
    const fieldCount = i16(p + 2);
    p += 4;
    const fields = {};
    let offset = 0;
    for (let f = 0; f < fieldCount; f++) {
      const fieldType = types[i16(p)];
      const name = names[i16(p + 2)];
      p += 4;
      const dims = [...name.matchAll(/\[(\d+)\]/g)].reduce((product, match) => product * Number(match[1]), 1);
      const base = name.replace(/\[.*$/, '');
      const isPointer = base.startsWith('*') || base.startsWith('(*');
      const key = base.replace(/[()*]/g, '').replace(/\(.*$/, '');
      const width = isPointer ? size : lengths[types.indexOf(fieldType)];
      fields[key] = offset;
      offset += width * dims;
    }
    structs.push({ type, fields });
  }
  const struct = (name) => structs.find((entry) => entry.type === name);
  const byOld = new Map(blocks.map((block) => [block.old, block]));
  const object = struct('Object');
  const mesh = struct('Mesh');
  const id = struct('ID');
  const found = {};
  for (const block of blocks.filter((entry) => entry.code === 'OB')) {
    const nameAt = block.body + object.fields.id + id.fields.name;
    const name = buffer.toString('latin1', nameAt, buffer.indexOf(0, nameAt));
    const data = byOld.get(pointer(block.body + object.fields.data));
    if (!data || structs[data.sdna].type !== 'Mesh') continue;
    const read = (field) => byOld.get(pointer(data.body + mesh.fields[field]));
    const verts = read('mvert');
    const polys = read('mpoly');
    const loops = read('mloop');
    const vertSize = verts.length / verts.count;
    const polySize = polys.length / polys.count;
    const loopSize = loops.length / loops.count;
    const co = structs[verts.sdna].fields.co;
    const loopStart = structs[polys.sdna].fields.loopstart;
    const loopTotal = structs[polys.sdna].fields.totloop;
    const loopVert = structs[loops.sdna].fields.v;
    const positions = [];
    for (let n = 0; n < verts.count; n++) {
      const base = verts.body + n * vertSize + co;
      positions.push([f32(base), f32(base + 4), f32(base + 8)]);
    }
    const loopVerts = [];
    for (let n = 0; n < loops.count; n++) loopVerts.push(u32(loops.body + n * loopSize + loopVert));
    const faces = [];
    for (let n = 0; n < polys.count; n++) {
      const start = i32(polys.body + n * polySize + loopStart);
      const total = i32(polys.body + n * polySize + loopTotal);
      faces.push(loopVerts.slice(start, start + total));
    }
    found[name] = { positions, faces };
  }
  return found;
}

function subdivide({ positions, faces }) {
  const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
  const scale = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
  const facePoints = faces.map((face) => scale(face.reduce((sum, v) => add(sum, positions[v]), [0, 0, 0]), 1 / face.length));
  const edges = new Map();
  const key = (a, b) => (a < b ? `${a}_${b}` : `${b}_${a}`);
  faces.forEach((face, f) => {
    face.forEach((v, n) => {
      const w = face[(n + 1) % face.length];
      const k = key(v, w);
      if (!edges.has(k)) edges.set(k, { a: v, b: w, faces: [] });
      edges.get(k).faces.push(f);
    });
  });
  const edgePoints = new Map();
  const vertexFaces = positions.map(() => []);
  const vertexEdges = positions.map(() => []);
  faces.forEach((face, f) => face.forEach((v) => vertexFaces[v].push(f)));
  for (const [k, edge] of edges) {
    const mid = scale(add(positions[edge.a], positions[edge.b]), 0.5);
    vertexEdges[edge.a].push(k);
    vertexEdges[edge.b].push(k);
    if (edge.faces.length === 2) {
      edgePoints.set(k, scale(add(add(positions[edge.a], positions[edge.b]), add(facePoints[edge.faces[0]], facePoints[edge.faces[1]])), 0.25));
    } else edgePoints.set(k, mid);
  }
  const moved = positions.map((p, v) => {
    const around = vertexEdges[v].map((k) => edges.get(k));
    const border = around.filter((edge) => edge.faces.length !== 2);
    if (border.length) {
      const ends = border.map((edge) => scale(add(positions[edge.a], positions[edge.b]), 0.5));
      if (border.length !== 2) return p;
      return add(scale(p, 0.5), scale(add(ends[0], ends[1]), 0.25));
    }
    const n = vertexFaces[v].length;
    const q = scale(vertexFaces[v].reduce((sum, f) => add(sum, facePoints[f]), [0, 0, 0]), 1 / n);
    const r = scale(around.reduce((sum, edge) => add(sum, scale(add(positions[edge.a], positions[edge.b]), 0.5)), [0, 0, 0]), 1 / around.length);
    return scale(add(add(q, scale(r, 2)), scale(p, n - 3)), 1 / n);
  });
  const out = [...moved];
  const faceIndex = facePoints.map((point) => out.push(point) - 1);
  const edgeIndex = new Map();
  for (const [k, point] of edgePoints) edgeIndex.set(k, out.push(point) - 1);
  const quads = [];
  faces.forEach((face, f) => {
    face.forEach((v, n) => {
      const next = face[(n + 1) % face.length];
      const prev = face[(n + face.length - 1) % face.length];
      quads.push([v, edgeIndex.get(key(v, next)), faceIndex[f], edgeIndex.get(key(prev, v))]);
    });
  });
  return { positions: out, faces: quads };
}

function normals(positions, triangles) {
  const out = positions.map(() => [0, 0, 0]);
  for (let t = 0; t < triangles.length; t += 3) {
    const [a, b, c] = [triangles[t], triangles[t + 1], triangles[t + 2]].map((n) => positions[n]);
    const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    const w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    const n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
    for (const v of [triangles[t], triangles[t + 1], triangles[t + 2]]) for (let k = 0; k < 3; k++) out[v][k] += n[k];
  }
  return out.map((n) => {
    const length = Math.hypot(...n) || 1;
    return n.map((value) => value / length);
  });
}

function innerProfile(positions, triangles, top) {
  const floor = Math.max(...positions.filter((p) => Math.hypot(p[0], p[2]) < top * 0.05).map((p) => p[1]));
  const radiiAt = (y) => {
    const radii = [];
    for (let t = 0; t < triangles.length; t += 3) {
      const corners = [triangles[t], triangles[t + 1], triangles[t + 2]].map((n) => positions[n]);
      for (let e = 0; e < 3; e++) {
        const a = corners[e];
        const b = corners[(e + 1) % 3];
        if ((a[1] - y) * (b[1] - y) > 0 || a[1] === b[1]) continue;
        const s = (y - a[1]) / (b[1] - a[1]);
        radii.push(Math.hypot(a[0] + (b[0] - a[0]) * s, a[2] + (b[2] - a[2]) * s));
      }
    }
    return radii.sort((p, q) => p - q);
  };
  const profile = [];
  for (let n = 1; n < 120; n++) {
    const y = (top * n) / 120;
    const radii = radiiAt(y);
    if (!radii.length) continue;
    const outer = radii[radii.length - 1];
    const inner = radii[0];
    if (y > floor + 0.0005 && outer - inner > outer * 0.04) profile.push([Number(inner.toFixed(5)), Number(y.toFixed(5))]);
  }
  return { floor: Number(floor.toFixed(5)), profile };
}

await download();
const meshes = blendMeshes(await readFile(cached));
const raw = meshes[source.object];
if (!raw) throw new Error(`no ${source.object} in ${Object.keys(meshes).join(', ')}`);
const shape = subdivide(raw);
const xs = shape.positions.map((p) => p[0]);
const ys = shape.positions.map((p) => p[1]);
const zs = shape.positions.map((p) => p[2]);
const cx = (Math.min(...xs) + Math.max(...xs)) / 2;
const cy = (Math.min(...ys) + Math.max(...ys)) / 2;
const bottom = Math.min(...zs);
const factor = HEIGHT / (Math.max(...zs) - bottom);
const positions = shape.positions.map(([x, y, z]) => [(x - cx) * factor, (z - bottom) * factor, -(y - cy) * factor]);
const triangles = [];
for (const face of shape.faces) for (let n = 1; n < face.length - 1; n++) triangles.push(face[0], face[n], face[n + 1]);
const vertexNormals = normals(positions, triangles);
const { floor, profile } = innerProfile(positions, triangles, HEIGHT);

await MeshoptEncoder.ready;
const document = new Document();
const buffer = document.createBuffer();
const material = document.createMaterial('tumbler_glass').setBaseColorFactor([0.86, 0.9, 0.92, 0.2]).setAlphaMode('BLEND').setRoughnessFactor(0.05).setMetallicFactor(0);
const primitive = document
  .createPrimitive()
  .setAttribute('POSITION', document.createAccessor().setType('VEC3').setArray(new Float32Array(positions.flat())).setBuffer(buffer))
  .setAttribute('NORMAL', document.createAccessor().setType('VEC3').setArray(new Float32Array(vertexNormals.flat())).setBuffer(buffer))
  .setIndices(document.createAccessor().setType('SCALAR').setArray(new Uint32Array(triangles)).setBuffer(buffer))
  .setMaterial(material);
const node = document.createNode('tumbler').setMesh(document.createMesh('tumbler').addPrimitive(primitive)).setExtras({ floor, inner: profile.filter((_, n) => n % 4 === 0 || n === profile.length - 1) });
document.createScene('tumbler').addChild(node);
await document.transform(weld(), meshopt({ encoder: MeshoptEncoder, level: 'medium' }));
const io = new NodeIO().registerExtensions(ALL_EXTENSIONS).registerDependencies({ 'meshopt.encoder': MeshoptEncoder });
const bytes = await io.writeBinary(document);
await mkdir(dirname(target), { recursive: true });
await writeFile(target, bytes);
console.log('tumbler', positions.length, 'vertices', triangles.length / 3, 'triangles', floor, profile.length, 'profile rows', (bytes.byteLength / 1024).toFixed(1), 'KB');
