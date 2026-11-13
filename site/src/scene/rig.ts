import { CatmullRomCurve3, MathUtils, Matrix4, PerspectiveCamera, Vector3 } from 'three';
import type { Frame, Key } from './shots';

export type ScreenFrame = {
  center: Vector3;
  normal: Vector3;
  up: Vector3;
  width: number;
  height: number;
};

export class Rig {
  private positions: CatmullRomCurve3;
  private targets: CatmullRomCurve3;
  private fovs: number[];
  private shiftsX: number[];
  private shiftsY: number[];
  private position = new Vector3();
  private target = new Vector3();
  private forward = new Vector3();
  private right = new Vector3();
  private up = new Vector3();
  private worldUp = new Vector3(0, 1, 0);
  private yaw = 0;
  private pitch = 0;
  readonly count: number;

  constructor(keys: Key[], frames: Record<Frame, Matrix4>, aspect: number) {
    const positions: Vector3[] = [];
    const targets: Vector3[] = [];
    this.fovs = [];
    this.shiftsX = [];
    this.shiftsY = [];
    for (const key of keys) {
      const frame = frames[key.frame];
      positions.push(new Vector3(...key.position).applyMatrix4(frame));
      targets.push(new Vector3(...key.target).applyMatrix4(frame));
      this.fovs.push(this.fitFov(key.fov, aspect));
      this.shiftsX.push(key.shiftX);
      this.shiftsY.push(key.shiftY);
    }
    this.positions = new CatmullRomCurve3(positions, false, 'centripetal');
    this.targets = new CatmullRomCurve3(targets, false, 'centripetal');
    this.count = keys.length;
  }

  private fitFov(fov: number, aspect: number) {
    const reference = 1.6;
    if (aspect >= reference || aspect < 0.9) return fov;
    const horizontal = Math.atan(Math.tan(MathUtils.degToRad(fov) / 2) * reference);
    return MathUtils.radToDeg(2 * Math.atan(Math.tan(horizontal) / aspect));
  }

  private lerpAt(values: number[], u: number) {
    const i = Math.min(Math.floor(u), values.length - 2);
    const f = MathUtils.clamp(u - i, 0, 1);
    return values[i] + (values[i + 1] - values[i]) * f;
  }

  apply(camera: PerspectiveCamera, u: number, time: number, pointerX: number, pointerY: number, delta: number, width: number, height: number, idle: boolean) {
    const t = MathUtils.clamp(u / (this.count - 1), 0, 1);
    this.positions.getPoint(t, this.position);
    this.targets.getPoint(t, this.target);
    const distance = this.position.distanceTo(this.target);
    if (idle) {
      this.position.y += Math.sin(time * 0.52) * 0.0025;
      this.position.x += Math.sin(time * 0.31 + 1.3) * 0.002;
      this.position.z += Math.sin(time * 0.27 + 0.4) * 0.0015;
    }
    const damping = 1 - Math.exp(-delta * 2.2);
    this.yaw += (pointerX * MathUtils.degToRad(1.5) - this.yaw) * damping;
    this.pitch += (pointerY * MathUtils.degToRad(1.0) - this.pitch) * damping;
    this.forward.subVectors(this.target, this.position).normalize();
    this.right.crossVectors(this.forward, this.worldUp).normalize();
    this.up.crossVectors(this.right, this.forward).normalize();
    this.target.addScaledVector(this.right, Math.tan(this.yaw) * distance);
    this.target.addScaledVector(this.up, -Math.tan(this.pitch) * distance);
    camera.position.copy(this.position);
    camera.lookAt(this.target);
    camera.fov = this.lerpAt(this.fovs, u);
    const shiftX = this.lerpAt(this.shiftsX, u);
    const shiftY = this.lerpAt(this.shiftsY, u);
    camera.setViewOffset(width, height, (-shiftX * width) / 2, (shiftY * height) / 2, width, height);
    camera.updateProjectionMatrix();
    return distance;
  }

  focusPoint(out: Vector3) {
    return out.copy(this.target);
  }
}
