import type { DepthOfFieldEffect, EffectComposer, NoiseEffect } from 'postprocessing';
import type { Texture, Vector2, Vector3, WebGLRenderer } from 'three';

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

const ORDER: GpuClass[] = ['low', 'mid', 'high'];

function lower(a: GpuClass, b: GpuClass) {
  return ORDER[Math.min(ORDER.indexOf(a), ORDER.indexOf(b))];
}

export type Detected = { tier: number; type: string } | null;

export function firstGuess(named: GpuClass, detected: Detected): GpuClass {
  if (!detected) return lower(named, 'mid');
  if (detected.type === 'BENCHMARK') return lower(named, detected.tier >= 3 ? 'high' : detected.tier === 2 ? 'mid' : 'low');
  if (detected.type === 'FALLBACK') return lower(named, 'mid');
  return 'low';
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

type Settings = { sharp: number; fast: number; dof: boolean; ao: boolean; grain: boolean };

const settings: Record<GpuClass, Settings> = {
  high: { sharp: 1.5, fast: 0.7, dof: true, ao: true, grain: true },
  mid: { sharp: 1, fast: 0.85, dof: false, ao: false, grain: true },
  low: { sharp: 0.85, fast: 0.7, dof: false, ao: false, grain: true },
};

const FAST: number | undefined = undefined;
const LATE: boolean | undefined = undefined;
const NATIVE = 2;
const EARN = 2000;
const EARN_LIMIT = 32000;
const COMFORT = 0.6;
const REFRESH = [1000 / 240, 1000 / 165, 1000 / 144, 1000 / 120, 1000 / 100, 1000 / 90, 1000 / 75, 1000 / 60];
const MAX_BOKEH = 3.2;
const GRAIN = 0.05;

export type Chain = 'sharp' | 'fast';

class Quality {
  dofs: DepthOfFieldEffect[] = [];
  composer: EffectComposer | null = null;
  fastComposer: EffectComposer | null = null;
  nativeComposer: EffectComposer | null = null;
  hold = false;

  tier: Tier = 'full';
  phone = false;
  gpu: GpuClass = 'high';
  dof = true;
  ao = true;
  grain = true;
  sharpDpr = 1;
  nativeDpr = 1;
  liveScale = 1;
  stillScale = 1;
  late = true;
  switched = false;
  onNative: (() => void) | null = null;
  fastScale = 1;
  chain: Chain = 'sharp';
  switchAt = -1e9;
  blendAt = -1e9;
  lastFast: Texture | null = null;
  fastReady = false;
  warmFast: (() => void) | null = null;
  moving = false;
  slowAt = 0;
  period = 1000 / 60;
  allowed = false;
  motionSharp = false;
  earned = 0;
  earnFor = EARN;
  native = false;
  configured = false;
  detected: Detected = null;
  failed = new Map<GpuClass, number>();
  changes = 0;
  onLevel: ((level: GpuClass) => void) | null = null;

  configure(phone: boolean, deviceDpr: number, gpu: GpuClass) {
    if (this.configured) return;
    this.configured = true;
    this.phone = phone;
    this.gpu = phone ? 'low' : gpu;
    this.tier = this.gpu === 'low' ? 'light' : 'full';
    const chosen = phone ? { sharp: 1, fast: 0.7, dof: false, ao: false, grain: true } : settings[this.gpu];
    this.dof = chosen.dof;
    this.ao = chosen.ao;
    this.grain = chosen.grain;
    this.nativeDpr = Math.min(Math.max(deviceDpr, 1), NATIVE);
    this.sharpDpr = Math.min(chosen.sharp, this.nativeDpr);
    this.stillScale = this.sharpDpr / this.nativeDpr;
    this.late = LATE ?? true;
    this.liveScale = this.late && !this.switched ? 1 : this.stillScale;
    this.fastScale = Math.min(1, (FAST ?? chosen.fast) / this.sharpDpr);
    this.native = this.stillScale < 0.999;
  }

  get ceiling(): GpuClass {
    return this.phone ? 'low' : this.tier === 'full' ? 'high' : 'low';
  }

  step(direction: 1 | -1): GpuClass | null {
    const index = ORDER.indexOf(this.gpu) + direction;
    if (index < 0 || index > ORDER.indexOf(this.ceiling)) return null;
    const next = ORDER[index];
    if (direction > 0 && (this.failed.get(next) ?? 0) >= 2) return null;
    return next;
  }

  setLevel(level: GpuClass) {
    if (level === this.gpu || this.phone) return;
    if (ORDER.indexOf(level) < ORDER.indexOf(this.gpu)) this.failed.set(this.gpu, (this.failed.get(this.gpu) ?? 0) + 1);
    const chosen = settings[level];
    this.gpu = level;
    this.dof = chosen.dof;
    this.ao = chosen.ao;
    this.grain = chosen.grain;
    if (this.native) {
      this.sharpDpr = Math.min(chosen.sharp, this.nativeDpr * 0.9);
      this.stillScale = this.sharpDpr / this.nativeDpr;
      if (this.switched) this.liveScale = this.stillScale;
    }
    this.fastScale = Math.min(1, (FAST ?? chosen.fast) / this.sharpDpr);
    this.changes += 1;
    this.composer?.setSize(0, 0);
    this.fastComposer?.setSize(0, 0);
    this.onLevel?.(level);
  }

  get canvasDpr() {
    return this.liveScale < 0.999 ? this.nativeDpr : this.sharpDpr;
  }

  liveSize(gl: WebGLRenderer, out: Vector2) {
    gl.getSize(out);
    return out.set(Math.max(1, Math.round(out.x * this.sharpDpr)), Math.max(1, Math.round(out.y * this.sharpDpr)));
  }

  get hasFast() {
    return this.fastScale < 0.99;
  }

  refresh(interval: number) {
    let best = this.period;
    for (const rate of REFRESH) if (Math.abs(Math.log(rate / interval)) < Math.abs(Math.log(best / interval))) best = rate;
    if (best < this.period * 0.8) this.period = best;
  }

  earn(gpu: number | null, frame: number) {
    if (this.allowed) return;
    const fits = frame <= this.period * 1.25 && (gpu === null || gpu / (this.fastScale * this.fastScale) <= this.period * COMFORT);
    this.earned = fits ? this.earned + Math.min(frame, 50) : 0;
    if (this.earned < this.earnFor) return;
    this.allowed = true;
    this.earned = 0;
  }

  miss() {
    if (!this.motionSharp && !this.allowed) return;
    this.motionSharp = false;
    this.allowed = false;
    this.earned = 0;
    this.earnFor = Math.min(this.earnFor * 2, EARN_LIMIT);
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
