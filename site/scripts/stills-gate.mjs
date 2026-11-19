import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import sharp from 'sharp';
import { origin } from './browser.mjs';

const REGION = 2;
const LOSS = 0.1;
const FLOOR = 50;

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

export async function liveShots(browser, size, places) {
  const context = await browser.newContext({ viewport: { width: size.width, height: size.height }, deviceScaleFactor: size.scale });
  const page = await context.newPage();
  await page.goto(`${origin}?hold&v=${Date.now()}`, { waitUntil: 'load' });
  await page.addStyleTag({ content: 'header, .beats, [data-overlay="hint"], #desk-controls { visibility: hidden !important; }' });
  await page.waitForFunction(() => performance.getEntriesByName('scooch:uploads').length > 0, null, { timeout: 180000, polling: 200 });
  const shots = [];
  for (const place of places) {
    await page.evaluate((f) => {
      const story = document.getElementById('story');
      window.scrollTo(0, story.offsetTop + f * (story.offsetHeight - innerHeight));
    }, place);
    await page.waitForTimeout(1500);
    shots.push(await stable(page));
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

export async function compare(still, live) {
  const a = await sharp(still).removeAlpha().raw().toBuffer({ resolveWithObject: true });
  const b = await sharp(live).resize(a.info.width, a.info.height).removeAlpha().raw().toBuffer();
  const s = measure(a.data, a.info.width, a.info.height);
  const l = measure(b, a.info.width, a.info.height);
  const region = Math.max(...s.regions.map((value, i) => Math.abs(value - l.regions[i])));
  const around = (energy, k) => {
    const gx = k % 16;
    const gy = Math.floor(k / 16);
    let sum = 0;
    for (let y = Math.max(0, gy - 1); y <= Math.min(9, gy + 1); y += 1) for (let x = Math.max(0, gx - 1); x <= Math.min(15, gx + 1); x += 1) sum += energy[y * 16 + x];
    return sum;
  };
  let loss = 0;
  let cell = '-';
  l.energy.forEach((e, k) => {
    if (e < FLOOR) return;
    const r = 1 - around(s.energy, k) / around(l.energy, k);
    if (r > loss) {
      loss = r;
      cell = `${k % 16},${Math.floor(k / 16)}`;
    }
  });
  return { region, loss, cell, pass: region <= REGION && loss <= LOSS };
}
