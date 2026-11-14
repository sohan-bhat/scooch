import { asset } from '@/lib/base';
import { onSound, sound } from '@/lib/sound';
import { live } from './touch';

type Voice = { stop: () => void };
type Options = { gain?: number; rate?: number; repeat?: boolean };

const files: Record<string, string> = {
  switch: 'sounds/switch.m4a',
  knob: 'sounds/knob.m4a',
  tack: 'sounds/tack.m4a',
  tape: 'sounds/tape.m4a',
  bell: 'sounds/bell.m4a',
  pour: 'sounds/pour.m4a',
};

const SHOTS = ['switch', 'knob', 'tack', 'bell', 'pour'];
const LOOP = 8.42;
let context: AudioContext | null = null;
let settable: boolean | null = null;
const buffers = new Map<string, Promise<AudioBuffer | null>>();
const elements = new Map<string, HTMLAudioElement>();
let loop: Voice | null = null;
let bell: Voice | null = null;

onSound((on) => {
  if (!on) hush();
  else {
    warm();
    if (live.tape && context) play('cassette');
  }
});

function volumeWorks() {
  if (settable === null) {
    const probe = new Audio();
    probe.volume = 0.5;
    settable = Math.abs(probe.volume - 0.5) < 0.01;
  }
  return settable;
}

function element(name: string) {
  let base = elements.get(name);
  if (!base) {
    base = new Audio(asset(files[name]));
    base.preload = 'auto';
    elements.set(name, base);
  }
  return base;
}

function warm() {
  if (!sound.on || !volumeWorks() || elements.size) return;
  for (const name of SHOTS) element(name);
  const primer = element('switch');
  primer.muted = true;
  primer.play().then(
    () => {
      primer.pause();
      primer.currentTime = 0;
      primer.muted = false;
    },
    () => {
      primer.muted = false;
    },
  );
}

function audio() {
  context ??= new AudioContext();
  if (context.state === 'suspended') void context.resume();
  return context;
}

function buffer(name: string, target: AudioContext) {
  let pending = buffers.get(name);
  if (!pending) {
    pending = fetch(asset(files[name]))
      .then((response) => (response.ok ? response.arrayBuffer() : Promise.reject(new Error(String(response.status)))))
      .then((bytes) => target.decodeAudioData(bytes))
      .catch(() => null);
    buffers.set(name, pending);
  }
  return pending;
}

function lead(data: AudioBuffer) {
  const samples = data.getChannelData(0);
  for (let i = 0; i < samples.length; i++) if (Math.abs(samples[i]) > 0.004) return i / data.sampleRate;
  return 0;
}

async function start(name: string, { gain = 0.6, rate = 1, repeat = false }: Options = {}): Promise<Voice | null> {
  const target = audio();
  const data = await buffer(name, target);
  if (!data || !sound.on) return null;
  const source = target.createBufferSource();
  const level = target.createGain();
  level.gain.value = gain;
  source.buffer = data;
  source.playbackRate.value = rate;
  source.connect(level).connect(target.destination);
  if (repeat) {
    const from = lead(data);
    source.loop = true;
    source.loopStart = from;
    source.loopEnd = Math.min(from + LOOP, data.duration);
    source.start(0, from);
  } else source.start();
  return source;
}

function shot(name: string, { gain = 0.6, rate = 1 }: Options = {}): Promise<Voice | null> {
  if (!volumeWorks()) return start(name, { gain, rate });
  const base = element(name);
  const voice = base.paused ? base : (base.cloneNode() as HTMLAudioElement);
  voice.volume = gain;
  voice.preservesPitch = false;
  (voice as HTMLAudioElement & { webkitPreservesPitch?: boolean }).webkitPreservesPitch = false;
  voice.playbackRate = rate;
  voice.currentTime = 0;
  return voice.play().then(
    () => ({ stop: () => voice.pause() }),
    () => null,
  );
}

export function play(name: string) {
  if (!sound.on) return;
  if (name === 'cassette') {
    void shot('tack', { gain: 0.7, rate: 0.82 });
    loop?.stop();
    loop = null;
    if (live.tape) void start('tape', { gain: 0.42, repeat: true }).then((voice) => (loop = live.tape ? voice : (voice?.stop(), null)));
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
  else if (name === 'channel') void shot('knob', { gain: 0.7 });
  else if (name === 'stapler') void shot('tack', { gain: 0.75, rate: 1.12 });
  else if (name === 'glass' && live.juice === 4) void shot('pour', { gain: 0.5 });
}

export function hush() {
  loop?.stop();
  loop = null;
  bell?.stop();
  bell = null;
}

if (typeof window !== 'undefined') {
  const first = () => {
    window.removeEventListener('pointermove', first);
    window.removeEventListener('pointerdown', first);
    warm();
  };
  window.addEventListener('pointermove', first, { passive: true });
  window.addEventListener('pointerdown', first, { passive: true });
}
