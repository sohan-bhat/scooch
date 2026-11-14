import { BufferAttribute, BufferGeometry } from 'three';
import { MeshBVH, type MeshBVHOptions, type SerializedBVH } from 'three-mesh-bvh';

type Job = {
  id: number;
  position: Float32Array;
  index: Uint16Array | Uint32Array | null;
  groups: { start: number; count: number; materialIndex?: number }[];
};

type Indirect = SerializedBVH & { indirectBuffer: Uint32Array | null };

self.onmessage = ({ data }: MessageEvent<Job>) => {
  try {
    const geometry = new BufferGeometry();
    geometry.setAttribute('position', new BufferAttribute(data.position, 3));
    if (data.index) geometry.setIndex(new BufferAttribute(data.index, 1));
    for (const group of data.groups) geometry.addGroup(group.start, group.count, group.materialIndex);
    const bvh = new MeshBVH(geometry, { indirect: true } as MeshBVHOptions);
    const { roots, indirectBuffer } = MeshBVH.serialize(bvh, { cloneBuffers: false }) as Indirect;
    const transfer: ArrayBuffer[] = [...roots];
    if (indirectBuffer) transfer.push(indirectBuffer.buffer as ArrayBuffer);
    self.postMessage({ id: data.id, roots, indirectBuffer }, { transfer });
  } catch {
    self.postMessage({ id: data.id, roots: null, indirectBuffer: null });
  }
};
