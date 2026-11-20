import { asset } from './base';
import { onSound, sound } from './sound';

export type Voice = { stop: () => void };
type Options = { gain?: number; rate?: number };

const files: Record<string, string> = {
  switch: 'sounds/switch.m4a',
  knob: 'sounds/knob.m4a',
  tack: 'sounds/tack.m4a',
  bell: 'sounds/bell.m4a',
  pour: 'sounds/pour.m4a',
  click: 'sounds/click.m4a',
  static: 'sounds/static.m4a',
  creak: 'sounds/creak.m4a',
  lift: 'sounds/lift.m4a',
  down: 'sounds/down.m4a',
};

const MUSIC = 'sounds/lobby.m4a';
const LEVEL = 0.13;
const LATE = 150;
const RATE = 48000;
const INPUTS = ['pointerdown', 'keydown', 'touchend'] as const;
const NEXT = ['pointerdown', 'keydown'] as const;
const touch = typeof window !== 'undefined' && window.matchMedia('(hover: none) and (pointer: coarse)').matches;

function quiet() {
  return new Promise<void>((resolve) => {
    const host = window as Window & { requestIdleCallback?: (callback: () => void, options: { timeout: number }) => number };
    if (host.requestIdleCallback) host.requestIdleCallback(() => resolve(), { timeout: 2000 });
    else window.setTimeout(resolve, 200);
  });
}

export const tune = { wanted: true, sounding: false };

let context: AudioContext | null = null;
let resuming: Promise<void> | null = null;
let settable: boolean | null = null;
const buffers = new Map<string, Promise<AudioBuffer | null>>();
const elements = new Map<string, HTMLAudioElement>();
const asked = new Set<string>();
let primed = false;

const music = {
  element: null as HTMLAudioElement | null,
  gain: null as GainNode | null,
  level: 0,
  playing: false,
  timer: 0,
  armed: false,
  listening: false,
  away: false,
  autoAt: 0,
};

