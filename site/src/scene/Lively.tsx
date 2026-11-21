import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { draw, held, step } from './drag';
import { Live } from './Live';
import { installPour, makeStream, pourFrame } from './pour';
import { play } from './sound';
import { hooks } from './touch';

function Pour() {
  const camera = useThree((state) => state.camera);
  const gl = useThree((state) => state.gl);
  const group = useMemo(() => makeStream(), []);
  useEffect(() => installPour(camera), [camera]);
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
