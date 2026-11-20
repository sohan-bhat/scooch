import { preload, shot, tape, type Voice } from '@/lib/audio';
import { onSound, sound } from '@/lib/sound';
import { live } from './touch';

const DESK = ['switch', 'knob', 'tack', 'bell', 'pour', 'static', 'creak', 'lift', 'down'];
let bell: Voice | null = null;

onSound((on) => {
  if (!on) hush();
});

export function prepare() {
  preload(DESK);
}

export function toggleTape() {
  tape();
}

export function play(name: string) {
  if (!sound.on) return;
  if (name === 'cassette') {
    void shot('tack', { gain: 0.7, rate: 0.82 });
    return;
  }
  if (name === 'clock') {
    bell?.stop();
    bell = null;
    if (live.ringUntil > 0) void shot('bell', { gain: 0.38 }).then((voice) => (bell = voice));
    return;
  }
  if (name === 'lamp') void shot('switch', { gain: 0.7 });
  else if (name === 'power') void shot('switch', { gain: 0.6, rate: 0.72 });
  else if (name === 'channel') {
    void shot('knob', { gain: 0.7 });
    void shot('static', { gain: 0.12 });
  } else if (name === 'stapler') void shot('tack', { gain: 0.75, rate: 1.12 });
  else if (name === 'glass' && live.juice === 4) void shot('pour', { gain: 0.5 });
  else if (name === 'creak') void shot('creak', { gain: 0.25 });
  else if (name === 'lift') void shot('lift', { gain: 0.18 });
  else if (name === 'down') void shot('down', { gain: 0.4 });
}

export function hush() {
  bell?.stop();
  bell = null;
}
