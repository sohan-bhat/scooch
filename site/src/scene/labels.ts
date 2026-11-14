import { Box3, Mesh, MeshStandardMaterial, Object3D, PlaneGeometry, Quaternion, Vector3, type Texture } from 'three';
import { dialPainter, fontsReady, paintedTexture, stickyPainter, typedPainter } from './captions';

function decal(map: Texture, width: number, height: number, options: { opaque?: boolean } = {}) {
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
  const mesh = new Mesh(new PlaneGeometry(width, height), material);
  mesh.name = 'caption';
  mesh.castShadow = false;
  mesh.receiveShadow = true;
  return mesh;
}

export function localBox(holder: Object3D) {
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
    const box = localBox(pad);
    const size = box.getSize(new Vector3());
    const long = Math.max(size.x, size.z);
    const short = Math.min(size.x, size.z);
    const mesh = decal(typed.texture, short * 0.86, long * 0.84);
    mesh.rotation.set(-Math.PI / 2, 0, size.x > size.z ? Math.PI / 2 : 0);
    mesh.position.set((box.min.x + box.max.x) / 2, box.max.y + 0.0006, (box.min.z + box.max.z) / 2 + long * 0.02);
    pad.add(mesh);
  }

  const clock = group.getObjectByName('clock_whole');
  const hands: { node: Object3D; rest: Quaternion; angle: number; turn: (date: Date) => number }[] = [];
  if (clock) {
    const box = localBox(clock);
    const size = box.getSize(new Vector3());
    const mesh = decal(dial.texture, size.x * 0.62, size.x * 0.62);
    mesh.position.set(0, size.y * 0.48, box.max.z + 0.0015);
    clock.add(mesh);
    const turns: Record<string, (date: Date) => number> = {
      houd_hand: (date) => ((date.getHours() % 12) + date.getMinutes() / 60) / 12,
      minute_hand: (date) => (date.getMinutes() + date.getSeconds() / 60) / 60,
      second_hand: (date) => (date.getSeconds() + date.getMilliseconds() / 1000) / 60,
    };
    for (const [name, turn] of Object.entries(turns)) {
      const node = clock.getObjectByName(name);
      const mesh = node?.children.find((child) => (child as Mesh).isMesh) as Mesh | undefined;
      const target = (node as Mesh)?.isMesh ? (node as Mesh) : mesh;
      if (!node || !target) continue;
      target.geometry.computeBoundingBox();
      const center = target.geometry.boundingBox!.getCenter(new Vector3());
      if (target !== node) center.applyMatrix4(target.matrix);
      center.applyQuaternion(node.quaternion);
      hands.push({ node, rest: node.quaternion.clone(), angle: Math.atan2(center.x, center.y), turn });
    }
    if (hands.length) {
      clock.updateWorldMatrix(true, true);
      const pivot = clock.worldToLocal(hands[0].node.getWorldPosition(new Vector3()));
      mesh.position.set(pivot.x, pivot.y, box.max.z + 0.0015);
    }
  }
  const spin = new Quaternion();
  const axis = new Vector3(0, 0, 1);

  return {
    update(amount: number) {
      typed.draw(Math.round(amount * 64) / 64);
      const now = new Date();
      for (const hand of hands) {
        spin.setFromAxisAngle(axis, -(hand.turn(now) * Math.PI * 2 - hand.angle));
        hand.node.quaternion.multiplyQuaternions(spin, hand.rest);
      }
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
  const note = decal(sticky.texture, 0.1, 0.1, { opaque: true });
  note.position
    .copy(glass.center)
    .addScaledVector(glass.right, glass.width * 0.32)
    .addScaledVector(glass.up, -glass.height * 0.5 - 0.068)
    .addScaledVector(glass.normal, 0.03);
  note.lookAt(note.position.clone().add(glass.normal));
  note.rotateZ(-0.08);
  return { note, write: (amount: number) => sticky.draw(Math.round(amount * 48) / 48) };
}
