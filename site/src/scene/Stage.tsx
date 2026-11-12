import { PerformanceMonitor } from '@react-three/drei';
import { Canvas, useThree } from '@react-three/fiber';
import { Suspense, useEffect, useRef } from 'react';
import { DefaultLoadingManager, setConsoleFunction } from 'three';
import { suspend } from 'suspend-react';
import { loadManifest } from '@/story/demo';
import { pointer, runtime } from '@/story/state';
import { Post } from './Post';
import { quality } from './quality';
import { layoutFor } from './shots';
import { World } from './World';

setConsoleFunction((type: string, message: unknown, ...params: unknown[]) => {
  if (type === 'warn' && String(message).startsWith('THREE.Clock:')) return;
  const write = (console as unknown as Record<string, (...input: unknown[]) => void>)[type] ?? console.log;
  write(message, ...params);
});

const loading = { value: 0, listener: null as ((value: number) => void) | null };
DefaultLoadingManager.onProgress = (_url, loaded, total) => {
  loading.value = total > 0 ? loaded / total : 0;
  loading.listener?.(loading.value);
};
DefaultLoadingManager.onLoad = () => {
  loading.value = 1;
  loading.listener?.(1);
};

function Progress({ onProgress }: { onProgress: (value: number) => void }) {
  useEffect(() => {
    loading.listener = onProgress;
    onProgress(loading.value);
    return () => {
      loading.listener = null;
    };
  }, [onProgress]);
  return null;
}

function Pauser({ host }: { host: React.RefObject<HTMLDivElement | null> }) {
  const setFrameloop = useThree((state) => state.setFrameloop);
  const setDpr = useThree((state) => state.setDpr);
  useEffect(() => {
    quality.setDpr = setDpr;
    let visible = true;
    const sync = () => setFrameloop(visible && !document.hidden ? 'always' : 'never');
    const observer = new IntersectionObserver(([entry]) => {
      visible = entry.isIntersecting;
      sync();
    });
    if (host.current) observer.observe(host.current);
    document.addEventListener('visibilitychange', sync);
    const fine = window.matchMedia('(pointer: fine)').matches;
    const move = (event: PointerEvent) => {
      pointer.x = (event.clientX / window.innerWidth) * 2 - 1;
      pointer.y = (event.clientY / window.innerHeight) * 2 - 1;
    };
    if (fine && runtime.still < 0) window.addEventListener('pointermove', move, { passive: true });
    return () => {
      observer.disconnect();
      document.removeEventListener('visibilitychange', sync);
      window.removeEventListener('pointermove', move);
    };
  }, [host, setFrameloop, setDpr]);
  return null;
}

function Scene({ phone, onReady }: { phone: boolean; onReady: () => void }) {
  const manifest = suspend(loadManifest, ['scooch-manifest']);
  return (
    <>
      <World manifest={manifest} onReady={onReady} />
      <Post phone={phone} />
    </>
  );
}

export default function Stage({ onProgress, onReady }: { onProgress: (value: number) => void; onReady: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const phone = layoutFor(window.innerWidth, window.innerHeight) === 'phone';
  const deviceDpr = window.devicePixelRatio || 1;
  quality.configure(phone, phone ? Math.min(deviceDpr, 1.5) : deviceDpr);
  const dpr = Math.min(quality.step.dpr, deviceDpr);

  return (
    <div ref={host} className="absolute inset-0">
      <Canvas
        shadows="percentage"
        dpr={dpr}
        gl={{ antialias: false, powerPreference: 'high-performance', stencil: false, alpha: false }}
        camera={{ fov: 30, near: 0.03, far: 9, position: [0, 1.3, 1.6] }}
        aria-hidden
        tabIndex={-1}
      >
        <Suspense fallback={null}>
          <Scene phone={phone} onReady={onReady} />
        </Suspense>
        <Progress onProgress={onProgress} />
        <Pauser host={host} />
        {runtime.still < 0 && (
          <PerformanceMonitor
            ms={250}
            iterations={8}
            threshold={0.7}
            flipflops={8}
            bounds={(refresh) => (refresh > 90 ? [52, 88] : [50, 58.5])}
            onDecline={() => quality.down()}
            onIncline={() => quality.up()}
          />
        )}
      </Canvas>
    </div>
  );
}
