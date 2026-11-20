import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import sharp from 'sharp';
import { origin } from './browser.mjs';

const REGION = 2;
const LOSS = 0.1;
const FLOOR = 20;
const HEROES = ['desk', 'laptop', 'lamp', 'television'];
const MODELS = [...HEROES, 'clock', 'cassette', 'plant', 'stapler', 'glasses', 'gamepad'];

async function colourSizes(path) {
  const buffer = await readFile(path);
  const length = buffer.readUInt32LE(12);
  const doc = JSON.parse(buffer.subarray(20, 20 + length).toString());
  const binary = buffer.subarray(20 + length + 8);
  const sizes = {};
  for (const material of doc.materials ?? []) {
    const info = material.pbrMetallicRoughness?.baseColorTexture;
    if (!info) continue;
    const texture = doc.textures[info.index];
    const source = texture.source ?? texture.extensions?.KHR_texture_basisu?.source ?? texture.extensions?.EXT_texture_webp?.source;
    const image = doc.images[source];
    const view = doc.bufferViews[image.bufferView];
    const blob = binary.subarray(view.byteOffset ?? 0, (view.byteOffset ?? 0) + view.byteLength);
    const width = image.mimeType === 'image/ktx2' ? blob.readUInt32LE(20) : (await sharp(blob).metadata()).width;
    sizes[material.name] = Math.max(sizes[material.name] ?? 0, width);
  }
  return sizes;
}

