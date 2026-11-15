import { Bloom, DepthOfField, EffectComposer, N8AO, Noise, SMAA, ToneMapping, Vignette } from '@react-three/postprocessing';
import { useFrame, useThree } from '@react-three/fiber';
import { BlendFunction, CopyPass, ToneMappingMode } from 'postprocessing';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ConstantAlphaFactor, CustomBlending, HalfFloatType, NoBlending, OneMinusConstantAlphaFactor, SRGBColorSpace, Vector2, WebGLRenderTarget, type Texture } from 'three';
import type { DepthOfFieldEffect, EffectComposer as EffectComposerType, NoiseEffect, Pass } from 'postprocessing';
import { holdQuiet, quiet } from '@/lib/idle';
import { release as releaseLoader } from '@/lib/load';
import { changed, runtime, story, wallClock } from '@/story/state';
import { beats, TOTAL } from '@/story/timeline';
import { quality, type Toggle } from './quality';
import { allocate, freeze, inView, nativeAllowed, nativeOff, screenBox, still, thaw } from './still';
import { budget, grant, release } from './budget';
import { animating, hooks, live, things } from './touch';
import { precompile } from './warm';

type Target = { isWebGLMultipleRenderTargets?: boolean; textures?: Texture[] & { mipmaps?: unknown[] } };

const SHARP_AFTER = 250;
const COALESCE = 2000;
const YIELD = 300;
const BUILDING = new Set(['capture', 'screen', 'bands', 'post']);

function loading() {
  return !(runtime.props && runtime.gallery && runtime.painted && runtime.inside);
}

const arrived = { mark: '' };

function arrival() {
  const mark = `${runtime.props}${runtime.gallery}${runtime.painted}${runtime.inside}`;
  if (mark === arrived.mark) return;
  arrived.mark = mark;
  still.arrived = true;
  changed();
}
const LOST_FOR = 4000;

function giveUp() {
  const at = story.progress * TOTAL;
  const found = beats.findIndex((beat) => at < beat.end);
  const index = found < 0 ? beats.length - 1 : found;
  runtime.abandoned = true;
  document.documentElement.classList.add('stills-mode');
  releaseLoader(false);
  requestAnimationFrame(() => document.querySelectorAll('.stills li')[index]?.scrollIntoView({ block: 'start' }));
}
const BLEND = 150;
const STILL_AFTER = 400;
const JITTER = [
  [0, 0],
  [0.5, 0.25],
  [0.25, 0.75],
  [0.75, 0.5],
];

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
  }, []);
  const noise = useCallback((effect: NoiseEffect | null) => {
    if (effect) effect.blendMode.opacity.value = quality.grainOpacity();
  }, []);
  return (
    <>
      {full && quality.ao && <N8AO ref={ao} halfRes quality="performance" aoRadius={0.18} distanceFalloff={0.5} intensity={1.4} />}
      {full && quality.dof && <DepthOfField ref={dof} focusDistance={1} focusRange={0.2} bokehScale={0} resolutionScale={0.5} />}
      <Bloom mipmapBlur levels={full ? 5 : 4} luminanceThreshold={1.3} luminanceSmoothing={0.35} intensity={0.6} radius={0.7} />
      <ToneMapping mode={ToneMappingMode.ACES_FILMIC} />
      <SMAA />
      <Vignette offset={0.3} darkness={0.62} />
      <Noise ref={noise} premultiply blendFunction={BlendFunction.SOFT_LIGHT} opacity={quality.grainOpacity()} />
    </>
  );
}

const motion = { until: 0, was: false };
const corner = { scale: '' };

type Sized = EffectComposerType & { depthRenderTarget?: { setSize: (width: number, height: number) => void } | null };

function sizeTo(composer: EffectComposerType, width: number, height: number) {
  composer.inputBuffer.setSize(width, height);
  composer.outputBuffer.setSize(width, height);
  (composer as Sized).depthRenderTarget?.setSize(width, height);
  for (const pass of composer.passes) pass.setSize(width, height);
}
const captured = new WeakSet<CopyPass>();

function grab(pass: Pass, run: () => void) {
  let texture: Texture | null = null;
  const render = pass.render;
  const toScreen = pass.renderToScreen;
  pass.renderToScreen = false;
  pass.render = function grabbed(this: Pass, renderer, inputBuffer, outputBuffer, delta, stencil) {
    render.call(this, renderer, inputBuffer, outputBuffer, delta, stencil);
    texture = renderer.getRenderTarget()?.texture ?? null;
  };
  try {
    run();
  } finally {
    pass.render = render;
    pass.renderToScreen = toScreen;
  }
  return texture as Texture | null;
}

