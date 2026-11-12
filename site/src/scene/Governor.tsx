import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { quality } from './quality';

const SIZE = 512;
const UP_WINDOW = 3000;
const UP_LIMIT = 12;
const DOWN_WINDOW = 500;
const DOWN_LIMIT = 18;
const WARMUP = 1500;
const SETTLE = 700;
const RETRY = 30000;

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

  result = { mean: 0, max: 0, n: 0 };

  stats(now: number, window: number) {
    let sum = 0;
    let max = 0;
    let n = 0;
    for (let i = 1; i <= this.count; i++) {
      const at = (this.head - i + SIZE) % SIZE;
      if (now - this.times[at] > window) break;
      sum += this.values[at];
      max = Math.max(max, this.values[at]);
      n += 1;
    }
    this.result.mean = n ? sum / n : 0;
    this.result.max = max;
    this.result.n = n;
    return this.result;
  }
}

export function Governor({ fade }: { fade: React.RefObject<HTMLCanvasElement | null> }) {
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
      start: performance.now(),
      changed: performance.now(),
      fading: 0,
      failedAt: new Float64Array(16).fill(-Infinity),
      failures: new Uint8Array(16),
    };
  }, [gl]);

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
    if (state.fading > 0) {
      state.fading -= 1;
      if (state.fading === 0 && fade.current) {
        fade.current.style.transition = 'opacity 420ms cubic-bezier(0.22, 1, 0.36, 1)';
        fade.current.style.opacity = '0';
      }
    }
    if (now - state.start < WARMUP || now - state.changed < SETTLE) return;
    const frames = state.frames.stats(now, DOWN_WINDOW);
    const slowFrames = frames.n >= 8 && frames.mean > DOWN_LIMIT;
    const gpuShort = state.gpu.stats(now, DOWN_WINDOW);
    const slow = slowFrames || (gpuShort.n >= 8 && gpuShort.mean > DOWN_LIMIT);
    if (slow && quality.canDown()) {
      state.failedAt[quality.level] = now;
      state.failures[quality.level] += 1;
      change(() => quality.down());
      return;
    }
    if (!timer || !quality.canUp()) return;
    const gpuLong = state.gpu.stats(now, UP_WINDOW);
    const quiet = gpuLong.n >= 90 && gpuLong.max < UP_LIMIT;
    const framesLong = state.frames.stats(now, UP_WINDOW);
    const target = quality.level - 1;
    const allowed = state.failures[target] < 2 && now - state.failedAt[target] > RETRY;
    const roomy = allowed && quiet && framesLong.mean < 17.5 && now - state.changed > UP_WINDOW;
    if (roomy) change(() => quality.up());
  }, 2);

  function change(apply: () => void) {
    const overlay = fade.current;
    const source = gl.domElement;
    if (overlay) {
      overlay.width = source.width;
      overlay.height = source.height;
      overlay.getContext('2d')?.drawImage(source, 0, 0);
      overlay.style.transition = 'none';
      overlay.style.opacity = '1';
      state.fading = 3;
    }
    apply();
    state.changed = performance.now();
    state.frames.clear();
    state.gpu.clear();
  }

  return null;
}