function useElements() {
  if (touch) return false;
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

function decode(name: string, target: BaseAudioContext) {
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

function prime() {
  window.removeEventListener('pointermove', prime);
  if (primed || !sound.on || !elements.size) return;
  primed = true;
  const primer = elements.values().next().value as HTMLAudioElement;
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

export function preload(names: string[]) {
  if (!sound.on) return;
  const fresh = names.filter((name) => files[name] && !asked.has(name));
  if (!fresh.length) return;
  for (const name of fresh) asked.add(name);
  const offline = !useElements() && typeof OfflineAudioContext !== 'undefined' ? new OfflineAudioContext(1, 1, RATE) : null;
  void (async () => {
    for (const name of fresh) {
      await quiet();
      if (offline) await decode(name, offline);
      else if (useElements()) element(name);
    }
    if (useElements() && !primed) window.addEventListener('pointermove', prime, { passive: true });
  })();
}

export function audio() {
  if (!context) {
    context = new AudioContext();
    resuming = null;
    context.addEventListener('statechange', update);
  }
  if (context.state !== 'running') resuming ??= context.resume().finally(() => (resuming = null));
  return context;
}

function wait(ms: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, Math.max(0, ms)));
}

async function start(name: string, { gain = 0.6, rate = 1 }: Options): Promise<Voice | null> {
  const began = performance.now();
  const target = audio();
  const data = await decode(name, target);
  if (!data || !sound.on) return null;
  if (target.state !== 'running' && resuming) await Promise.race([resuming, wait(LATE - (performance.now() - began))]);
  if (target.state !== 'running' || performance.now() - began > LATE) return null;
  const source = target.createBufferSource();
  const level = target.createGain();
  level.gain.value = gain;
  source.buffer = data;
  source.playbackRate.value = rate;
  source.connect(level).connect(target.destination);
  source.start();
  return source;
}

export function shot(name: string, { gain = 0.6, rate = 1 }: Options = {}): Promise<Voice | null> {
  if (!sound.on || !files[name]) return Promise.resolve(null);
  if (!useElements()) return start(name, { gain, rate });
  const base = element(name);
  const voice = base.paused ? base : (base.cloneNode() as HTMLAudioElement);
  let late = false;
  const timer = window.setTimeout(() => {
    late = true;
    voice.muted = true;
  }, LATE);
  voice.muted = false;
  voice.volume = gain;
  voice.preservesPitch = false;
  (voice as HTMLAudioElement & { webkitPreservesPitch?: boolean }).webkitPreservesPitch = false;
  voice.playbackRate = rate;
  voice.currentTime = 0;
  return voice.play().then(
    () => {
      window.clearTimeout(timer);
      if (!late) return { stop: () => voice.pause() };
      voice.pause();
      voice.muted = false;
      return null;
    },
    () => {
      window.clearTimeout(timer);
      voice.muted = false;
      return null;
    },
  );
}

function update() {
  const running = !music.gain || context?.state === 'running';
  const sounding = music.playing && music.level > 0 && running;
  if (sounding !== tune.sounding) tune.sounding = sounding;
}

function track() {
  if (music.element) return music.element;
  const media = new Audio(asset(MUSIC));
  media.loop = true;
  media.preload = 'auto';
  media.addEventListener('playing', () => {
    music.playing = true;
    update();
  });
  for (const name of ['pause', 'waiting', 'ended', 'emptied']) {
    media.addEventListener(name, () => {
      music.playing = false;
      update();
    });
  }
  music.element = media;
  return media;
}

function wire(media: HTMLAudioElement) {
  if (music.gain || useElements()) return;
  const target = audio();
  const gain = target.createGain();
  gain.gain.value = 0;
  target.createMediaElementSource(media).connect(gain).connect(target.destination);
  music.gain = gain;
}

function ramp(to: number, seconds: number, then?: () => void) {
  window.clearTimeout(music.timer);
  const media = music.element;
  if (!media) return;
  if (music.gain && context) {
    const level = music.gain.gain;
    const now = context.currentTime;
    level.cancelScheduledValues(now);
    level.setValueAtTime(level.value, now);
    level.linearRampToValueAtTime(to, now + seconds);
    if (then) music.timer = window.setTimeout(then, seconds * 1000);
    return;
  }
  const from = media.volume;
  const begun = performance.now();
  const tick = () => {
    const t = Math.min((performance.now() - begun) / (seconds * 1000), 1);
    media.volume = Math.min(Math.max(from + (to - from) * t, 0), 1);
    if (t < 1) music.timer = window.setTimeout(tick, 30);
    else then?.();
  };
  tick();
}

function begin(seconds: number) {
  if (!sound.on || !tune.wanted || document.hidden) return;
  const media = track();
  wire(media);
  music.level = LEVEL;
  if (music.gain) audio();
  if (!media.paused) {
    ramp(LEVEL, seconds);
    update();
    return;
  }
  if (music.gain) music.gain.gain.value = 0;
  else media.volume = 0;
  media.play().then(
    () => {
      if (music.level > 0) ramp(LEVEL, seconds);
      update();
    },
    () => {
      if (music.level > 0 && media.paused) listen();
    },
  );
}

function halt(seconds: number) {
  music.level = 0;
  update();
  const media = music.element;
  if (!media || media.paused) return;
  ramp(0, seconds, () => {
    if (music.level === 0) media.pause();
  });
}

function onInput(event: Event) {
  const target = event.target as Element | null;
  if (target?.closest?.('[data-sound]')) return;
  unlisten();
  if (!music.element || music.element.paused) {
    music.autoAt = performance.now();
    const drop = () => {
      music.autoAt = 0;
      for (const name of NEXT) window.removeEventListener(name, drop, true);
    };
    for (const name of NEXT) window.addEventListener(name, drop, true);
  }
  begin(1.5);
}

function listen() {
  if (music.listening) return;
  music.listening = true;
  for (const name of INPUTS) window.addEventListener(name, onInput, { capture: true, passive: true });
}

function unlisten() {
  music.listening = false;
  for (const name of INPUTS) window.removeEventListener(name, onInput, { capture: true });
}

function onVisible() {
  if (document.hidden) {
    if (music.level <= 0) return;
    music.away = true;
    halt(0.6);
    return;
  }
  if (!music.away) return;
  music.away = false;
  begin(1.5);
}

export function tape() {
  const fresh = music.autoAt > 0 && performance.now() - music.autoAt < 1500;
  music.autoAt = 0;
  if (fresh) {
    tune.wanted = true;
    return;
  }
  tune.wanted = !tune.wanted;
  if (tune.wanted) begin(0.5);
  else halt(0.3);
}

export async function arm() {
  if (music.armed) return;
  music.armed = true;
  await quiet();
  onSound((on) => {
    if (on) begin(0.5);
    else {
      music.away = false;
      halt(0.2);
    }
  });
  document.addEventListener('visibilitychange', onVisible);
  preload(['click']);
  const active = (navigator as Navigator & { userActivation?: { hasBeenActive: boolean } }).userActivation?.hasBeenActive;
  if (active) begin(1.5);
  else listen();
}
