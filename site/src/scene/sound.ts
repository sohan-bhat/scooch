import { asset } from '@/lib/base';
import { live } from './touch';

const files: Record<string, string> = {
  lamp: 'sounds/switch.m4a',
  channel: 'sounds/channel.m4a',
  power: 'sounds/power.m4a',
  key: 'sounds/key.m4a',
  tape: 'sounds/tape.m4a',
  clock: 'sounds/bell.m4a',
  stapler: 'sounds/stapler.m4a',
  cup: 'sounds/cup.m4a',
};

const KEY = 'scooch:sound';
let context: AudioContext | null = null;
const buffers = new Map<string, Promise<AudioBuffer | null>>();
let loop: AudioBufferSourceNode | null = null;
let bell: AudioBufferSourceNode | null = null;

function stored() {
  try {
    return window.localStorage.getItem(KEY) !== 'off';
  } catch {
    return true;
  }
}

export const sound = { on: typeof window === 'undefined' ? true : stored() };

export function setSound(on: boolean) {
  sound.on = on;
  try {
    window.localStorage.setItem(KEY, on ? 'on' : 'off');
  } catch {
    return;
  }
  if (!on) {
    loop?.stop();
    loop = null;
    bell?.stop();
    bell = null;
  }
}

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

async function start(name: string, repeat = false, gain = 0.6) {
  if (!context) return null;
  const data = await buffer(name);
  if (!data || !sound.on) return null;
  const source = context.createBufferSource();
  const level = context.createGain();
  level.gain.value = gain;
  source.buffer = data;
  source.loop = repeat;
  source.connect(level).connect(context.destination);
  source.start();
  return source;
}

export function play(name: string) {
  if (!sound.on) return;
  context ??= new AudioContext();
  if (context.state === 'suspended') void context.resume();
  if (name === 'cassette') {
    void start('key', false, 0.7);
    loop?.stop();
    loop = null;
    if (live.tape) void start('tape', true, 0.45).then((source) => (loop = live.tape ? source : (source?.stop(), null)));
    return;
  }
  if (name === 'clock') {
    bell?.stop();
    bell = null;
    if (live.ringUntil > 0) void start('clock', false, 0.5).then((source) => (bell = source));
    return;
  }
  void start(name);
}

export function hush() {
  loop?.stop();
  loop = null;
  bell?.stop();
  bell = null;
}
