import { useTexture } from '@react-three/drei';
import { useMemo } from 'react';
import { Color, Group, LinearMipmapLinearFilter, Matrix4, Mesh, SRGBColorSpace, Texture, Vector3 } from 'three';
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

function useScreenTextures(manifest: DemoManifest) {
  const { frames } = manifest;
  const textures = useTexture([
    asset(`demo/${frames.before.frame}`),
    asset(`demo/${frames.fixed.frame}`),
    asset(`demo/${frames.moved.frame}`),
    asset(`demo/${frames.before.saliency}`),
    asset('demo/atlas.png'),
  ]);
  const [frameA, frameB, frameC, saliency, atlas] = textures;
  for (const texture of [frameA, frameB, frameC, atlas]) prepareTexture(texture, true, 8);
  prepareTexture(saliency, false, 1);
  return textures;
}

export function ScreenTextures({ manifest }: { manifest: DemoManifest }) {
  useScreenTextures(manifest);
  return null;
}

export function useScreen(manifest: DemoManifest) {
  const { frames } = manifest;
  const [frameA, frameB, frameC, saliency, atlas] = useScreenTextures(manifest);

  return useMemo(() => {
    const { material, uniforms } = createScreenMaterial();
    uniforms.uFrameA.value = frameA;
    uniforms.uFrameB.value = frameB;
    uniforms.uFrameC.value = frameC;
    uniforms.uSaliency.value = saliency;
    uniforms.uAtlas.value = atlas;
    uniforms.uAspect.value = manifest.screen.width / manifest.screen.height;
    uniforms.uRoom.value = tokenColor('room');
    uniforms.uHeat.value = tokenColor('lamp');
    uniforms.uGlow.value = tokenColor('pad');
    uniforms.uHot.value = tokenColor('paper');
    uniforms.uRim.value = tokenColor('rim');
    const averages = [frames.before.average, frames.fixed.average, frames.moved.average].map(
      ([r, g, b]) => new Color().setRGB(r, g, b, SRGBColorSpace),
    );
    return { material, uniforms, averages };
  }, [frameA, frameB, frameC, saliency, atlas, manifest, frames]);
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
