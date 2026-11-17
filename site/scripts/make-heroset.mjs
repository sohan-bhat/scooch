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
const EARLY = 0.2;
const WIDE = 0.5;
const OVER = 1.5;
const REGION = 2;
const LABELS = { dial: 512, typed: 680 };

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
  desk: ['metal_office_desk'], laptop: ['classic_laptop'], lamp: ['desk_lamp_arm_01'], television: ['television_02'], clock: ['alarm_clock_01'], cassette: ['cassette_player'], plant: ['potted_plant_04'], stapler: ['vintage_stapler'], glasses: ['round_spectacles'], gamepad: ['gamepad'],
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
  if (name === 'labels') return Math.min(...Object.values(LABELS));
  const path = heroModels.includes(name) ? join(site, 'public', 'models', 'lite', `${name}.glb`) : join(site, 'public', 'models', `${name}.glb`);
  return Math.max(...(await Promise.all(imageSizes(await readFile(path)))));
}

async function storyOrder(browser, names) {
  const context = await browser.newContext({ viewport: { width: view.width, height: view.height }, deviceScaleFactor: view.scale });
  const page = await context.newPage();
  await page.goto(`${origin}?poster&scan&v=${Date.now()}`, { waitUntil: 'load' });
  await page.waitForFunction(() => performance.getEntriesByName('scooch:settled').length > 0, null, { timeout: 120000, polling: 100 });
  const first = {};
  const early = {};
  const moves = {};
  const width = view.width * view.scale;
  const bases = Object.fromEntries(await Promise.all(names.map(async (name) => [name, await baseSize(name)])));
  for (let i = 0; i <= 200; i++) {
    const place = i / 200;
    await page.evaluate((f) => {
      const story = document.getElementById('story');
      window.scrollTo(0, story.offsetTop + f * (story.offsetHeight - innerHeight));
    }, place);
    await page.waitForTimeout(300);
    await page.evaluate(async () => {
      const { camera } = window.__scan;
      let last = '';
      for (let k = 0; k < 40; k++) {
        await new Promise((resolve) => setTimeout(resolve, 100));
        const now = camera.matrixWorld.elements.map((value) => value.toFixed(4)).join(',');
        if (now === last) return;
        last = now;
      }
    });
    const seen = await page.evaluate((families) => {
      const { gl, scene, camera, gallery, main, three } = window.__scan;
      const W = innerWidth;
      const H = innerHeight;
      const scale = devicePixelRatio;
      window.__ids ??= { target: new three.WebGLRenderTarget(W, H), pixels: new Uint8Array(W * H * 4), materials: new Map() };
      const { target, pixels, materials } = window.__ids;
      const ids = [];
      const paint = (name, side) => {
        let index = ids.indexOf(name);
        if (index < 0) index = ids.push(name) - 1;
        const key = `${index + 1}/${side}`;
        if (!materials.has(key)) {
          const material = new three.MeshBasicMaterial({ fog: false, toneMapped: false, side });
          material.color.setRGB((index + 1) / 255, 0, 0);
          materials.set(key, material);
        }
        return materials.get(key);
      };
      const within = (object, name) => { for (let at = object; at; at = at.parent) if (at.name === name) return true; return false; };
      const family = (object, material) => {
        if (object.name === 'caption') {
          if ((gl.properties.get(material).uniforms?.uReveal?.value ?? 1) <= 0) return null;
          return within(object, 'clock_whole') ? 'dial' : within(object, 'notepads_office_notepads_yellow_pad') ? 'typed' : null;
        }
        const uniforms = material?.uniforms;
        if (uniforms?.uFrame) {
          for (const texture of [uniforms.uFrame.value, uniforms.uFrameB?.value]) {
            if (!texture) continue;
            if (texture === main.texture) return 'main';
            for (const [id, frame] of Object.entries(gallery)) if (frame.texture === texture) return id;
          }
          return null;
        }
        const label = material?.name ?? '';
        if (/screen/.test(label)) return null;
        for (const [name, prefixes] of Object.entries(families)) if (prefixes.some((prefix) => label.startsWith(prefix))) return name;
        return null;
      };
      const clear = (material) => /glass/i.test(material?.name ?? '') || material?.depthWrite === false || (material?.transparent && material.opacity < 0.99) || (material?.transmission ?? 0) > 0;
      const swapped = [];
      const hidden = [];
      scene.traverseVisible((object) => {
        if (object.isLine || object.isPoints || object.isSprite) {
          hidden.push(object);
          return;
        }
        if (!object.isMesh) return;
        const material = Array.isArray(object.material) ? object.material[0] : object.material;
        const name = family(object, material);
        if (object.name !== 'caption' && !material?.uniforms?.uFrame && clear(material)) {
          hidden.push(object);
          return;
        }
        swapped.push([object, object.material]);
        object.material = paint(name ?? 'other', material?.side ?? 0);
      });
      for (const object of hidden) object.visible = false;
      const background = scene.background;
      const alpha = gl.getClearAlpha();
      const auto = gl.shadowMap.autoUpdate;
      const previous = gl.getRenderTarget();
      scene.background = null;
      gl.shadowMap.autoUpdate = false;
      try {
        gl.setRenderTarget(target);
        gl.setClearAlpha(0);
        gl.clear();
        gl.render(scene, camera);
        gl.readRenderTargetPixels(target, 0, 0, W, H, pixels);
      } finally {
        gl.setRenderTarget(previous);
        gl.setClearAlpha(alpha);
        gl.shadowMap.autoUpdate = auto;
        scene.background = background;
        for (const object of hidden) object.visible = true;
        for (const [object, material] of swapped) object.material = material;
      }
      const extents = ids.map(() => ({ x0: Infinity, x1: -Infinity, y0: Infinity, y1: -Infinity, n: 0 }));
      for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) {
        const k = (y * W + x) * 4;
        if (pixels[k + 3] < 255 || pixels[k] === 0) continue;
        const e = extents[pixels[k] - 1];
        if (!e) continue;
        e.n += 1;
        if (x < e.x0) e.x0 = x;
        if (x > e.x1) e.x1 = x;
        if (y < e.y0) e.y0 = y;
        if (y > e.y1) e.y1 = y;
      }
      const out = {};
      ids.forEach((name, index) => {
        const e = extents[index];
        if (name === 'other' || e.n === 0) return;
        const wide = (e.x1 - e.x0 + 1) * scale;
        out[name] = { size: Math.max(wide, (e.y1 - e.y0 + 1) * scale), wide, pixels: e.n };
      });
      return out;
    }, families);
    for (const name of names) {
      const parts = name === 'labels' ? Object.entries(LABELS) : [[name, bases[name]]];
      for (const [part, base] of parts) {
        const shown = seen[part] ?? { size: 0, wide: 0 };
        if (first[name] === undefined && shown.size > base) first[name] = place;
        if (place >= EARLY) continue;
        if (moves[name] === undefined && shown.wide > width * WIDE && shown.size > base * OVER) moves[name] = place;
        if (shown.wide > (early[name]?.wide ?? 0)) early[name] = { part, wide: Math.round(shown.wide), size: Math.round(shown.size), place };
      }
    }
  }
  await context.close();
  console.log('heroset bases', JSON.stringify(bases));
  return [...names].sort((a, b) => (first[a] ?? 2) - (first[b] ?? 2)).map((name) => ({ name, first: first[name] ?? null, move: moves[name] !== undefined, early: { ...(early[name] ?? { wide: 0, size: 0 }), base: bases[name], at: moves[name] } }));
}

