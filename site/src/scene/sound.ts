import { asset } from '@/lib/base';
import { onSound, sound } from '@/lib/sound';
import { live } from './touch';

const files: Record<string, string> = {
  switch: 'sounds/switch.m4a',
  knob: 'sounds/knob.m4a',
  tack: 'sounds/tack.m4a',
  tape: 'sounds/tape.m4a',
  bell: 'sounds/bell.m4a',
  pour: 'sounds/pour.m4a',
};

const LOOP = 8.42;
let context: AudioContext | null = null;
const buffers = new Map<string, Promise<AudioBuffer | null>>();
let loop: AudioBufferSourceNode | null = null;
let bell: AudioBufferSourceNode | null = null;

onSound((on) => {
  if (!on) hush();
  else if (live.tape && context) play('cassette');
});

function buffer(name: string) {
  const file = files[name];
  if (!file || !context) return Promise.resolve(null);
  let pending = buffers.get(name);
  if (!pending) {
    const audio = context;
    pending = fetch(asset(file))
      .then((response) => (response.ok ? response.arrayBuffer() : Promise.reject(new Error(String(response.status)))))
      .then((bytes) => audio.decodeAudioData(bytes))
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

async function start(name: string, { gain = 0.6, rate = 1, repeat = false } = {}) {
  if (!context) return null;
  const data = await buffer(name);
  if (!data || !sound.on) return null;
  const source = context.createBufferSource();
  const level = context.createGain();
  level.gain.value = gain;
  source.buffer = data;
  source.playbackRate.value = rate;
  if (repeat) {
    const from = lead(data);
    source.loop = true;
    source.loopStart = from;
    source.loopEnd = Math.min(from + LOOP, data.duration);
    source.connect(level).connect(context.destination);
    source.start(0, from);
    return source;
  }
  source.connect(level).connect(context.destination);
  source.start();
  return source;
}

export function play(name: string) {
  if (!sound.on) return;
  context ??= new AudioContext();
  if (context.state === 'suspended') void context.resume();
  if (name === 'cassette') {
    void start('tack', { gain: 0.7, rate: 0.82 });
    loop?.stop();
    loop = null;
    if (live.tape) void start('tape', { gain: 0.42, repeat: true }).then((source) => (loop = live.tape ? source : (source?.stop(), null)));
    return;
  }
  if (name === 'clock') {
    bell?.stop();
    bell = null;
    if (live.ringUntil > 0) void start('bell', { gain: 0.38 }).then((source) => (bell = source));
    return;
  }
  if (name === 'lamp') void start('switch', { gain: 0.7 });
  else if (name === 'power') void start('switch', { gain: 0.6, rate: 0.72 });
  else if (name === 'channel') void start('knob', { gain: 0.7 });
  else if (name === 'stapler') void start('tack', { gain: 0.75, rate: 1.12 });
  else if (name === 'glass' && live.juice === 4) void start('pour', { gain: 0.5 });
}

export function hush() {
  loop?.stop();
  loop = null;
  bell?.stop();
  bell = null;
}
