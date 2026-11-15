import { acceleratedRaycast, MeshBVH, type SerializedBVH } from 'three-mesh-bvh';
import type { BufferGeometry, Mesh, Object3D } from 'three';
import { quiet } from '@/lib/idle';

type Reply = { id: number; roots: ArrayBuffer[] | null; indirectBuffer: Uint32Array | null };

let worker: Worker | null = null;
let next = 0;
const waiting = new Map<number, (reply: Reply) => void>();
const building = new WeakMap<BufferGeometry, Promise<boolean>>();
const started = new WeakSet<Object3D>();

function host() {
  if (!worker) {
    worker = new Worker(new URL('./bounds.worker.ts', import.meta.url), { type: 'module' });
    worker.onmessage = ({ data }: MessageEvent<Reply>) => {
      const resolve = waiting.get(data.id);
      waiting.delete(data.id);
      resolve?.(data);
    };
  }
  return worker;
}

function positions(geometry: BufferGeometry) {
  const attribute = geometry.getAttribute('position');
  const plain = !('isInterleavedBufferAttribute' in attribute && attribute.isInterleavedBufferAttribute) && attribute.itemSize === 3 && !attribute.normalized && attribute.array instanceof Float32Array;
  if (plain) return (attribute.array as Float32Array).slice(0, attribute.count * 3);
  const out = new Float32Array(attribute.count * 3);
  for (let i = 0; i < attribute.count; i++) {
    out[i * 3] = attribute.getX(i);
    out[i * 3 + 1] = attribute.getY(i);
    out[i * 3 + 2] = attribute.getZ(i);
  }
  return out;
}

function build(geometry: BufferGeometry) {
  const known = building.get(geometry);
  if (known) return known;
  const job = new Promise<boolean>((resolve) => {
    const position = positions(geometry);
    const index = geometry.index ? (geometry.index.array as Uint16Array | Uint32Array).slice() : null;
    const id = (next += 1);
    waiting.set(id, ({ roots, indirectBuffer }) => {
      if (!roots) return resolve(false);
      geometry.boundsTree = MeshBVH.deserialize({ roots, index: null, indirectBuffer } as unknown as SerializedBVH, geometry, { setIndex: false });
      resolve(true);
    });
    const transfer: ArrayBuffer[] = [position.buffer];
    if (index) transfer.push(index.buffer as ArrayBuffer);
    host().postMessage({ id, position, index, groups: geometry.groups.map(({ start, count, materialIndex }) => ({ start, count, materialIndex })) }, { transfer });
  });
  building.set(geometry, job);
  return job;
}

export function bounds(root: Object3D) {
  if (started.has(root)) return;
  started.add(root);
  const meshes: Mesh[] = [];
  root.traverse((child) => {
    if ((child as Mesh).isMesh) meshes.push(child as Mesh);
  });
  void (async () => {
    for (const mesh of meshes) {
      if (!mesh.geometry.boundsTree) {
        await quiet();
        if (!(await build(mesh.geometry))) continue;
      }
      mesh.raycast = acceleratedRaycast;
    }
  })();
}
