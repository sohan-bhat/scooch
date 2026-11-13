import type { DepthOfFieldEffect, EffectComposer, NoiseEffect } from 'postprocessing';
import type { Vector3 } from 'three';

export type Toggle = { enabled: boolean };

type Step = { dpr: number; dof: boolean; ao: boolean; grain: boolean };

const fullSteps: Step[] = [
  { dpr: 2, dof: true, ao: true, grain: true },
  { dpr: 1.75, dof: true, ao: true, grain: true },
  { dpr: 1.5, dof: true, ao: true, grain: true },
  { dpr: 1.25, dof: true, ao: true, grain: true },
  { dpr: 1, dof: true, ao: true, grain: true },
  { dpr: 1, dof: true, ao: false, grain: true },
  { dpr: 1, dof: false, ao: false, grain: true },
  { dpr: 0.85, dof: false, ao: false, grain: true },
  { dpr: 0.7, dof: false, ao: false, grain: true },
  { dpr: 0.7, dof: false, ao: false, grain: false },
];

const lightSteps: Step[] = [
  { dpr: 1.5, dof: false, ao: false, grain: true },
  { dpr: 1.25, dof: false, ao: false, grain: true },
  { dpr: 1, dof: false, ao: false, grain: true },
  { dpr: 0.85, dof: false, ao: false, grain: true },
  { dpr: 0.7, dof: false, ao: false, grain: true },
  { dpr: 0.7, dof: false, ao: false, grain: false },
];

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
    const gl = canvas.getContext('webgl2', { powerPreference: 'high-performance' });
    if (!gl) return (probed = { gpu: 'low', renderer: 'none', extensions });
    const info = gl.getExtension('WEBGL_debug_renderer_info');
    const renderer = String(gl.getParameter(info ? info.UNMASKED_RENDERER_WEBGL : gl.RENDERER));
    for (const name of compressed) if (gl.getExtension(name)) extensions.add(name);
    const astc = gl.getExtension('WEBGL_compressed_texture_astc');
    if (astc?.getSupportedProfiles().includes('hdr')) extensions.add('astc-hdr');
    gl.getExtension('WEBGL_lose_context')?.loseContext();
    probed = { gpu: classify(renderer), renderer, extensions };
  } catch {
    probed = { gpu: 'low', renderer: 'unknown', extensions };
  }
  return probed;
}

const starts: Record<GpuClass, Step> = {
  high: { dpr: 1.5, dof: true, ao: true, grain: true },
  mid: { dpr: 1, dof: true, ao: false, grain: true },
  low: { dpr: 0.7, dof: false, ao: false, grain: true },
};

const MAX_BOKEH = 3.2;
const GRAIN = 0.05;

class Quality {
  steps: Step[] = fullSteps;
  level = 0;
  dof: DepthOfFieldEffect | null = null;
  ao: Toggle | null = null;
  noise: NoiseEffect | null = null;
  composer: EffectComposer | null = null;
  setDpr: ((dpr: number) => void) | null = null;
  onChange: (() => void) | null = null;

  tier: Tier = 'full';
  gpu: GpuClass = 'high';

  configure(phone: boolean, deviceDpr: number, gpu: GpuClass) {
    this.gpu = phone ? 'low' : gpu;
    this.tier = this.gpu === 'low' ? 'light' : 'full';
    const all = this.tier === 'light' ? lightSteps : fullSteps;
    const fit = all.filter((step) => step.dpr <= Math.max(deviceDpr, 1) + 0.01);
    this.steps = fit.length > 0 ? fit : [all[all.length - 1]];
    const start = phone ? { dpr: 1, dof: false, ao: false, grain: true } : starts[this.gpu];
    const index = this.steps.findIndex((step) => step.dpr <= start.dpr && step.ao === start.ao && step.dof === start.dof && step.grain === start.grain);
    this.level = index >= 0 ? index : 0;
  }

  get step() {
    return this.steps[Math.min(this.level, this.steps.length - 1)];
  }

  canDown() {
    return this.level < this.steps.length - 1;
  }

  canUp() {
    return this.level > 0;
  }

  down() {
    if (!this.canDown()) return;
    this.level += 1;
    this.apply();
  }

  up() {
    if (!this.canUp()) return;
    this.level -= 1;
    this.apply();
  }

  apply() {
    const step = this.step;
    this.setDpr?.(Math.min(step.dpr, window.devicePixelRatio || 1));
    if (this.ao) this.ao.enabled = step.ao;
    if (this.noise) this.noise.blendMode.opacity.value = step.grain ? GRAIN : 0;
    this.onChange?.();
  }

  attachDof(effect: DepthOfFieldEffect | null) {
    this.dof = effect;
    if (!effect) return;
    const update = effect.update.bind(effect);
    effect.update = (renderer, input, delta) => {
      if (effect.blendMode.opacity.value > 0) update(renderer, input, delta);
    };
    effect.blendMode.opacity.value = 0;
  }

  focus(screen: Vector3, camera: Vector3, mix: number, wide: Vector3, out: Vector3, amount: number) {
    const effect = this.dof;
    if (!effect) return;
    const on = this.step.dof && amount > 0.002;
    effect.blendMode.opacity.value = on ? 1 : 0;
    if (!on) return;
    out.copy(wide).lerp(screen, mix);
    effect.cocMaterial.focusDistance = camera.distanceTo(out);
    effect.cocMaterial.focusRange = 0.06 + camera.distanceTo(out) * 0.08;
    effect.bokehScale = amount * MAX_BOKEH;
  }
}

export const quality = new Quality();
export const GRAIN_OPACITY = GRAIN;
