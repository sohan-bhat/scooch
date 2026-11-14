import { Box3, Mesh, MeshStandardMaterial, Object3D, PlaneGeometry, Quaternion, Vector3, type Texture, type WebGLRenderer } from 'three';
import { quiet } from '@/lib/idle';
import { canvasTexture, dialPainter, fontsReady, paintedTexture, sticky as stickyText, typed as typedText, type Reveal } from './captions';

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

function revealDecal(painters: Reveal, width: number, height: number, size: number[], options: { opaque?: boolean } = {}) {
  const blank = canvasTexture(painters.blank ? 16 : 1, painters.blank ? 128 : 1, true);
  if (blank.context && painters.blank) painters.blank(blank.context, 16, 128, 1);
  const hidden = canvasTexture(1, 1, true, false);
  if (hidden.context) {
    hidden.context.fillStyle = '#fff';
    hidden.context.fillRect(0, 0, 1, 1);
  }
  const full = canvasTexture(size[0], size[1], true);
  const order = canvasTexture(size[0], size[1], true, false);
  const uniforms = { uReveal: { value: -1 }, uBlank: { value: blank.texture }, uOrder: { value: hidden.texture } };
  const mesh = decal(blank.texture, width, height, options);
  const material = mesh.material as MeshStandardMaterial;
  material.onBeforeCompile = (shader) => {
    Object.assign(shader.uniforms, uniforms);
    shader.fragmentShader = `uniform sampler2D uBlank;\nuniform sampler2D uOrder;\nuniform float uReveal;\n${shader.fragmentShader.replace(
      '#include <map_fragment>',
      '#ifdef USE_MAP\nvec4 sampledDiffuseColor = mix(texture2D(uBlank, vMapUv), texture2D(map, vMapUv), step(texture2D(uOrder, vMapUv).r, uReveal));\ndiffuseColor *= sampledDiffuseColor;\n#endif',
    )}`;
  };
  material.customProgramCacheKey = () => 'reveal';
  const ready = async (gl: WebGLRenderer) => {
    await fontsReady();
    await quiet();
    if (full.context) painters.full(full.context, size[0], size[1], 1);
    await quiet();
    if (order.context) painters.order(order.context, size[0], size[1], 1);
    await quiet();
    gl.initTexture(blank.texture);
    await quiet();
    gl.initTexture(full.texture);
    await quiet();
    gl.initTexture(order.texture);
    material.map = full.texture;
    uniforms.uOrder.value = order.texture;
  };
  const reveal = (amount: number) => {
    uniforms.uReveal.value = amount > 0 ? amount : -1;
  };
  return { mesh, ready, reveal };
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
  const dial = paintedTexture(512, 512, dialPainter, true, false);
  let typed: ReturnType<typeof revealDecal> | null = null;

  const pad = group.getObjectByName('notepads_office_notepads_yellow_pad');
  if (pad) {
    const box = localBox(pad);
    const size = box.getSize(new Vector3());
    const long = Math.max(size.x, size.z);
    const short = Math.min(size.x, size.z);
    typed = revealDecal(typedText, short * 0.86, long * 0.84, [512, 680]);
    const mesh = typed.mesh;
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
    async ready(gl: WebGLRenderer) {
      await fontsReady();
      await quiet();
      dial.draw(1, true);
      await quiet();
      gl.initTexture(dial.texture);
      await typed?.ready(gl);
    },
    update(amount: number) {
      typed?.reveal(amount);
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
  const sticky = revealDecal(stickyText, 0.1, 0.1, [512, 512], { opaque: true });
  const note = sticky.mesh;
  note.position
    .copy(glass.center)
    .addScaledVector(glass.right, glass.width * 0.32)
    .addScaledVector(glass.up, -glass.height * 0.5 - 0.068)
    .addScaledVector(glass.normal, 0.03);
  note.lookAt(note.position.clone().add(glass.normal));
  note.rotateZ(-0.08);
  return { note, write: sticky.reveal, ready: sticky.ready };
}
