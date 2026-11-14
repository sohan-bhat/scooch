import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { runtime } from '@/story/state';
import { quality } from './quality';

const SIZE = 512;
const WINDOW = 1500;
const GPU_BUDGET = 10.5;
const FRAME_BUDGET = 17.5;
const REST = 1000;
const SETTLE = 600;

type Timer = {
  TIME_ELAPSED_EXT: number;
  GPU_DISJOINT_EXT: number;
};

class Ring {
  times = new Float64Array(SIZE);
  values = new Float64Array(SIZE);
  head = 0;
  count = 0;

  push(time: number, value: number) {
    this.times[this.head] = time;
    this.values[this.head] = value;
    this.head = (this.head + 1) % SIZE;
    this.count = Math.min(this.count + 1, SIZE);
  }

  clear() {
    this.count = 0;
  }

  result = { mean: 0, max: 0, n: 0, steady: 0 };

  stats(now: number, window: number) {
    let sum = 0;
    let max = 0;
    let next = 0;
    let n = 0;
    for (let i = 1; i <= this.count; i++) {
      const at = (this.head - i + SIZE) % SIZE;
      if (now - this.times[at] > window) break;
      const value = this.values[at];
      sum += value;
      if (value > max) {
        next = max;
        max = value;
      } else if (value > next) next = value;
      n += 1;
    }
    this.result.mean = n ? sum / n : 0;
    this.result.max = max;
    this.result.n = n;
    this.result.steady = n > 2 ? (sum - max - next) / (n - 2) : this.result.mean;
    return this.result;
  }
}

export function Governor() {
  const gl = useThree((state) => state.gl);
  const state = useMemo(() => {
    const context = gl.getContext() as WebGL2RenderingContext;
    const timer = context.getExtension('EXT_disjoint_timer_query_webgl2') as Timer | null;
    return {
      context,
      timer,
      pool: [] as WebGLQuery[],
      pending: [] as WebGLQuery[],
      current: null as WebGLQuery | null,
      frames: new Ring(),
      gpu: new Ring(),
      input: performance.now(),
      restSince: performance.now(),
    };
  }, [gl]);

  useEffect(() => {
    const mark = () => {
      state.input = performance.now();
    };
    const events = ['scroll', 'wheel', 'pointerdown', 'keydown', 'touchmove'] as const;
    for (const name of events) window.addEventListener(name, mark, { passive: true });
    return () => {
      for (const name of events) window.removeEventListener(name, mark);
    };
  }, [state]);

  useEffect(
    () => () => {
      for (const query of [...state.pool, ...state.pending]) state.context.deleteQuery(query);
      if (state.current) state.context.deleteQuery(state.current);
    },
    [state],
  );

  useFrame((_, delta) => {
    const now = performance.now();
    const ms = delta * 1000;
    if (ms > 0 && ms < 120) state.frames.push(now, ms);
    const { context, timer } = state;
    if (!timer) return;
    while (state.pending.length > 0) {
      const query = state.pending[0];
      if (!context.getQueryParameter(query, context.QUERY_RESULT_AVAILABLE)) break;
      if (!context.getParameter(timer.GPU_DISJOINT_EXT)) state.gpu.push(now, context.getQueryParameter(query, context.QUERY_RESULT) / 1e6);
      state.pending.shift();
      state.pool.push(query);
    }
    if (state.pending.length < 6) {
      state.current = state.pool.pop() ?? context.createQuery();
      if (state.current) context.beginQuery(timer.TIME_ELAPSED_EXT, state.current);
    }
  });

  useFrame(() => {
    const now = performance.now();
    const { context, timer } = state;
    if (timer && state.current) {
      context.endQuery(timer.TIME_ELAPSED_EXT);
      state.pending.push(state.current);
      state.current = null;
    }
    const warm = runtime.ready && runtime.props && runtime.scenery && runtime.gallery && runtime.painted && runtime.inside;
    const resting = now - state.input > REST && quality.chain === 'sharp' && now - quality.blendAt > 300;
    if (!warm || !resting || !quality.hasFast) {
      state.restSince = now;
      state.frames.clear();
      state.gpu.clear();
      return;
    }
    if (now - state.restSince < SETTLE) return;
    const frames = state.frames.stats(now, WINDOW);
    const gpu = state.gpu.stats(now, WINDOW);
    const enough = timer ? gpu.n >= 45 : frames.n >= 45;
    if (!enough) return;
    const needs = (timer ? gpu.steady > GPU_BUDGET : false) || frames.steady > FRAME_BUDGET;
    quality.needsFast = needs;
    quality.decided = true;
    state.restSince = now;
    state.frames.clear();
    state.gpu.clear();
  }, 2);

  return null;
}
