import { asset } from '@/lib/base';
import type { SceneryInfo, Tables } from './screen/scenery';

export type Live = {
  drop(icons: Float32Array): Promise<Tables>;
  drag(icons: Float32Array, held: number, budget: number): Promise<Tables | null>;
  free(): Promise<void>;
};

const IDLE = 30000;

async function rgb(url: string) {
  const response = await fetch(url);
  const bitmap = await createImageBitmap(await response.blob(), { colorSpaceConversion: 'none', premultiplyAlpha: 'none' });
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext('2d', { willReadFrequently: true });
  if (!context) throw new Error('no 2d context');
  context.drawImage(bitmap, 0, 0);
  const rgba = context.getImageData(0, 0, bitmap.width, bitmap.height).data;
  const out = new Uint8Array(bitmap.width * bitmap.height * 3);
  for (let i = 0, j = 0; i < rgba.length; i += 4, j += 3) {
    out[j] = rgba[i];
    out[j + 1] = rgba[i + 1];
    out[j + 2] = rgba[i + 2];
  }
  const size = { width: bitmap.width, height: bitmap.height };
  bitmap.close();
  return { pixels: out, ...size };
}

async function unzip(buffer: ArrayBuffer) {
  const head = new Uint8Array(buffer, 0, Math.min(2, buffer.byteLength));
  if (head[0] !== 0x1f || head[1] !== 0x8b) return buffer;
  return new Response(new Blob([buffer]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer();
}

const started = new Map<string, Promise<Live>>();
const warmed = new Map<string, Promise<unknown>>();
let idle = 0;

export function freeEngines() {
  for (const pending of started.values()) void pending.then((remote) => remote.free()).catch(() => undefined);
  started.clear();
  warmed.clear();
}

function later() {
  window.clearTimeout(idle);
  idle = window.setTimeout(freeEngines, IDLE);
}

if (typeof document !== 'undefined') {
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) later();
    else window.clearTimeout(idle);
  });
  const story = document.getElementById('story');
  if (story) {
    new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting) window.clearTimeout(idle);
      else later();
    }).observe(story);
  }
}

export function warm(info: SceneryInfo & { analysis?: { file: string } }, icons: Float32Array) {
  let pending = warmed.get(info.folder);
  if (!pending) {
    pending = engineFor(info).then((remote) => remote.drop(icons));
    warmed.set(info.folder, pending);
    pending.catch(() => warmed.delete(info.folder));
  }
  return pending;
}

export function engineFor(info: SceneryInfo & { analysis?: { file: string } }) {
  let pending = started.get(info.folder);
  if (!pending) {
    pending = (async () => {
      const base = asset(`demo/${info.folder}`);
      const [picture, analysis, module] = await Promise.all([
        rgb(`${base}/${info.canvas.file}`),
        fetch(`${base}/${info.analysis?.file ?? 'analysis.bin.gz'}`).then((response) => response.arrayBuffer()).then(unzip),
        import('@engine/engine.js'),
      ]);
      const remote = new module.Remote(new Worker(new URL('../../../crates/scooch-web/pkg/worker.js', import.meta.url), { type: 'module' }));
      await remote.call('create', [picture.pixels, picture.width, picture.height, new Uint8Array(analysis)]);
      await remote.screen(info.width, info.height);
      return remote;
    })();
    started.set(info.folder, pending);
    pending.catch(() => started.delete(info.folder));
  }
  return pending;
}
