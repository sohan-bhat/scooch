import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { draw, held, step } from './drag';
import { Live } from './Live';
import { installPour, makeStream, pourFrame, warmStream } from './pour';
import { runtime } from '@/story/state';
import { play } from './sound';
import { hooks } from './touch';

function Pour() {
  const camera = useThree((state) => state.camera);
  const gl = useThree((state) => state.gl);
  const scene = useThree((state) => state.scene);
  const group = useMemo(() => makeStream(), []);
  useEffect(() => installPour(camera), [camera]);
  useEffect(() => {
    let timer = 0;
    const wait = () => {
      if (runtime.ready && runtime.props && runtime.painted) warmStream(gl, camera, group, scene);
      else timer = window.setTimeout(wait, 500);
    };
    timer = window.setTimeout(wait, 1500);
    return () => window.clearTimeout(timer);
  }, [gl, camera, group, scene]);
  useFrame((_, delta) => pourFrame(performance.now(), Math.min(delta, 0.05), gl));
  return <primitive object={group} />;
}

export default function Lively() {
  useEffect(() => {
    hooks.drawHeld = (gl, delta, part) => {
      if (!held.active) return false;
      step(delta);
      return draw(gl, part);
    };
    return () => {
      hooks.drawHeld = () => false;
    };
  }, []);
  return (
    <>
      <Live onAction={play} />
      <Pour />
    </>
  );
}
