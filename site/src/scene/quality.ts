import type { DepthOfFieldEffect, EffectComposer, NoiseEffect } from 'postprocessing';
import type { Texture, Vector3 } from 'three';

export type Toggle = { enabled: boolean };

export type Tier = 'full' | 'light';
export type GpuClass = 'low' | 'mid' | 'high';

type Probe = { gpu: GpuClass; renderer: string; extensions: Set<string> };

const compressed = [
  'WEBGL_compressed_texture_astc',
  'WEBGL_compressed_texture_etc1',
  'WEBGL_compressed_texture_etc',
  'WEBGL_compressed_texture_s3tc',
  'EXT_texture_compression_bptc',
  'WEBGL_compressed_texture_pvrtc',
  'WEBKIT_WEBGL_compressed_texture_pvrtc',
];

let probed: Probe | null = null;

function classify(renderer: string): GpuClass {
  const mobile = /Android|iPhone|iPad|Mobi/i.test(navigator.userAgent);
  if (mobile || /Intel|Mali|Adreno|PowerVR|SwiftShader|llvmpipe|Software|Basic Render/i.test(renderer)) return 'low';
  if (/Radeon|AMD/i.test(renderer)) return 'mid';
  return 'high';
}

export function gpuClass(): Probe {
  if (probed) return probed;
  const extensions = new Set<string>();
  try {
    const canvas = document.createElement('canvas');
    canvas.width = 1;
    canvas.height = 1;
    const gl = canvas.getContext('webgl2', { powerPreference: 'high-performance' });
    if (!gl) return (probed = { gpu: 'low', renderer: 'none', extensions });
    const plain = String(gl.getParameter(gl.RENDERER));
    const info = /^(WebKit WebGL|Mozilla)?$/.test(plain) ? gl.getExtension('WEBGL_debug_renderer_info') : null;
    const renderer = info ? String(gl.getParameter(info.UNMASKED_RENDERER_WEBGL)) : plain;
    for (const name of compressed) if (gl.getExtension(name)) extensions.add(name);
    const astc = gl.getExtension('WEBGL_compressed_texture_astc');
    if (astc?.getSupportedProfiles().includes('hdr')) extensions.add('astc-hdr');
    probed = { gpu: classify(renderer), renderer, extensions };
  } catch {
    probed = { gpu: 'low', renderer: 'unknown', extensions };
  }
  return probed;
}

type Settings = { sharp: number; dof: boolean; ao: boolean; grain: boolean };

const settings: Record<GpuClass, Settings> = {
  high: { sharp: 1.5, dof: true, ao: true, grain: true },
  mid: { sharp: 1, dof: false, ao: false, grain: true },
  low: { sharp: 0.85, dof: false, ao: false, grain: true },
};

const FAST = 0.7;
const MAX_BOKEH = 3.2;
const GRAIN = 0.05;

export type Chain = 'sharp' | 'fast';

class Quality {
  dofs: DepthOfFieldEffect[] = [];
  composer: EffectComposer | null = null;
  fastComposer: EffectComposer | null = null;

  tier: Tier = 'full';
  gpu: GpuClass = 'high';
  dof = true;
  ao = true;
  grain = true;
  sharpDpr = 1;
  fastScale = 1;
  needsFast = false;
  decided = false;
  chain: Chain = 'sharp';
  blendAt = -1e9;
  lastFast: Texture | null = null;
  fastReady = false;

  configure(phone: boolean, deviceDpr: number, gpu: GpuClass) {
    this.gpu = phone ? 'low' : gpu;
    this.tier = this.gpu === 'low' ? 'light' : 'full';
    const chosen = phone ? { sharp: 1, dof: false, ao: false, grain: true } : settings[this.gpu];
    this.dof = chosen.dof;
    this.ao = chosen.ao;
    this.grain = chosen.grain;
    this.sharpDpr = Math.min(chosen.sharp, Math.max(deviceDpr, 1));
    this.fastScale = Math.min(1, FAST / this.sharpDpr);
    this.needsFast = this.gpu !== 'high' && this.fastScale < 0.99;
  }

  get hasFast() {
    return this.fastScale < 0.99;
  }

  grainOpacity() {
    return this.grain ? GRAIN : 0;
  }

  attachDof(effect: DepthOfFieldEffect | null) {
    if (!effect || this.dofs.includes(effect)) return;
    this.dofs.push(effect);
    const update = effect.update.bind(effect);
    effect.update = (renderer, input, delta) => {
      if (effect.blendMode.opacity.value > 0) update(renderer, input, delta);
    };
    effect.blendMode.opacity.value = 0;
  }

  focus(screen: Vector3, camera: Vector3, mix: number, wide: Vector3, out: Vector3, amount: number) {
    if (!this.dofs.length) return;
    const on = this.dof && amount > 0.002;
    if (on) out.copy(wide).lerp(screen, mix);
    for (const effect of this.dofs) {
      effect.blendMode.opacity.value = on ? 1 : 0;
      if (!on) continue;
      effect.cocMaterial.focusDistance = camera.distanceTo(out);
      effect.cocMaterial.focusRange = 0.06 + camera.distanceTo(out) * 0.08;
      effect.bokehScale = amount * MAX_BOKEH;
    }
  }
}

export const quality = new Quality();
