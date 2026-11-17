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
  await writeFile(file, `${JSON.stringify({ hero, measured, view: `${view.width}x${view.height}@${view.scale}` }, null, 2)}\n`);
  if ([...previous.hero].sort().join(',') !== [...hero].sort().join(',')) {
    console.log('heroset changed, build again before the posters');
    process.exitCode = 2;
  }
} finally {
  await browser.close();
}
