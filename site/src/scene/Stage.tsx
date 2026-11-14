import './early';
import './lighting';
import { Canvas, useThree } from '@react-three/fiber';
import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { setConsoleFunction } from 'three';
import { suspend } from 'suspend-react';
import { loadManifest } from '@/story/demo';
import { pointer, runtime } from '@/story/state';
import { Post } from './Post';
import { Governor } from './Governor';
import { gpuClass, quality } from './quality';
import { layoutFor } from './shots';
import { ScreenTextures } from './screen/useScreen';
import { Surroundings, World } from './World';

const Controls = lazy(() => import('./Controls'));

performance.mark('scooch:stage');

setConsoleFunction((type: string, message: unknown, ...params: unknown[]) => {
  if (type === 'warn' && String(message).startsWith('THREE.Clock:')) return;
  const write = (console as unknown as Record<string, (...input: unknown[]) => void>)[type] ?? console.log;
  write(message, ...params);
});

function Pauser({ host }: { host: React.RefObject<HTMLDivElement | null> }) {
  const setFrameloop = useThree((state) => state.setFrameloop);
  useEffect(() => {
    let visible = true;
    const sync = () => setFrameloop(runtime.compiled && visible && !document.hidden && !runtime.abandoned ? 'always' : 'never');
    runtime.resume = sync;
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
      runtime.resume = null;
      window.removeEventListener('pointermove', move);
    };
  }, [host, setFrameloop]);
  return null;
}

function Early() {
  const manifest = suspend(loadManifest, ['scooch-manifest']);
  return (
    <>
      <Surroundings />
      <ScreenTextures manifest={manifest} />
    </>
  );
}

function loadAreaLights() {
  return import('three/examples/jsm/lights/RectAreaLightUniformsLib.js').then(({ RectAreaLightUniformsLib }) => RectAreaLightUniformsLib.init());
}

function Scene({ phone, onReady }: { phone: boolean; onReady: () => void }) {
  const manifest = suspend(loadManifest, ['scooch-manifest']);
  if (quality.tier === 'full') suspend(loadAreaLights, ['scooch-area-lights']);
  return (
    <>
      <World manifest={manifest} onReady={onReady} />
      <Post phone={phone} />
    </>
  );
}

function useShown() {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (runtime.still >= 0) return;
    const timer = window.setInterval(() => {
      if (!runtime.shown) return;
      window.clearInterval(timer);
      setShown(true);
    }, 250);
    return () => window.clearInterval(timer);
  }, []);
  return shown;
}

export default function Stage({ onReady }: { onReady: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const shown = useShown();
  const phone = layoutFor(window.innerWidth, window.innerHeight) === 'phone';
  const deviceDpr = window.devicePixelRatio || 1;
  quality.configure(phone, phone ? Math.min(deviceDpr, 1.5) : deviceDpr, gpuClass().gpu);
  const dpr = runtime.still >= 0 ? Math.min(deviceDpr, 2) : Math.min(quality.sharpDpr, deviceDpr);

  return (
    <div ref={host} className="absolute inset-0">
      <Canvas
        shadows="percentage"
        frameloop="never"
        dpr={dpr}
        gl={{ antialias: false, powerPreference: 'high-performance', stencil: false, alpha: false }}
        camera={{ fov: 30, near: 0.03, far: 9, position: [0, 1.3, 1.6] }}
        onCreated={({ gl }) => {
          gl.debug.checkShaderErrors = import.meta.env.DEV;
        }}
        aria-hidden
        tabIndex={-1}
      >
        <Suspense fallback={null}>
          <Early />
        </Suspense>
        <Suspense fallback={null}>
          <Scene phone={phone} onReady={onReady} />
        </Suspense>
        <Pauser host={host} />
        {runtime.still < 0 && <Governor />}
      </Canvas>
      {shown ? (
        <Suspense fallback={null}>
          <Controls />
        </Suspense>
      ) : null}
    </div>
  );
}
