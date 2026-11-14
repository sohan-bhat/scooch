import { useTexture } from '@react-three/drei';
import { useThree } from '@react-three/fiber';
import { useMemo } from 'react';
import { suspend } from 'suspend-react';
import { ClampToEdgeWrapping, Color, Group, LinearMipmapLinearFilter, Matrix4, Mesh, SRGBColorSpace, Texture, Vector2, Vector3 } from 'three';
import { asset } from '@/lib/base';
import { tokenColor } from '@/lib/tokens';
import type { DemoManifest } from '@/story/demo';
import type { ScreenFrame } from '../rig';
import { createCrtMaterial } from './crt';
import { createLcdMaterial } from './material';
import { createFrame, loadScenery } from './scenery';

function prepareTexture(texture: Texture, srgb: boolean, anisotropy: number) {
  if (texture.userData.prepared) return;
  texture.userData.prepared = true;
  if (srgb) texture.colorSpace = SRGBColorSpace;
  texture.flipY = false;
  texture.generateMipmaps = true;
  texture.minFilter = LinearMipmapLinearFilter;
  texture.anisotropy = anisotropy;
  texture.wrapS = ClampToEdgeWrapping;
  texture.wrapT = ClampToEdgeWrapping;
  texture.needsUpdate = true;
}

function useScreenTextures(manifest: DemoManifest) {
  const [saliency, atlas] = useTexture([asset(`demo/${manifest.photo.saliency}`), asset('demo/atlas.png')]);
  prepareTexture(atlas, true, 8);
  prepareTexture(saliency, false, 1);
  const main = suspend(() => loadScenery(manifest.scenery, true), ['scooch-scenery-main']);
  return { saliency, atlas, main };
}

export function ScreenTextures({ manifest }: { manifest: DemoManifest }) {
  useScreenTextures(manifest);
  return null;
}

function colors(averages: [number, number, number][]) {
  return averages.map(([r, g, b]) => new Color().setRGB(r, g, b, SRGBColorSpace));
}

export function useScreens(manifest: DemoManifest) {
  const { saliency, atlas, main } = useScreenTextures(manifest);
  const gl = useThree((state) => state.gl);

  return useMemo(() => {
    const buffer = gl.getDrawingBufferSize(new Vector2());
    const { width, height } = main.info;
    const scale = Math.min(1, Math.max(buffer.x, buffer.y) / Math.max(width, height));
    const mainFrame = createFrame(main, { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) });
    const crt = createCrtMaterial();
    crt.uniforms.uFrame.value = mainFrame.texture;
    crt.uniforms.uAtlas.value = atlas;
    crt.uniforms.uRim.value = tokenColor('rim');
    const lcd = createLcdMaterial();
    lcd.uniforms.uAtlas.value = atlas;
    lcd.uniforms.uRim.value = tokenColor('rim');
    const picture = manifest.pictures[0];
    manifest.icons.forEach((icon, index) => {
      const [x, y, w, h] = picture.icons[icon.id];
      lcd.uniforms.uIcons.value[index]?.set(x, y, w, h);
    });
    const { before, fixed, moved } = manifest.frames;
    return {
      crt,
      lcd,
      saliency,
      atlas,
      main: mainFrame,
      balloon: colors([before.average, fixed.average, fixed.average, moved.average]),
      secondColor: colors([picture.frames.fixed.average, picture.frames.fixed.average]),
    };
  }, [saliency, atlas, main, manifest, gl]);
}

export function measureScreen(mesh: Mesh, frame: Group, aspect: number, fill = [0.95, 0.93]) {
  mesh.geometry.computeBoundingBox();
  const box = mesh.geometry.boundingBox!;
  const glassWidth = box.max.x - box.min.x;
  const glassHeight = box.max.y - box.min.y;
  let width = glassWidth * fill[0];
  let height = width / aspect;
  if (height > glassHeight * fill[1]) {
    height = glassHeight * fill[1];
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