const previous = JSON.parse(await readFile(file, 'utf8').catch(() => '{"hero":[]}'));
const browser = await launch();
if (process.argv.includes('--scan')) {
  const manifest = JSON.parse(await readFile(join(site, 'public', 'demo', 'manifest.json'), 'utf8'));
  const later = names.filter((name) => !previous.hero.includes(name)).flatMap((name) => (name === 'gallery' ? manifest.pictures.map((picture) => picture.id) : [name]));
  const story = await storyOrder(browser, later);
  for (const { name, first, move, early } of story) console.log('heroset scan', name, `first ${first ?? 'never'}`, move ? 'move' : '-', JSON.stringify(early));
  await browser.close();
  process.exit(0);
}
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
  const move = story.filter((entry) => entry.move).map(({ name }) => name);
  console.log('heroset first move', move.join(',') || 'none', JSON.stringify(Object.fromEntries(story.map(({ name, early }) => [name, early]))));
  await writeFile(file, `${JSON.stringify({ hero, move, order: restOrder, measured, firsts, view: `${view.width}x${view.height}@${view.scale}` }, null, 2)}\n`);
  const same = (a, b) => [...(a ?? [])].sort().join(',') === [...(b ?? [])].sort().join(',');
  if (!same(previous.hero, hero) || !same(previous.move, move) || (previous.order ?? []).join(',') !== restOrder.join(',')) {
    console.log('heroset changed, build again before the posters');
    process.exitCode = 2;
  }
} finally {
  await browser.close();
}