export async function sharpSet(site) {
  const heroset = JSON.parse(await readFile(join(site, 'src', 'scene', 'heroset.json'), 'utf8'));
  const names = [...new Set([...heroset.hero, ...(heroset.move ?? []), ...heroset.order])];
  const manifest = JSON.parse(await readFile(join(site, 'public', 'demo', 'manifest.json'), 'utf8'));
  const labels = await readFile(join(site, 'src', 'scene', 'labels.ts'), 'utf8');
  const dial = Number(labels.match(/const next = paintedTexture\((\d+),/)[1]);
  const typed = Number(labels.match(/typed\?\.sharpen\(gl, \[(\d+),/)[1]);
  const models = {};
  for (const name of names.filter((n) => MODELS.includes(n))) models[name] = await colourSizes(join(site, 'public', 'models', HEROES.includes(name) ? `${name}.glb` : `sharp/${name}.glb`));
  const canvases = {};
  for (const name of names.filter((n) => n === 'main' || manifest.pictures.some((picture) => picture.id === n))) {
    const scenery = name === 'main' ? manifest.scenery : manifest.pictures.find((picture) => picture.id === name).scenery;
    if (scenery.sharp) canvases[name] = { file: `demo/${scenery.folder}/${scenery.sharp.file}`, size: [scenery.sharp.width, scenery.sharp.height] };
  }
  return { names, models, canvases, labels: names.includes('labels') ? { dial, typed } : null };
}

async function proveSharp(page, set) {
  const seen = await page.evaluate(() => {
    const { scene } = window.__scan;
    const materials = {};
    const captions = {};
    scene.traverse((object) => {
      if (!object.isMesh) return;
      const material = Array.isArray(object.material) ? object.material[0] : object.material;
      const width = material?.map?.image?.width ?? 0;
      if (material?.name) materials[material.name] = Math.max(materials[material.name] ?? 0, width);
      if (object.name === 'caption') {
        let place = 'other';
        for (let at = object; at; at = at.parent) if (at.name === 'clock_whole') place = 'dial';
        for (let at = object; at; at = at.parent) if (at.name === 'notepads_office_notepads_yellow_pad') place = 'typed';
        captions[place] = Math.max(captions[place] ?? 0, width);
      }
    });
    return { materials, captions, files: performance.getEntriesByType('resource').map((entry) => new URL(entry.name).pathname), uploads: window.__uploads ?? [] };
  });
  const missing = [];
  for (const [name, sizes] of Object.entries(set.models)) {
    for (const [material, width] of Object.entries(sizes)) if ((seen.materials[material] ?? 0) < width) missing.push(`${name} (${material} at ${seen.materials[material] ?? 'none'}, needs ${width})`);
  }
  for (const [name, canvas] of Object.entries(set.canvases)) {
    const fetched = seen.files.some((file) => file.endsWith(`/${canvas.file}`));
    const uploaded = seen.uploads.filter(([w, h]) => w === canvas.size[0] && h === canvas.size[1]).length;
    const needed = Object.values(set.canvases).filter((other) => other.size[0] === canvas.size[0] && other.size[1] === canvas.size[1]).length;
    if (!fetched || uploaded < needed) missing.push(`${name} (${canvas.file} ${fetched ? 'fetched' : 'not fetched'}, ${uploaded} of ${needed} uploads at ${canvas.size.join('x')})`);
  }
  if (set.labels) {
    if ((seen.captions.dial ?? 0) < set.labels.dial) missing.push(`labels (dial at ${seen.captions.dial ?? 'none'}, needs ${set.labels.dial})`);
    if ((seen.captions.typed ?? 0) < set.labels.typed) missing.push(`labels (typed pad at ${seen.captions.typed ?? 'none'}, needs ${set.labels.typed})`);
  }
  return missing;
}

export async function restPlaces(site) {
  const source = await readFile(join(site, 'src', 'story', 'timeline.ts'), 'utf8');
  const total = Number(source.match(/export const TOTAL = (\d+(?:\.\d+)?)/)[1]);
  return [...source.matchAll(/\{ id: '[a-z]+', start: [\d.]+, end: [\w.]+, rest: ([\d.]+) \}/g)].map((match) => Number(match[1]) / total);
}

async function stable(page) {
  let last = null;
  for (let k = 0; k < 40; k += 1) {
    await page.waitForTimeout(500);
    const shot = await page.screenshot({ type: 'png' });
    const { data } = await sharp(shot).resize(320).removeAlpha().raw().toBuffer({ resolveWithObject: true });
    if (last) {
      let sum = 0;
      for (let i = 0; i < data.length; i += 1) sum += Math.abs(data[i] - last[i]);
      if (sum / data.length < 0.05) return shot;
    }
    last = data;
  }
  return page.screenshot({ type: 'png' });
}

export async function liveShots(browser, size, places, set = null, query = '') {
  const context = await browser.newContext({ viewport: { width: size.width, height: size.height }, deviceScaleFactor: size.scale });
  await context.addInitScript(() => {
    window.__uploads = [];
    const proto = WebGL2RenderingContext.prototype;
    const storage = proto.texStorage2D;
    proto.texStorage2D = function patched(target, levels, format, width, height) {
      window.__uploads.push([width, height]);
      return storage.call(this, target, levels, format, width, height);
    };
  });
  const page = await context.newPage();
  await page.goto(`${origin}?hold&clean&scan${query}&v=${Date.now()}`, { waitUntil: 'load' });
  await page.addStyleTag({ content: 'header, .beats, [data-overlay="hint"], #desk-controls { visibility: hidden !important; }' });
  await page.waitForFunction(() => performance.getEntriesByName('scooch:uploads').length > 0, null, { timeout: 180000, polling: 200 });
  const shots = [];
  for (const place of places) {
    await page.evaluate((f) => {
      const story = document.getElementById('story');
      window.scrollTo(0, story.offsetTop + f * (story.offsetHeight - innerHeight));
    }, place);
    await page.waitForTimeout(1500);
    const shot = await stable(page);
    if (set) {
      const missing = await proveSharp(page, set);
      if (missing.length) {
        await context.close();
        throw new Error(`sharp set missing before the shot at ${place.toFixed(3)}: ${missing.join(', ')}`);
      }
    }
    shots.push(shot);
  }
  await context.close();
  return shots;
}

function measure(data, width, height) {
  const luma = new Float32Array(width * height);
  for (let p = 0; p < luma.length; p += 1) luma[p] = data[p * 3] * 0.299 + data[p * 3 + 1] * 0.587 + data[p * 3 + 2] * 0.114;
  const energy = [];
  for (let gy = 0; gy < 10; gy += 1) for (let gx = 0; gx < 16; gx += 1) {
    let sum = 0;
    let n = 0;
    for (let y = Math.max(1, Math.floor((gy * height) / 10)); y < Math.min(height - 1, Math.floor(((gy + 1) * height) / 10)); y += 1) {
      for (let x = Math.max(1, Math.floor((gx * width) / 16)); x < Math.min(width - 1, Math.floor(((gx + 1) * width) / 16)); x += 1) {
        const p = y * width + x;
        const l = 4 * luma[p] - luma[p - 1] - luma[p + 1] - luma[p - width] - luma[p + width];
        sum += l * l;
        n += 1;
      }
    }
    energy.push(sum / n);
  }
  const regions = [];
  for (let gy = 0; gy < 3; gy += 1) for (let gx = 0; gx < 4; gx += 1) {
    let sum = 0;
    let n = 0;
    for (let y = Math.floor((gy * height) / 3); y < Math.floor(((gy + 1) * height) / 3); y += 1) {
      for (let x = Math.floor((gx * width) / 4); x < Math.floor(((gx + 1) * width) / 4); x += 1) {
        const i = (y * width + x) * 3;
        sum += data[i] + data[i + 1] + data[i + 2];
        n += 3;
      }
    }
    regions.push(sum / n);
  }
  return { energy, regions };
}

export async function compare(still, live, shrink = 1) {
  const meta = await sharp(still).metadata();
  const width = Math.round(meta.width / shrink);
  const height = Math.round(meta.height / shrink);
  const a = await sharp(still).resize(width, height).removeAlpha().raw().toBuffer({ resolveWithObject: true });
  const b = await sharp(live).resize(width, height).removeAlpha().raw().toBuffer();
  const s = measure(a.data, a.info.width, a.info.height);
  const l = measure(b, a.info.width, a.info.height);
  const region = Math.max(...s.regions.map((value, i) => Math.abs(value - l.regions[i])));
  let loss = 0;
  let cell = '-';
  l.energy.forEach((e, k) => {
    if (e < FLOOR) return;
    const r = 1 - s.energy[k] / e;
    if (r > loss) {
      loss = r;
      cell = `${k % 16},${Math.floor(k / 16)}`;
    }
  });
  return { region, loss, cell, pass: region <= REGION && loss <= LOSS };
}

const ENCODERS = {
  avif: (quality) => (image) => image.avif({ quality, effort: 4, chromaSubsampling: '4:4:4' }),
  webp: (quality) => (image) => image.webp({ quality, effort: 6, smartSubsample: true }),
};

export async function encodeLowest(render, format, low = 60, high = 100) {
  const tried = new Map();
  const at = async (quality) => {
    if (!tried.has(quality)) {
      const buffer = await ENCODERS[format](quality)(sharp(render)).toBuffer();
      const result = await compare(await sharp(buffer).png().toBuffer(), render);
      tried.set(quality, { quality, buffer, ...result });
    }
    return tried.get(quality);
  };
  if (!(await at(high)).pass) return at(high);
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if ((await at(middle)).pass) high = middle;
    else low = middle + 1;
  }
  return at(high);
}

export async function encodeUnder(render, format, maxBytes, low = 40, high = 100) {
  const tried = new Map();
  const at = async (quality) => {
    if (!tried.has(quality)) tried.set(quality, await ENCODERS[format](quality)(sharp(render)).toBuffer());
    return tried.get(quality);
  };
  if ((await at(low)).length > maxBytes) return null;
  while (low < high) {
    const middle = Math.ceil((low + high) / 2);
    if ((await at(middle)).length <= maxBytes) low = middle;
    else high = middle - 1;
  }
  const buffer = await at(low);
  return { quality: low, buffer, ...(await compare(await sharp(buffer).png().toBuffer(), render)) };
}
