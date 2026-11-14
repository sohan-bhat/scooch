import { Matrix4, Object3D, Vector2, Vector3, type BufferGeometry, type Mesh } from 'three';

export type Turn = (date: Date) => number;

export type Dial = { pivot: Vector3; normal: Vector3; up: Vector3; right: Vector3 };

type Hand = { name: string; node: Object3D; points: Vector3[]; center: Vector3; axis: Vector3; turn: Turn };

function pointsOf(node: Object3D, toClock: Matrix4) {
  const points: Vector3[] = [];
  const matrix = new Matrix4();
  node.traverse((child) => {
    const mesh = child as Mesh;
    if (!mesh.isMesh) return;
    const position = (mesh.geometry as BufferGeometry).getAttribute('position');
    if (!position) return;
    matrix.multiplyMatrices(toClock, mesh.matrixWorld);
    for (let i = 0; i < position.count; i++) points.push(new Vector3().fromBufferAttribute(position, i).applyMatrix4(matrix));
  });
  return points;
}

function principal(points: Vector3[], center: Vector3) {
  let axis = new Vector3(0.6, 0.5, 0.3).normalize();
  const offset = new Vector3();
  for (let step = 0; step < 32; step++) {
    const next = new Vector3();
    for (const point of points) {
      offset.subVectors(point, center);
      next.addScaledVector(offset, offset.dot(axis));
    }
    if (next.lengthSq() === 0) break;
    axis = next.normalize();
  }
  return axis;
}

function flat(vector: Vector3, normal: Vector3) {
  return vector.clone().addScaledVector(normal, -vector.dot(normal));
}

export function dialOf(hands: { points: Vector3[]; center: Vector3; axis: Vector3 }[], front: Vector3) {
  let normal = new Vector3();
  for (let i = 0; i < hands.length; i++) {
    for (let j = i + 1; j < hands.length; j++) {
      const cross = new Vector3().crossVectors(hands[i].axis, hands[j].axis);
      if (cross.lengthSq() > normal.lengthSq()) normal = cross;
    }
  }
  if (normal.lengthSq() < 0.04) return null;
  normal.normalize();
  if (normal.dot(front) < 0) normal.negate();
  const up = flat(new Vector3(0, 1, 0), normal).normalize();
  const right = new Vector3().crossVectors(up, normal).normalize();
  let xx = 0;
  let xy = 0;
  let yy = 0;
  let bx = 0;
  let by = 0;
  let depth = 0;
  for (const hand of hands) {
    const a = new Vector2(hand.axis.dot(right), hand.axis.dot(up)).normalize();
    const c = new Vector2(hand.center.dot(right), hand.center.dot(up));
    const m00 = 1 - a.x * a.x;
    const m01 = -a.x * a.y;
    const m11 = 1 - a.y * a.y;
    xx += m00;
    xy += m01;
    yy += m11;
    bx += m00 * c.x + m01 * c.y;
    by += m01 * c.x + m11 * c.y;
    depth += hand.center.dot(normal);
  }
  const determinant = xx * yy - xy * xy;
  if (Math.abs(determinant) < 1e-6) return null;
  const px = (yy * bx - xy * by) / determinant;
  const py = (xx * by - xy * bx) / determinant;
  const pivot = new Vector3().addScaledVector(right, px).addScaledVector(up, py).addScaledVector(normal, depth / hands.length);
  return { pivot, normal, up, right } as Dial;
}

export function restAngle(points: Vector3[], dial: Dial) {
  let far = -1;
  const tip = new Vector3();
  const offset = new Vector3();
  for (const point of points) {
    offset.copy(flat(offset.subVectors(point, dial.pivot), dial.normal));
    const reach = offset.lengthSq();
    if (reach > far) {
      far = reach;
      tip.copy(offset);
    }
  }
  return Math.atan2(tip.dot(dial.right), tip.dot(dial.up));
}

export function clockHands(clock: Object3D, turns: Record<string, Turn>, middle: Vector3) {
  clock.updateWorldMatrix(true, true);
  const toClock = new Matrix4().copy(clock.matrixWorld).invert();
  const hands: Hand[] = [];
  for (const [name, turn] of Object.entries(turns)) {
    const node = clock.getObjectByName(name);
    if (!node) continue;
    const points = pointsOf(node, toClock);
    if (points.length < 3) continue;
    const center = points.reduce((sum, point) => sum.add(point), new Vector3()).divideScalar(points.length);
    hands.push({ name, node, points, center, axis: principal(points, center), turn });
  }
  const front = hands.reduce((sum, hand) => sum.add(hand.center), new Vector3()).divideScalar(Math.max(hands.length, 1)).sub(middle);
  const dial = hands.length >= 2 ? dialOf(hands, front) : null;
  if (!dial) return { dial: null, set: () => undefined };
  const turning = hands.map((hand) => {
    const parent = hand.node.parent ?? clock;
    const fromClock = new Matrix4().copy(parent.matrixWorld).invert().multiply(clock.matrixWorld);
    const holder = new Object3D();
    holder.name = `${hand.name}_turn`;
    holder.position.copy(dial.pivot).applyMatrix4(fromClock);
    parent.add(holder);
    holder.updateWorldMatrix(true, false);
    holder.attach(hand.node);
    const axis = dial.normal.clone().transformDirection(fromClock);
    return { holder, axis, angle: restAngle(hand.points, dial), turn: hand.turn };
  });
  return {
    dial,
    set(date: Date) {
      for (const hand of turning) hand.holder.quaternion.setFromAxisAngle(hand.axis, -(hand.turn(date) * Math.PI * 2 - hand.angle));
    },
  };
}
