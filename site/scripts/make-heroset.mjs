import { readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';
import { launch, origin } from './browser.mjs';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const file = join(site, 'src', 'scene', 'heroset.json');
const names = ['television', 'desk', 'laptop', 'lamp', 'clock', 'cassette', 'plant', 'stapler', 'glasses', 'gamepad', 'main', 'gallery', 'labels'];
const view = { width: 1440, height: 900, scale: 2 };
const GAIN = 0.1;
const REGION = 2;

async function settled(browser, query) {
  const context = await browser.newContext({ viewport: { width: view.width, height: view.height }, deviceScaleFactor: view.scale });
  const page = await context.newPage();
  page.on('pageerror', (error) => console.error('pageerror', error.message));
  await page.goto(`${origin}?poster&${query}${query ? '&' : ''}v=${Date.now()}`, { waitUntil: 'load' });
  await page.addStyleTag({ content: '[data-overlay="hint"]{visibility:hidden!important}' });
  await page.waitForFunction(() => performance.getEntriesByName('scooch:settled').length > 0, null, { timeout: 120000, polling: 100 });
  await page.waitForTimeout(800);
  const shot = await page.screenshot({ type: 'png' });
  await context.close();
  const { data, info } = await sharp(shot).removeAlpha().raw().toBuffer({ resolveWithObject: true });
  return { data, width: info.width, height: info.height };
}

function measure(image) {
  const { data, width: W, height: H } = image;
  const luma = new Float32Array(W * H);
  for (let p = 0; p < luma.length; p++) luma[p] = data[p * 3] * 0.299 + data[p * 3 + 1] * 0.587 + data[p * 3 + 2] * 0.114;
  const energy = [];
  for (let gy = 0; gy < 10; gy++) for (let gx = 0; gx < 16; gx++) {
    let sum = 0;
    let n = 0;
    for (let y = Math.max(1, Math.floor((gy * H) / 10)); y < Math.min(H - 1, Math.floor(((gy + 1) * H) / 10)); y++) {
      for (let x = Math.max(1, Math.floor((gx * W) / 16)); x < Math.min(W - 1, Math.floor(((gx + 1) * W) / 16)); x++) {
        const p = y * W + x;
        const l = 4 * luma[p] - luma[p - 1] - luma[p + 1] - luma[p - W] - luma[p + W];
        sum += l * l;
        n += 1;
      }
    }
    energy.push(sum / n);
  }
  const regions = [];
  for (let gy = 0; gy < 3; gy++) for (let gx = 0; gx < 4; gx++) {
    let sum = 0;
    let n = 0;
    for (let y = Math.floor((gy * H) / 3); y < Math.floor(((gy + 1) * H) / 3); y += 2) for (let x = Math.floor((gx * W) / 4); x < Math.floor(((gx + 1) * W) / 4); x += 2) {
      const i = (y * W + x) * 3;
      sum += data[i] + data[i + 1] + data[i + 2];
      n += 3;
    }
    regions.push(sum / n);
  }
  return { energy, regions };
}

function gain(base, other) {
  let top = 0;
  base.energy.forEach((e, i) => {
    if (e > 2) top = Math.max(top, other.energy[i] / e - 1);
  });
  return top;
}

function check(set, whole) {
  let loss = 0;
  whole.energy.forEach((e, i) => {
    if (e > 2) loss = Math.max(loss, 1 - set.energy[i] / e);
  });
  const region = Math.max(...whole.regions.map((value, i) => Math.abs(value - set.regions[i])));
  return { loss, region, pass: loss <= GAIN && region <= REGION };
}

const families = {
  desk: ['metal_office_desk'], laptop: ['classic_laptop'], lamp: ['desk_lamp_arm_01'], television: ['television_02'], clock: ['alarm_clock_01'], cassette: ['cassette_player'], plant: ['potted_plant_04'], stapler: ['vintage_stapler'], glasses: ['round_spectacles'], gamepad: ['gamepad'], labels: ['alarm_clock_01', 'office_notepads'],
};
const heroModels = ['television', 'desk', 'laptop', 'lamp'];

function imageSizes(buffer) {
  const length = buffer.readUInt32LE(12);
  const doc = JSON.parse(buffer.subarray(20, 20 + length).toString());
  const binary = buffer.subarray(20 + length + 8);
  return (doc.images ?? []).map((image) => {
    const view = doc.bufferViews[image.bufferView];
    const blob = binary.subarray(view.byteOffset ?? 0, (view.byteOffset ?? 0) + view.byteLength);
    return image.mimeType === 'image/ktx2' ? Promise.resolve(blob.readUInt32LE(20)) : sharp(blob).metadata().then((meta) => Math.max(meta.width, meta.height));
  });
}

async function baseSize(name) {
  const manifest = JSON.parse(await readFile(join(site, 'public', 'demo', 'manifest.json'), 'utf8'));
  const picture = manifest.pictures.find((candidate) => candidate.id === name);
  if (name === 'main' || picture) {
    const canvas = picture ? picture.scenery.canvas : manifest.scenery.canvas;
    return Math.max(canvas.width, canvas.height);
  }
  if (name === 'labels') return 768;
  const path = heroModels.includes(name) ? join(site, 'public', 'models', 'lite', `${name}.glb`) : join(site, 'public', 'models', `${name}.glb`);
  return Math.max(...(await Promise.all(imageSizes(await readFile(path)))));
}

async function storyOrder(browser, names) {
  const context = await browser.newContext({ viewport: { width: view.width, height: view.height }, deviceScaleFactor: view.scale });
  const page = await context.newPage();
  await page.goto(`${origin}?poster&scan&v=${Date.now()}`, { waitUntil: 'load' });
  await page.waitForFunction(() => performance.getEntriesByName('scooch:settled').length > 0, null, { timeout: 120000, polling: 100 });
  const first = {};
  const bases = Object.fromEntries(await Promise.all(names.map(async (name) => [name, await baseSize(name)])));
  for (let i = 0; i <= 200; i++) {
    const place = i / 200;
    await page.evaluate((f) => {
      const story = document.getElementById('story');
      window.scrollTo(0, story.offsetTop + f * (story.offsetHeight - innerHeight));
    }, place);
    await page.waitForTimeout(600);
    const seen = await page.evaluate((families) => {
      const { scene, camera, gallery, main } = window.__scan;
      camera.updateMatrixWorld();
      scene.updateMatrixWorld(true);
      const width = innerWidth * devicePixelRatio;
      const height = innerHeight * devicePixelRatio;
      const boxes = {};
      const visible = (object) => { for (let at = object; at; at = at.parent) if (!at.visible) return false; return true; };
      const add = (name, mesh) => {
        if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
        const b = mesh.geometry.boundingBox;
        const e = boxes[name] ?? { x0: Infinity, y0: Infinity, x1: -Infinity, y1: -Infinity };
        for (let k = 0; k < 8; k++) {
          const p = b.min.clone().set(k & 1 ? b.max.x : b.min.x, k & 2 ? b.max.y : b.min.y, k & 4 ? b.max.z : b.min.z).applyMatrix4(mesh.matrixWorld);
          if (p.clone().applyMatrix4(camera.matrixWorldInverse).z > -camera.near) return;
          p.project(camera);
          e.x0 = Math.min(e.x0, (p.x * 0.5 + 0.5) * width);
          e.x1 = Math.max(e.x1, (p.x * 0.5 + 0.5) * width);
          e.y0 = Math.min(e.y0, (0.5 - p.y * 0.5) * height);
          e.y1 = Math.max(e.y1, (0.5 - p.y * 0.5) * height);
        }
        boxes[name] = e;
      };
      scene.traverse((object) => {
        if (!object.isMesh || !visible(object)) return;
        const material = Array.isArray(object.material) ? object.material[0] : object.material;
        const label = material?.name ?? '';
        for (const [name, prefixes] of Object.entries(families)) if (prefixes.some((prefix) => label.startsWith(prefix)) && !/screen/.test(label)) add(name, object);
        const uniforms = material?.uniforms;
        if (uniforms?.uFrame) {
          const bound = [uniforms.uFrame.value];
          if (uniforms.uFrameB && (uniforms.uBlend?.value ?? 0) > 0) bound.push(uniforms.uFrameB.value);
          for (const texture of bound) {
            if (!texture) continue;
            if (texture === main.texture) add('main', object);
            for (const [id, frame] of Object.entries(gallery)) if (frame.texture === texture) add(id, object);
          }
        }
      });
      const out = {};
      for (const [name, e] of Object.entries(boxes)) {
        const w = Math.min(width, e.x1) - Math.max(0, e.x0);
        const h = Math.min(height, e.y1) - Math.max(0, e.y0);
        if (w > 0 && h > 0) out[name] = Math.max(w, h);
      }
      return out;
    }, families);
    for (const name of names) if (first[name] === undefined && (seen[name] ?? 0) > bases[name]) first[name] = place;
  }
  await context.close();
  console.log('heroset bases', JSON.stringify(bases));
  return [...names].sort((a, b) => (first[a] ?? 2) - (first[b] ?? 2)).map((name) => ({ name, first: first[name] ?? null }));
}

const previous = JSON.parse(await readFile(file, 'utf8').catch(() => '{"hero":[]}'));
const browser = await launch();
try {
  const none = measure(await settled(browser, 'sharp='));
  const whole = measure(await settled(browser, ''));
  const gains = {};
  for (const name of names) {
    gains[name] = gain(none, measure(await settled(browser, `sharp=${name}`)));
    console.log('heroset', name, `${(gains[name] * 100).toFixed(1)} percent`);
  }
  const order = names.filter((name) => gains[name] > 0).sort((a, b) => gains[b] - gains[a]);
  const hero = order.filter((name) => gains[name] > GAIN);
  let result = check(measure(await settled(browser, `sharp=${hero.join(',')}`)), whole);
  for (const name of order) {
    if (result.pass) break;
    if (hero.includes(name)) continue;
    hero.push(name);
    result = check(measure(await settled(browser, `sharp=${hero.join(',')}`)), whole);
  }
  console.log('heroset', hero.join(','), `largest energy loss ${(result.loss * 100).toFixed(1)} percent, worst region ${result.region.toFixed(2)}, ${result.pass ? 'pass' : 'fail'}`);
  if (!result.pass) process.exitCode = 1;
  const measured = Object.fromEntries(names.map((name) => [name, Math.round(gains[name] * 1000) / 10]));
  const manifest = JSON.parse(await readFile(join(site, 'public', 'demo', 'manifest.json'), 'utf8'));
  const later = names.filter((name) => !hero.includes(name)).flatMap((name) => (name === 'gallery' ? manifest.pictures.map((picture) => picture.id) : [name]));
  const story = await storyOrder(browser, later);
  console.log('heroset order', story.map(({ name, first }) => `${name} ${first ?? 'never'}`).join(', '));
  const restOrder = story.map(({ name }) => name);
  const firsts = Object.fromEntries(story.map(({ name, first }) => [name, first]));
  await writeFile(file, `${JSON.stringify({ hero, order: restOrder, measured, firsts, view: `${view.width}x${view.height}@${view.scale}` }, null, 2)}\n`);
  if ([...previous.hero].sort().join(',') !== [...hero].sort().join(',')) {
    console.log('heroset changed, build again before the posters');
    process.exitCode = 2;
  }
} finally {
  await browser.close();
}
