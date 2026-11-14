import { useEffect } from 'react';
import { draw, held, step } from './drag';
import { Live } from './Live';
import { play } from './sound';
import { hooks } from './touch';

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
  return <Live onAction={play} />;
}
