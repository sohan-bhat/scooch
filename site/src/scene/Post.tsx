import { Bloom, DepthOfField, EffectComposer, N8AO, Noise, SMAA, ToneMapping, Vignette } from '@react-three/postprocessing';
import { BlendFunction, ToneMappingMode } from 'postprocessing';
import { useCallback } from 'react';
import { HalfFloatType, type Texture } from 'three';
import type { DepthOfFieldEffect, NoiseEffect } from 'postprocessing';
import { GRAIN_OPACITY, quality, type Toggle } from './quality';

type Target = { isWebGLMultipleRenderTargets?: boolean; textures?: Texture[] & { mipmaps?: unknown[] } };

function patchTargets(pass: object) {
  const fix = () => {
    for (const value of Object.values(pass) as Target[]) {
      if (value?.isWebGLMultipleRenderTargets && value.textures && !value.textures.mipmaps) value.textures.mipmaps = [];
    }
  };
  fix();
  const holder = pass as { setSize?: (width: number, height: number) => void; configureHalfResTargets?: () => void };
  for (const key of ['setSize', 'configureHalfResTargets'] as const) {
    const original = holder[key];
    if (typeof original !== 'function') continue;
    holder[key] = function patched(this: unknown, ...args: unknown[]) {
      const result = (original as (...input: unknown[]) => unknown).apply(this, args);
      fix();
      return result;
    } as never;
  }
}

export function Post({ phone }: { phone: boolean }) {
  const full = !phone && quality.tier === 'full';
  const dof = useCallback((effect: DepthOfFieldEffect | null) => quality.attachDof(effect), []);
  const ao = useCallback((pass: Toggle | null) => {
    if (pass) patchTargets(pass);
    quality.ao = pass;
    if (pass) pass.enabled = quality.step.ao;
  }, []);
  const noise = useCallback((effect: NoiseEffect | null) => {
    quality.noise = effect;
    if (effect) effect.blendMode.opacity.value = quality.step.grain ? GRAIN_OPACITY : 0;
  }, []);

  return (
    <EffectComposer multisampling={0} frameBufferType={HalfFloatType}>
      {full && <N8AO ref={ao} halfRes quality="performance" aoRadius={0.18} distanceFalloff={0.5} intensity={1.4} />}
      {full && <DepthOfField ref={dof} focusDistance={1} focusRange={0.2} bokehScale={0} resolutionScale={0.5} />}
      <Bloom mipmapBlur levels={full ? 5 : 4} luminanceThreshold={1.3} luminanceSmoothing={0.35} intensity={0.6} radius={0.7} />
      <ToneMapping mode={ToneMappingMode.ACES_FILMIC} />
      <SMAA />
      <Vignette offset={0.3} darkness={0.62} />
      <Noise ref={noise} premultiply blendFunction={BlendFunction.SOFT_LIGHT} opacity={GRAIN_OPACITY} />
    </EffectComposer>
  );
}
