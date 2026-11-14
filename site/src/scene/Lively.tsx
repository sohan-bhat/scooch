import { useEffect } from 'react';
import { draw, held, step } from './drag';
import { Live } from './Live';
import { play } from './sound';
import { hooks } from './touch';

export default function Lively() {
  useEffect(() => {
    hooks.drawHeld = (gl, delta) => {
      if (!held.active) return false;
      step(delta);
      return draw(gl);
    };
    return () => {
      hooks.drawHeld = () => false;
    };
  }, []);
  return <Live onAction={play} />;
}
