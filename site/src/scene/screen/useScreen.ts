import { useTexture } from '@react-three/drei';
import { useMemo } from 'react';
import { ClampToEdgeWrapping, Color, DataTexture, FloatType, Group, LinearMipmapLinearFilter, Matrix4, Mesh, NearestFilter, RedFormat, SRGBColorSpace, Texture, Vector3 } from 'three';
import { asset } from '@/lib/base';
import { tokenColor } from '@/lib/tokens';
import type { DemoManifest } from '@/story/demo';
import type { ScreenFrame } from '../rig';
import { createScreenMaterial } from './material';

function prepareTexture(texture: Texture, srgb: boolean, anisotropy: number) {
  if (texture.userData.prepared) return;
  texture.userData.prepared = true;
  if (srgb) texture.colorSpace = SRGBColorSpace;
  texture.flipY = false;
  texture.generateMipmaps = true;
  texture.minFilter = LinearMipmapLinearFilter;
  texture.anisotropy = anisotropy;
  texture.needsUpdate = true;
}

function tablesTexture(manifest: DemoManifest) {
  const rows = manifest.order.flatMap((id) => [manifest.tables[id].fx, manifest.tables[id].fy]);
  const width = rows[0].length;
  const data = new Float32Array(width * rows.length);
  rows.forEach((row, index) => data.set(row, index * width));
  const texture = new DataTexture(data, width, rows.length, RedFormat, FloatType);
  texture.minFilter = NearestFilter;
  texture.magFilter = NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}

function useScreenTextures(manifest: DemoManifest) {
  const textures = useTexture([
    asset(`demo/${manifest.photo.file}`),
    asset(`demo/${manifest.frames.before.saliency}`),
    asset('demo/atlas.png'),
  ]);
  const [photo, saliency, atlas] = textures;
  if (!photo.userData.prepared) {
    photo.wrapS = ClampToEdgeWrapping;
    photo.wrapT = ClampToEdgeWrapping;
  }
  prepareTexture(photo, true, 8);
  prepareTexture(atlas, true, 8);
  prepareTexture(saliency, false, 1);
  return textures;
}

export function ScreenTextures({ manifest }: { manifest: DemoManifest }) {
  useScreenTextures(manifest);
  return null;
}

export function useScreen(manifest: DemoManifest) {
  const { frames, order } = manifest;
  const [photo, saliency, atlas] = useScreenTextures(manifest);

  return useMemo(() => {
    const { material, uniforms } = createScreenMaterial();
    uniforms.uPhoto.value = photo;
    uniforms.uTables.value = tablesTexture(manifest);
    uniforms.uStates.value = order.length;
    uniforms.uSaliency.value = saliency;
    uniforms.uAtlas.value = atlas;
    uniforms.uAspect.value = manifest.screen.width / manifest.screen.height;
    uniforms.uRoom.value = tokenColor('room');
    uniforms.uHeat.value = tokenColor('lamp');
    uniforms.uGlow.value = tokenColor('pad');
    uniforms.uHot.value = tokenColor('paper');
    uniforms.uRim.value = tokenColor('rim');
    const averages = order.map((id) => {
      const [r, g, b] = frames[id].average;
      return new Color().setRGB(r, g, b, SRGBColorSpace);
    });
    return { material, uniforms, averages };
  }, [photo, saliency, atlas, manifest, frames, order]);
}

export function measureScreen(mesh: Mesh, frame: Group, aspect: number) {
  mesh.geometry.computeBoundingBox();
  const box = mesh.geometry.boundingBox!;
  const glassWidth = box.max.x - box.min.x;
  const glassHeight = box.max.y - box.min.y;
  let width = glassWidth * 0.95;
  let height = width / aspect;
  if (height > glassHeight * 0.93) {
    height = glassHeight * 0.93;
    width = height * aspect;
  }
  const centerX = (box.min.x + box.max.x) / 2;
  const centerY = (box.min.y + box.max.y) / 2 + (glassHeight - height) * 0.04;
  const origin = { x: centerX - width / 2, y: centerY - height / 2, width, height };

  mesh.updateWorldMatrix(true, false);
  frame.updateWorldMatrix(true, false);
  const toFrame = new Matrix4().copy(frame.matrixWorld).invert().multiply(mesh.matrixWorld);
  const center = new Vector3(centerX, centerY, box.max.z).applyMatrix4(toFrame);
  const right = new Vector3(centerX + width / 2, centerY, box.max.z).applyMatrix4(toFrame);
  const top = new Vector3(centerX, centerY + height / 2, box.max.z).applyMatrix4(toFrame);
  const normal = new Vector3(0, 0, 1).transformDirection(toFrame);
  const up = new Vector3().subVectors(top, center).normalize();
  const screen: ScreenFrame = {
    center,
    normal,
    up,
    width: right.distanceTo(center) * 2,
    height: top.distanceTo(center) * 2,
  };
  return { origin, screen };
}