function kept(pass: Pass, run: () => void) {
  const render = pass.render;
  pass.render = () => undefined;
  try {
    run();
  } finally {
    pass.render = render;
  }
}

function capture(pass: CopyPass) {
  if (captured.has(pass)) return;
  captured.add(pass);
  const render = pass.render.bind(pass);
  pass.render = (renderer, inputBuffer, outputBuffer, delta, stencil) => {
    if (inputBuffer) quality.lastFast = inputBuffer.texture;
    render(renderer, inputBuffer, outputBuffer, delta, stencil);
  };
}

export function Post({ phone }: { phone: boolean }) {
  const gl = useThree((state) => state.gl);
  const scene = useThree((state) => state.scene);
  const camera = useThree((state) => state.camera);
  const fastRender = useRef<((delta?: number) => void) | null>(null);
  const sharpRender = useRef<((delta?: number) => void) | null>(null);
  const nativeRender = useRef<((delta?: number) => void) | null>(null);
  const [native, setNative] = useState(false);
  const setDpr = useThree((state) => state.setDpr);
  const switchWanted = useRef(false);
  const warming = useRef(0);
  const bufferSize = useRef<[number, number] | null>(null);
  const viewSize = useMemo(() => new Vector2(), []);
  const measured = useMemo(() => ({ css: new Vector2(), buffer: new Vector2() }), []);
  const turnNative = useCallback(() => {
    switchWanted.current = false;
    const context = gl.getContext();
    if (context.isContextLost() || still.off) return;
    setDpr(quality.nativeDpr);
    gl.getSize(measured.css);
    gl.getDrawingBufferSize(measured.buffer);
    const wide = Math.floor(measured.css.x * quality.nativeDpr);
    const tall = Math.floor(measured.css.y * quality.nativeDpr);
    if (context.isContextLost() || Math.abs(measured.buffer.x - wide) > 1 || Math.abs(measured.buffer.y - tall) > 1) {
      setDpr(quality.sharpDpr);
      nativeOff();
      release();
      return;
    }
    quality.switched = true;
    quality.liveScale = quality.stillScale;
    quality.onNative?.();
    setNative(true);
  }, [gl, setDpr, measured]);
  const full = !phone && quality.tier === 'full';
  const fast = quality.hasFast && runtime.still < 0;
  const copy = useMemo(() => new CopyPass(), []);
  const overlay = useMemo(() => {
    const pass = new CopyPass();
    pass.renderToScreen = true;
    pass.fullscreenMaterial.transparent = true;
    pass.fullscreenMaterial.blending = CustomBlending;
    pass.fullscreenMaterial.blendSrc = ConstantAlphaFactor;
    pass.fullscreenMaterial.blendDst = OneMinusConstantAlphaFactor;
    pass.fullscreenMaterial.depthTest = false;
    pass.fullscreenMaterial.depthWrite = false;
    return pass;
  }, []);
  const present = useMemo(() => {
    const pass = new CopyPass();
    pass.renderToScreen = true;
    pass.fullscreenMaterial.blending = NoBlending;
    pass.fullscreenMaterial.depthTest = false;
    pass.fullscreenMaterial.depthWrite = false;
    return pass;
  }, []);
  const totals = useMemo(() => [0, 1].map(() => new WebGLRenderTarget(1, 1, { depthBuffer: false, colorSpace: SRGBColorSpace })), []);
  const accumulate = useMemo(() => {
    const pass = new CopyPass(totals[0], false) as CopyPass & { renderTarget: WebGLRenderTarget };
    pass.fullscreenMaterial.transparent = true;
    pass.fullscreenMaterial.blending = CustomBlending;
    pass.fullscreenMaterial.blendSrc = ConstantAlphaFactor;
    pass.fullscreenMaterial.blendDst = OneMinusConstantAlphaFactor;
    pass.fullscreenMaterial.depthTest = false;
    pass.fullscreenMaterial.depthWrite = false;
    return pass;
  }, [totals]);
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
    return () => {
      copy.dispose();
      overlay.dispose();
      present.dispose();
      accumulate.dispose();
      for (const target of totals) target.dispose();
    };
  }, [copy, overlay, present, accumulate, totals]);

  useEffect(() => holdQuiet(() => loading() && (BUILDING.has(still.phase) || performance.now() - still.inputAt < YIELD)), []);

  useEffect(() => {
    const input = () => {
      still.inputAt = performance.now();
    };
    const events = ['pointermove', 'pointerdown', 'keydown', 'wheel', 'touchstart', 'scroll'] as const;
    for (const name of events) window.addEventListener(name, input, { passive: true });
    let watchdog = 0;
    const lost = () => {
      nativeOff();
      switchWanted.current = false;
      release();
      window.clearTimeout(watchdog);
      watchdog = window.setTimeout(() => {
        if (gl.getContext().isContextLost()) giveUp();
      }, LOST_FOR);
    };
    const restored = () => window.clearTimeout(watchdog);
    gl.domElement.addEventListener('webglcontextlost', lost);
    gl.domElement.addEventListener('webglcontextrestored', restored);
    return () => {
      for (const name of events) window.removeEventListener(name, input);
      gl.domElement.removeEventListener('webglcontextlost', lost);
      gl.domElement.removeEventListener('webglcontextrestored', restored);
      window.clearTimeout(watchdog);
    };
  }, [gl]);

  useEffect(() => {
    if (runtime.still >= 0 || quality.stillScale >= 0.999) return;
    let stopped = false;
    let timer = 0;
    const wait = () => {
      if (stopped) return;
      const intro = performance.getEntriesByName('scooch:intro')[0]?.startTime;
      if (intro === undefined || performance.now() - intro < 1000) {
        timer = window.setTimeout(wait, 500);
        return;
      }
      if (!nativeAllowed(gl)) {
        still.off = true;
        release();
        return;
      }
      still.bands = quality.gpu === 'low' && !quality.phone ? 6 : 3;
      if (quality.liveScale < 0.999) setNative(true);
      else switchWanted.current = true;
    };
    timer = window.setTimeout(wait, 500);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [gl]);

  const sharpComposer = useCallback(
    (value: EffectComposerType | null) => {
      quality.composer = value;
      if (!value) return;
      value.setSize = () => {
        quality.liveSize(gl, size);
        sizeTo(value, size.x, size.y);
      };
      const render = value.render.bind(value);
      sharpRender.current = render;
      value.render = (delta?: number) => {
        if (quality.chain !== 'sharp' || quality.hold) return;
        render(delta);
        const t = (performance.now() - quality.blendAt) / BLEND;
        const last = quality.lastFast;
        if (t >= 1 || !last) return;
        overlay.fullscreenMaterial.blendAlpha = 1 - t;
        const clear = gl.autoClear;
        gl.autoClear = false;
        overlay.render(gl, { texture: last } as WebGLRenderTarget, null as never);
        gl.autoClear = clear;
      };
    },
    [gl, overlay, size],
  );

  const fastComposer = useCallback(
    (value: EffectComposerType | null) => {
      quality.fastComposer = value;
      if (!value) return;
      const render = value.render.bind(value);
      fastRender.current = render;
      quality.warmFast = () => {
        if (quality.fastReady || !value.passes.length) return;
        const last = value.passes[value.passes.length - 1];
        last.renderToScreen = false;
        render(0);
        last.renderToScreen = true;
        quality.fastReady = true;
      };
      value.render = (delta?: number) => {
        if (quality.chain !== 'fast' || quality.hold) return;
        const last = value.passes[value.passes.length - 1];
        if (last instanceof CopyPass) capture(last);
        render(delta);
      };
      value.setSize = () => {
        quality.liveSize(gl, size);
        sizeTo(value, Math.max(1, Math.round(size.x * quality.fastScale)), Math.max(1, Math.round(size.y * quality.fastScale)));
      };
    },
    [gl, size],
  );

  const nativeComposer = useCallback(
    (value: EffectComposerType | null) => {
      quality.nativeComposer = value;
      if (!value) return;
      nativeRender.current = value.render.bind(value);
      value.render = () => undefined;
      void (async () => {
        await quiet();
        await precompile(gl, scene, camera, [value, { passes: [present, overlay, accumulate] }]);
        const buffer = gl.getDrawingBufferSize(new Vector2());
        const wide = buffer.x;
        const tall = buffer.y;
        bufferSize.current = [wide, tall];
        for (const target of totals) {
          target.setSize(wide, tall);
          await quiet();
          gl.initRenderTarget(target);
        }
        await quiet();
        hooks.frame?.prepare(gl);
        if (await allocate(gl, value)) warming.current = 3;
        else release();
      })().catch(() => {
        nativeOff();
        release();
      });
    },
    [gl, scene, camera, size, totals, present, overlay, accumulate],
  );

  useEffect(() => {
    if (!fast) return;
    let stopped = false;
    let timer = 0;
    const warm = async () => {
      if (stopped) return;
      if (quality.fastReady) return;
      if (!(runtime.shown && quality.fastComposer && fastRender.current)) {
        timer = window.setTimeout(() => void warm(), 250);
        return;
      }
      await quiet();
      if (stopped) return;
      await precompile(gl, scene, camera, [quality.fastComposer]);
      await quiet();
      if (stopped || !fastRender.current || quality.fastReady) return;
      const last = quality.fastComposer.passes[quality.fastComposer.passes.length - 1];
      if (last) last.renderToScreen = false;
      fastRender.current(0);
      if (last) last.renderToScreen = true;
      quality.fastReady = true;
    };
    timer = window.setTimeout(() => void warm(), 0);
    return () => {
      stopped = true;
      window.clearTimeout(timer);
    };
  }, [fast, gl, scene, camera]);

  const introAt = useRef(0);

  const band = useCallback(() => {
    const composer = quality.nativeComposer;
    if (!composer) return false;
    const target = composer.inputBuffer;
    const rows = Math.ceil(target.height / still.bands);
    const y = still.band * rows;
    const box = still.box;
    if (box) target.scissor.set(box.x, box.y, box.w, box.h);
    else target.scissor.set(0, y, target.width, Math.min(rows, target.height - y));
    target.scissorTest = true;
    const previous = gl.getRenderTarget();
    const clear = gl.autoClear;
    const [jx, jy] = JITTER[still.pass] ?? [0, 0];
    const projection = camera.projectionMatrix.elements;
    const x = projection[8];
    const y0 = projection[9];
    projection[8] += (2 * jx) / target.width;
    projection[9] += (2 * jy) / target.height;
    gl.setRenderTarget(target);
    gl.clear();
    gl.autoClear = false;
    gl.render(scene, camera);
    gl.autoClear = clear;
    gl.setRenderTarget(previous);
    projection[8] = x;
    projection[9] = y0;
    target.scissorTest = false;
    return true;
  }, [gl, scene, camera]);

  const fit = useCallback((target: WebGLRenderTarget) => {
    const input = quality.nativeComposer?.inputBuffer;
    if (!input || (target.width === input.width && target.height === input.height)) return true;
    target.setSize(input.width, input.height);
    return false;
  }, []);

  const warm = useCallback(() => {
    const composer = quality.nativeComposer;
    const render = nativeRender.current;
    if (!composer || !render || still.off) {
      warming.current = 0;
      return;
    }
    const step = warming.current;
    warming.current -= 1;
    if (step === 3) {
      const target = composer.inputBuffer;
      target.scissor.set(0, 0, 1, 1);
      target.scissorTest = true;
      const previous = gl.getRenderTarget();
      const clear = gl.autoClear;
      gl.setRenderTarget(target);
      gl.clear();
      gl.autoClear = false;
      gl.render(scene, camera);
      gl.autoClear = clear;
      gl.setRenderTarget(previous);
      target.scissorTest = false;
      return;
    }
    if (step === 2) {
      grab(composer.passes[composer.passes.length - 1], () => kept(composer.passes[0], () => render(0)));
      return;
    }
    runtime.muted = true;
    hooks.frame?.renderRows(gl, story.place, 0, 1);
    runtime.muted = false;
    const into = totals[1 - still.target];
    fit(into);
    accumulate.renderTarget = into;
    accumulate.fullscreenMaterial.blendAlpha = 1;
    into.scissor.set(0, 0, 1, 1);
    into.scissorTest = true;
    const clear = gl.autoClear;
    gl.autoClear = false;
    accumulate.render(gl, composer.outputBuffer, null as never);
    gl.autoClear = clear;
    into.scissorTest = false;
    still.ready = true;
  }, [gl, scene, camera, totals, accumulate, fit]);

  const place = useCallback(
    (full: boolean) => {
      if (quality.liveScale >= 0.999) return;
      gl.getSize(viewSize);
      const r = full ? 1 : quality.liveScale;
      gl.setViewport(0, viewSize.y * (1 - r), viewSize.x * r, viewSize.y * r);
      const scale = full ? 'none' : `scale(${1 / r})`;
      if (corner.scale === scale) return;
      corner.scale = scale;
      gl.domElement.style.transformOrigin = '0 0';
      gl.domElement.style.transform = scale;
    },
    [gl, size],
  );

  const settle = useCallback(
    (now: number, moving: boolean, time: number) => {
      arrival();
      if (!introAt.current) introAt.current = performance.getEntriesByName('scooch:intro')[0]?.startTime ?? 0;
      const calm =
        still.ready &&
        !still.off &&
        !moving &&
        !live.pressing &&
        !live.tape &&
        runtime.settled &&
        !document.hidden &&
        introAt.current > 0 &&
        now - introAt.current > 2500 &&
        quality.chain === 'sharp' &&
        now - quality.blendAt > BLEND &&
        now - still.inputAt > STILL_AFTER &&
        (still.showing || still.phase === 'still' || still.refresh || loading() || now - runtime.changedAt > STILL_AFTER);
      if (still.phase !== 'live' && !calm) {
        still.phase = 'live';
        still.showing = false;
        still.full = false;
        still.box = null;
        still.refresh = false;
        still.from = null;
        quality.hold = false;
        thaw();
        return;
      }
      if (still.phase === 'live') {
        if (!calm) return;
        freeze(time);
        still.phase = 'capture';
        still.startedAt = now;
        quality.hold = true;
        return;
      }
      if (still.phase === 'capture') {
        const composer = quality.composer;
        const render = sharpRender.current;
        const last = composer?.passes[composer.passes.length - 1];
        still.live = composer && render && last ? grab(last, () => render(0)) : null;
        still.band = 0;
        still.pass = 0;
        still.row = 0;
        still.tick = false;
        still.stale = false;
        still.phase = still.live ? 'screen' : 'live';
        return;
      }
      if (still.phase === 'screen') {
        const frame = hooks.frame;
        if (!frame) {
          still.phase = 'live';
          return;
        }
        const rows = Math.max(32, Math.ceil(frame.rows / 8));
        runtime.muted = true;
        const done = frame.renderRows(gl, story.place, still.row, rows);
        runtime.muted = false;
        still.row += rows;
        if (!done) return;
        still.full = true;
        still.phase = 'bands';
        return;
      }
      if (still.phase === 'bands') {
        if (!band()) {
          still.phase = 'live';
          return;
        }
        still.band += 1;
        if (still.box || still.band >= still.bands) still.phase = 'post';
        return;
      }
      if (still.phase === 'post') {
        const composer = quality.nativeComposer;
        const render = nativeRender.current;
        if (!composer || !render) {
          still.phase = 'live';
          return;
        }
        const image = grab(composer.passes[composer.passes.length - 1], () => kept(composer.passes[0], () => render(0)));
        if (!image) {
          still.phase = 'live';
          still.showing = false;
          return;
        }
        const into = totals[still.refresh ? 1 - still.target : still.target];
        if (!fit(into)) {
          still.box = null;
          if (still.showing && !still.refresh) {
            still.phase = 'live';
            still.showing = false;
            still.full = false;
            still.refresh = false;
            still.from = null;
            quality.hold = false;
            thaw();
            return;
          }
          still.tick = false;
          still.pass = 0;
          still.band = 0;
          still.phase = 'bands';
          return;
        }
        accumulate.renderTarget = into;
        accumulate.fullscreenMaterial.blendAlpha = 1 / (still.pass + 1);
        const clear = gl.autoClear;
        gl.autoClear = false;
        const box = still.box;
        if (box) {
          into.scissor.set(box.x, box.y, box.w, box.h);
          into.scissorTest = true;
        }
        accumulate.render(gl, { texture: image } as WebGLRenderTarget, null as never);
        into.scissorTest = false;
        gl.autoClear = clear;
        if (!still.refresh) still.image = into.texture;
        if (still.pass === 0 && !still.tick && !still.refresh) {
          still.phase = 'show';
          still.shownAt = now;
          still.from = still.live;
          still.second = Math.floor(wallClock() / 1000);
          return;
        }
        if (still.pass < JITTER.length - 1) {
          still.pass += 1;
          still.band = 0;
          still.phase = 'bands';
          return;
        }
        if (still.refresh) {
          still.from = still.image;
          still.image = into.texture;
          still.shownAt = now;
          still.tick = false;
          still.phase = 'show';
          return;
        }
        still.tick = true;
        still.phase = 'show';
        return;
      }
      if (still.phase === 'still') {
        if (runtime.changedAt > still.builtAt) still.stale = true;
        if (still.pass < JITTER.length - 1) {
          still.pass += 1;
          still.band = 0;
          still.phase = 'bands';
          return;
        }
        const second = Math.floor(wallClock() / 1000);
        if (second !== still.second && inView(things.clock, camera)) {
          const target = quality.nativeComposer?.inputBuffer;
          still.second = second;
          still.box = target ? screenBox(things.clock, camera, target.width, target.height) : null;
          still.band = 0;
          still.pass = 0;
          still.tick = true;
          still.phase = 'bands';
          return;
        }
        if (still.stale && (still.arrived || !budget.busy || now - budget.lastStepAt > 1000) && (!loading() || now - still.builtAt > COALESCE)) {
          still.stale = false;
          still.arrived = false;
          still.rebuilds += 1;
          still.startedAt = now;
          still.refresh = true;
          still.row = 0;
          still.band = 0;
          still.pass = 0;
          still.phase = 'screen';
        }
      }
    },
    [band, camera, accumulate, gl, totals, fit],
  );

  const show = useCallback(
    (now: number) => {
      if (!still.image) return;
      present.render(gl, { texture: still.image } as WebGLRenderTarget, null as never);
      const t = still.tick ? 1 : (now - still.shownAt) / BLEND;
      if (t < 1 && still.from) {
        overlay.fullscreenMaterial.blendAlpha = 1 - t;
        const clear = gl.autoClear;
        gl.autoClear = false;
        overlay.render(gl, { texture: still.from } as WebGLRenderTarget, null as never);
        gl.autoClear = clear;
        return;
      }
      if (still.refresh) {
        still.target = 1 - still.target;
        still.refresh = false;
      }
      if (!still.tick) still.builtAt = still.startedAt;
      still.from = null;
      still.phase = 'still';
      still.tick = false;
      still.box = null;
    },
    [gl, overlay, present],
  );

  useFrame((state, delta) => {
    const now = performance.now();
    if (live.pressing || animating(now)) motion.until = Math.max(motion.until, now + SHARP_AFTER);
    const moving = now < motion.until;
    const before = motion.was;
    if (moving && !before) quality.motionSharp = quality.allowed;
    motion.was = moving;
    quality.moving = moving;
    if (before && moving && quality.motionSharp && quality.chain === 'sharp' && now - quality.switchAt > 100 && delta * 1000 > quality.period * 1.5) quality.miss();
    const want = fast && runtime.shown && quality.fastReady && moving && !quality.motionSharp ? 'fast' : 'sharp';
    if (want !== quality.chain) {
      if (want === 'sharp') quality.blendAt = now;
      quality.switchAt = now;
      quality.chain = want;
    }
    const known = bufferSize.current;
    if (known && still.ready) {
      gl.getDrawingBufferSize(measured.buffer);
      if (measured.buffer.x !== known[0] || measured.buffer.y !== known[1]) {
        bufferSize.current = [measured.buffer.x, measured.buffer.y];
        for (const target of totals) target.setSize(measured.buffer.x, measured.buffer.y);
        if (still.phase !== 'live') still.inputAt = now;
      }
    }
    if (switchWanted.current && !moving && !live.pressing && still.phase === 'live') turnNative();
    else if (warming.current > 0 && !moving && still.phase === 'live') warm();
    settle(now, moving, state.clock.elapsedTime - runtime.timeShift);
    grant(still.phase === 'still' && still.pass >= JITTER.length - 1 && !still.tick && !still.refresh);
    if (still.phase === 'show') still.showing = true;
    place(still.showing);
    if (still.phase === 'show') show(now);
  });

  return (
    <>
      <EffectComposer ref={sharpComposer} multisampling={0} frameBufferType={HalfFloatType} autoClear={false}>
        <Effects full={full} />
      </EffectComposer>
      {fast ? (
        <EffectComposer ref={fastComposer} multisampling={0} frameBufferType={HalfFloatType} autoClear={false}>
          <Effects full={full} />
          {full && quality.ao ? null : <primitive object={copy} />}
        </EffectComposer>
      ) : null}
      {native ? (
        <EffectComposer ref={nativeComposer} multisampling={0} frameBufferType={HalfFloatType} autoClear={false}>
          <Effects full={full} />
        </EffectComposer>
      ) : null}
    </>
  );
}
