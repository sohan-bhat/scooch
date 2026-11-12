import type { DepthOfFieldEffect, NoiseEffect } from 'postprocessing';
import type { Vector3 } from 'three';

export type Toggle = { enabled: boolean };

type Step = { dpr: number; dof: boolean; ao: boolean; grain: boolean };

const desktopSteps: Step[] = [
  { dpr: 2, dof: true, ao: true, grain: true },
  { dpr: 1.75, dof: true, ao: true, grain: true },
  { dpr: 1.5, dof: true, ao: true, grain: true },
  { dpr: 1.25, dof: true, ao: true, grain: true },
  { dpr: 1, dof: true, ao: true, grain: true },
  { dpr: 1, dof: true, ao: false, grain: true },
  { dpr: 1, dof: false, ao: false, grain: true },
  { dpr: 1, dof: false, ao: false, grain: false },
  { dpr: 0.85, dof: false, ao: false, grain: false },
  { dpr: 0.7, dof: false, ao: false, grain: false },
];

const lightSteps: Step[] = [
  { dpr: 1.5, dof: false, ao: false, grain: true },
  { dpr: 1.25, dof: false, ao: false, grain: true },
  { dpr: 1, dof: false, ao: false, grain: true },
  { dpr: 1, dof: false, ao: false, grain: false },
  { dpr: 0.85, dof: false, ao: false, grain: false },
  { dpr: 0.7, dof: false, ao: false, grain: false },
];

const phoneSteps: Step[] = [
  { dpr: 1.5, dof: false, ao: false, grain: true },
  { dpr: 1.25, dof: false, ao: false, grain: true },
  { dpr: 1, dof: false, ao: false, grain: true },
  { dpr: 1, dof: false, ao: false, grain: false },
  { dpr: 0.85, dof: false, ao: false, grain: false },
  { dpr: 0.7, dof: false, ao: false, grain: false },
];

export type Tier = 'full' | 'light';

export function gpuTier(): Tier {
  try {
    const canvas = document.createElement('canvas');
    const gl = canvas.getContext('webgl2', { powerPreference: 'high-performance' });
    if (!gl) return 'light';
    const info = gl.getExtension('WEBGL_debug_renderer_info');
    const name = String(gl.getParameter(info ? info.UNMASKED_RENDERER_WEBGL : gl.RENDERER));
    gl.getExtension('WEBGL_lose_context')?.loseContext();
    return /Intel|Mali|Adreno|PowerVR|SwiftShader|llvmpipe|Software/i.test(name) ? 'light' : 'full';
  } catch {
    return 'light';
  }
}

const MAX_BOKEH = 3.2;
const GRAIN = 0.05;

class Quality {
  steps: Step[] = desktopSteps;
  level = 0;
  dof: DepthOfFieldEffect | null = null;
  ao: Toggle | null = null;
  noise: NoiseEffect | null = null;
  setDpr: ((dpr: number) => void) | null = null;
  onChange: (() => void) | null = null;

  tier: Tier = 'full';

  configure(phone: boolean, deviceDpr: number, tier: Tier) {
    this.tier = phone ? 'light' : tier;
    const all = phone ? phoneSteps : tier === 'light' ? lightSteps : desktopSteps;
    const fit = all.filter((step) => step.dpr <= Math.max(deviceDpr, 1) + 0.01);
    this.steps = fit.length > 0 ? fit : [all[all.length - 1]];
    const start = phone ? 1.25 : tier === 'light' ? 0.85 : 1;
    this.level = Math.max(this.steps.findIndex((step) => step.dpr <= start), 0);
  }

  get step() {
    return this.steps[Math.min(this.level, this.steps.length - 1)];
  }

  down() {
    if (this.level >= this.steps.length - 1) return;
    this.level += 1;
    this.apply();
  }

  up() {
    if (this.level <= 0) return;
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
