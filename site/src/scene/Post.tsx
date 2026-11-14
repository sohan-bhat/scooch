import { Bloom, DepthOfField, EffectComposer, N8AO, Noise, SMAA, ToneMapping, Vignette } from '@react-three/postprocessing';
import { useFrame, useThree } from '@react-three/fiber';
import { BlendFunction, CopyPass, ToneMappingMode } from 'postprocessing';
import { useCallback, useEffect, useMemo, useRef } from 'react';
import { HalfFloatType, NormalBlending, Vector2, type Texture, type WebGLRenderTarget } from 'three';
import type { DepthOfFieldEffect, EffectComposer as EffectComposerType, NoiseEffect } from 'postprocessing';
import { quiet } from '@/lib/idle';
import { runtime } from '@/story/state';
import { quality, type Toggle } from './quality';
import { live } from './touch';
import { precompile } from './warm';

type Target = { isWebGLMultipleRenderTargets?: boolean; textures?: Texture[] & { mipmaps?: unknown[] } };

const SHARP_AFTER = 250;
const BLEND = 150;

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

function Effects({ full }: { full: boolean }) {
  const dof = useCallback((effect: DepthOfFieldEffect | null) => quality.attachDof(effect), []);
  const ao = useCallback((pass: Toggle | null) => {
    if (pass) patchTargets(pass);
    if (pass) pass.enabled = quality.ao;
  }, []);
  const noise = useCallback((effect: NoiseEffect | null) => {
    if (effect) effect.blendMode.opacity.value = quality.grainOpacity();
  }, []);
  return (
    <>
      {full && <N8AO ref={ao} halfRes quality="performance" aoRadius={0.18} distanceFalloff={0.5} intensity={1.4} />}
      {full && <DepthOfField ref={dof} focusDistance={1} focusRange={0.2} bokehScale={0} resolutionScale={0.5} />}
      <Bloom mipmapBlur levels={full ? 5 : 4} luminanceThreshold={1.3} luminanceSmoothing={0.35} intensity={0.6} radius={0.7} />
      <ToneMapping mode={ToneMappingMode.ACES_FILMIC} />
      <SMAA />
      <Vignette offset={0.3} darkness={0.62} />
      <Noise ref={noise} premultiply blendFunction={BlendFunction.SOFT_LIGHT} opacity={quality.grainOpacity()} />
    </>
  );
}

const motion = { until: 0, restAt: 0 };

export function Post({ phone }: { phone: boolean }) {
  const gl = useThree((state) => state.gl);
  const scene = useThree((state) => state.scene);
  const camera = useThree((state) => state.camera);
  const fastRender = useRef<((delta?: number) => void) | null>(null);
  const full = !phone && quality.tier === 'full';
  const fast = quality.hasFast && runtime.still < 0;
  const copy = useMemo(() => new CopyPass(), []);
  const overlay = useMemo(() => {
    const pass = new CopyPass();
    pass.renderToScreen = true;
    pass.fullscreenMaterial.transparent = true;
    pass.fullscreenMaterial.blending = NormalBlending;
    pass.fullscreenMaterial.depthTest = false;
    pass.fullscreenMaterial.depthWrite = false;
    return pass;
  }, []);
  const size = useMemo(() => new Vector2(), []);

  useEffect(() => {
    const moved = () => {
      motion.until = performance.now() + SHARP_AFTER;
    };
    const events = ['scroll', 'wheel', 'touchmove'] as const;
    for (const name of events) window.addEventListener(name, moved, { passive: true });
    return () => {
      for (const name of events) window.removeEventListener(name, moved);
    };
  }, []);

  useEffect(() => {
    const render = copy.render.bind(copy);
    copy.render = (renderer, inputBuffer, outputBuffer, delta, stencil) => {
      if (inputBuffer) quality.lastFast = inputBuffer.texture;
      render(renderer, inputBuffer, outputBuffer, delta, stencil);
    };
    return () => {
      copy.dispose();
      overlay.dispose();
    };
  }, [copy, overlay]);

  const sharpComposer = useCallback(
    (value: EffectComposerType | null) => {
      quality.composer = value;
      if (!value) return;
      const render = value.render.bind(value);
      value.render = (delta?: number) => {
        if (quality.chain !== 'sharp') return;
        render(delta);
        const t = (performance.now() - quality.blendAt) / BLEND;
        const last = quality.lastFast;
        if (t >= 1 || !last) return;
        overlay.fullscreenMaterial.opacity = 1 - t;
        const clear = gl.autoClear;
        gl.autoClear = false;
        overlay.render(gl, { texture: last } as WebGLRenderTarget, null as never);
        gl.autoClear = clear;
      };
    },
    [gl, overlay],
  );

  const fastComposer = useCallback(
    (value: EffectComposerType | null) => {
      quality.fastComposer = value;
      if (!value) return;
      const render = value.render.bind(value);
      fastRender.current = render;
      value.render = (delta?: number) => {
        if (quality.chain !== 'fast') return;
        render(delta);
      };
      value.setSize = () => {
        gl.getDrawingBufferSize(size);
        const width = Math.max(1, Math.round(size.x * quality.fastScale));
        const height = Math.max(1, Math.round(size.y * quality.fastScale));
        value.inputBuffer.setSize(width, height);
        value.outputBuffer.setSize(width, height);
        (value as unknown as { depthRenderTarget?: { setSize: (w: number, h: number) => void } | null }).depthRenderTarget?.setSize(width, height);
        for (const pass of value.passes) pass.setSize(width, height);
      };
    },
    [gl, size],
  );

  useEffect(() => {
    if (!fast) return;
    let stopped = false;
    let timer = 0;
    const warm = async () => {
      if (stopped) return;
      if (!(runtime.props && runtime.painted && runtime.inside && quality.fastComposer && fastRender.current)) {
        timer = window.setTimeout(() => void warm(), 250);
        return;
      }
      await quiet();
      if (stopped) return;
      await precompile(gl, scene, camera, quality.fastComposer);
      await quiet();
      if (stopped || !fastRender.current) return;
      copy.renderToScreen = false;
      fastRender.current(0);
      copy.renderToScreen = true;
      quality.fastReady = true;
    };
    timer = window.setTimeout(() => void warm(), 0);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [fast, gl, scene, camera, copy]);

  useFrame(() => {
    const now = performance.now();
    const moving = now < motion.until || live.pressing;
    if (moving) motion.restAt = now;
    const want = fast && quality.needsFast && runtime.shown && quality.fastReady && (moving || now - motion.restAt < SHARP_AFTER) ? 'fast' : 'sharp';
    if (want === quality.chain) return;
    if (want === 'sharp') quality.blendAt = now;
    quality.chain = want;
  });

  return (
    <>
      <EffectComposer ref={sharpComposer} multisampling={0} frameBufferType={HalfFloatType}>
        <Effects full={full} />
      </EffectComposer>
      {fast ? (
        <EffectComposer ref={fastComposer} multisampling={0} frameBufferType={HalfFloatType}>
          <Effects full={full} />
          <primitive object={copy} />
        </EffectComposer>
      ) : null}
    </>
  );
}
