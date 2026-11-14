import { Box3, Mesh, MeshStandardMaterial, Object3D, PlaneGeometry, Vector3, type Texture } from 'three';
import { dialPainter, fontsReady, paintedTexture, stickyPainter, typedPainter } from './captions';

function decal(map: Texture, width: number, height: number, options: { emissive?: boolean; opaque?: boolean } = {}) {
  const material = new MeshStandardMaterial({
    map,
    transparent: !options.opaque,
    depthWrite: !!options.opaque,
    roughness: 0.92,
    metalness: 0,
    polygonOffset: true,
    polygonOffsetFactor: -2,
    polygonOffsetUnits: -2,
  });
  if (options.emissive) {
    material.emissiveMap = map;
    material.emissive.set('#bff0b4');
    material.emissiveIntensity = 1.6;
  }
  const mesh = new Mesh(new PlaneGeometry(width, height), material);
  mesh.name = 'caption';
  mesh.castShadow = false;
  mesh.receiveShadow = !options.emissive;
  return mesh;
}

function top(holder: Object3D) {
  const copy = holder.children[0];
  const box = new Box3();
  const saved = holder.matrixWorld.clone();
  holder.matrixWorld.identity();
  copy.updateWorldMatrix(false, true);
  box.setFromObject(copy);
  holder.matrixWorld.copy(saved);
  copy.updateWorldMatrix(false, true);
  return box;
}

export function labelProps(group: Object3D) {
  const typed = paintedTexture(512, 680, typedPainter, true);
  const dial = paintedTexture(512, 512, dialPainter, true);
  typed.draw(0, true);
  void fontsReady().then(() => {
    typed.draw(0, true);
    dial.draw(1, true);
  });

  const pad = group.getObjectByName('notepads_office_notepads_yellow_pad');
  if (pad) {
    const box = top(pad);
    const size = box.getSize(new Vector3());
    const long = Math.max(size.x, size.z);
    const short = Math.min(size.x, size.z);
    const mesh = decal(typed.texture, short * 0.86, long * 0.84);
    mesh.rotation.set(-Math.PI / 2, 0, size.x > size.z ? Math.PI / 2 : 0);
    mesh.position.set((box.min.x + box.max.x) / 2, box.max.y + 0.0006, (box.min.z + box.max.z) / 2 + long * 0.02);
    pad.add(mesh);
  }

  const clock = group.getObjectByName('clock_whole');
  if (clock) {
    const box = top(clock);
    const size = box.getSize(new Vector3());
    const mesh = decal(dial.texture, size.x * 0.62, size.x * 0.62, { emissive: true });
    mesh.position.set(0, size.y * 0.48, box.max.z + 0.0015);
    clock.add(mesh);
  }

  return {
    update(amount: number) {
      typed.draw(Math.round(amount * 64) / 64);
    },
  };
}

export type Glass = {
  center: Vector3;
  normal: Vector3;
  up: Vector3;
  right: Vector3;
  width: number;
  height: number;
};

export function stickyNote(glass: Glass) {
  const sticky = paintedTexture(512, 512, stickyPainter, true);
  sticky.draw(0, true);
  void fontsReady().then(() => sticky.draw(0, true));
  const note = decal(sticky.texture, 0.122, 0.122, { opaque: true });
  note.position
    .copy(glass.center)
    .addScaledVector(glass.right, glass.width * 0.5 - 0.012)
    .addScaledVector(glass.up, -glass.height * 0.5 + 0.012)
    .addScaledVector(glass.normal, 0.03);
  note.lookAt(note.position.clone().add(glass.normal));
  note.rotateZ(-0.08);
  return { note, write: (amount: number) => sticky.draw(Math.round(amount * 48) / 48) };
}
