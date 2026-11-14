import { DataTexture, LinearFilter, Matrix4, PerspectiveCamera, RGBAFormat, UnsignedByteType, Vector3 } from 'three';

export const STREET = {
  position: new Vector3(2.4, 2.75, 1.25),
  target: new Vector3(-0.15, 1.05, -0.75),
  angle: 0.42,
};

export function blindsTexture() {
  const size = 256;
  const data = new Uint8Array(size * size * 4);
  const slats = 11;
  const clamp = (value: number) => Math.min(Math.max(value, 0), 1);
  const columns = new Float32Array(size);
  for (let x = 0; x < size; x++) {
    const u = x / (size - 1);
    columns[x] = clamp((u - 0.16) / 0.03) * clamp((0.84 - u) / 0.03) * (1 - clamp(1 - Math.abs(u - 0.5) / 0.012));
  }
  for (let y = 0; y < size; y++) {
    const v = y / (size - 1);
    const phase = (v * slats) % 1;
    const row = clamp((phase - 0.18) / 0.06) * clamp((0.74 - phase) / 0.07) * clamp((v - 0.12) / 0.03) * clamp((0.88 - v) / 0.03);
    for (let x = 0; x < size; x++) {
      const at = (y * size + x) * 4;
      data[at + 3] = 255;
      const light = row * columns[x];
      if (light === 0) continue;
      const u = x / (size - 1);
      const value = Math.round(255 * light * (0.94 + 0.06 * Math.sin(u * 37 + v * 11) * Math.sin(u * 13 - v * 29)));
      data[at] = value;
      data[at + 1] = value;
      data[at + 2] = value;
    }
  }
  const texture = new DataTexture(data, size, size, RGBAFormat, UnsignedByteType);
  texture.magFilter = LinearFilter;
  texture.minFilter = LinearFilter;
  texture.needsUpdate = true;
  return texture;
}

export function projector() {
  const camera = new PerspectiveCamera((STREET.angle * 2 * 180) / Math.PI, 1, 0.5, 8);
  camera.position.copy(STREET.position);
  camera.lookAt(STREET.target);
  camera.updateMatrixWorld();
  camera.updateProjectionMatrix();
  const bias = new Matrix4().set(0.5, 0, 0, 0.5, 0, 0.5, 0, 0.5, 0, 0, 0.5, 0.5, 0, 0, 0, 1);
  return bias.multiply(camera.projectionMatrix).multiply(camera.matrixWorldInverse);
}
