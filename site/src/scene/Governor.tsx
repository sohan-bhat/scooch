import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { quality, type Chain } from './quality';

const SIZE = 512;
const WINDOW = 1500;
const SETTLE = 100;

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

  sorted = new Float64Array(SIZE);

  quarter(now: number, window: number) {
    let n = 0;
    for (let i = 1; i <= this.count; i++) {
      const at = (this.head - i + SIZE) % SIZE;
      if (now - this.times[at] > window) break;
      this.sorted[n] = this.values[at];
      n += 1;
    }
    if (n < 30) return 0;
    const list = this.sorted.subarray(0, n).sort();
    return list[Math.floor(n / 4)];
  }
}

type Tag = { chain: Chain; moving: boolean; settled: boolean; frame: number };

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
      tags: new Map<WebGLQuery, Tag>(),
      current: null as WebGLQuery | null,
      frames: new Ring(),
      checked: 0,
      ms: 0,
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
    state.ms = ms;
    if (ms > 0 && ms < 120) state.frames.push(now, ms);
    if (now - state.checked > 1000) {
      state.checked = now;
      const interval = state.frames.quarter(now, WINDOW);
      if (interval > 0) quality.refresh(interval);
    }
    const { context, timer } = state;
    if (!timer) return;
    while (state.pending.length > 0) {
      const query = state.pending[0];
      if (!context.getQueryParameter(query, context.QUERY_RESULT_AVAILABLE)) break;
      const tag = state.tags.get(query);
      if (tag && !context.getParameter(timer.GPU_DISJOINT_EXT)) {
        const gpu = context.getQueryParameter(query, context.QUERY_RESULT) / 1e6;
        if (tag.moving && tag.settled && tag.chain === 'fast') quality.earn(gpu, tag.frame);
        if (tag.moving && tag.settled && tag.chain === 'sharp' && quality.motionSharp && gpu > quality.period) quality.miss();
      }
      state.pending.shift();
      state.pool.push(query);
    }
    if (state.pending.length < 6) {
      state.current = state.pool.pop() ?? context.createQuery();
      if (state.current) context.beginQuery(timer.TIME_ELAPSED_EXT, state.current);
    }
  });

  useFrame(() => {
    const settled = performance.now() - quality.switchAt > SETTLE;
    const { context, timer } = state;
    if (!timer) {
      if (quality.moving && quality.chain === 'fast' && settled) quality.earn(null, state.ms);
      return;
    }
    const query = state.current;
    if (!query) return;
    context.endQuery(timer.TIME_ELAPSED_EXT);
    const tag = state.tags.get(query);
    if (tag) {
      tag.chain = quality.chain;
      tag.moving = quality.moving;
      tag.settled = settled;
      tag.frame = state.ms;
    } else state.tags.set(query, { chain: quality.chain, moving: quality.moving, settled, frame: state.ms });
    state.pending.push(query);
    state.current = null;
  }, 2);

  return null;
}
