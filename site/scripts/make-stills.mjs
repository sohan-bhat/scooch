import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';
import { launch, openStill } from './browser.mjs';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(site, 'public', 'stills');
const BEATS = 11;
const sizes = [
  { name: 'desktop', width: 1600, height: 1000, scale: 1, crop: null },
  { name: 'phone', width: 390, height: 844, scale: 2, crop: { width: 780, height: 940 } },
];

const heroes = [
  { name: 'desktop', file: 'hero-desktop', width: 2304, height: 900, scale: 1.5, quality: 80 },
  { name: 'square', file: 'hero-square', width: 1280, height: 1278, scale: 1.5, quality: 80 },
  { name: 'phone', file: 'hero-phone', width: 720, height: 847, scale: 2, quality: 80 },
];

const heroOnly = process.argv.includes('--hero');

function hex([r, g, b]) {
  return `#${[r, g, b].map((value) => Math.round(value).toString(16).padStart(2, '0')).join('')}`;
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)] ?? 0;
}

function inside(polygon, x, y) {
  let hit = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const [xi, yi] = polygon[i];
    const [xj, yj] = polygon[j];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) hit = !hit;
  }
  return hit;
}

async function juiceColors(png, shape, scale) {
  const { data, info } = await sharp(png).raw().toBuffer({ resolveWithObject: true });
  const at = (x, y) => {
    const i = (Math.round(y) * info.width + Math.round(x)) * info.channels;
    return [data[i], data[i + 1], data[i + 2]];
  };
  const polygon = shape.hull.map(([x, y]) => [x * scale, y * scale]);
  const top = { cx: shape.top.cx * scale, cy: shape.top.cy * scale, rx: shape.top.rx * scale, ry: shape.top.ry * scale };
  const xs = polygon.map((p) => p[0]);
  const left = Math.min(...xs);
  const right = Math.max(...xs);
  const bottom = shape.bottom * scale;
  const pick = (test) => {
    const channels = [[], [], []];
    for (let y = Math.floor(top.cy - top.ry); y <= Math.ceil(bottom); y++) {
      for (let x = Math.floor(left); x <= Math.ceil(right); x++) {
        if (!inside(polygon, x, y) || !test(x, y)) continue;
        at(x, y).forEach((value, c) => channels[c].push(value));
      }
    }
    return channels.map(median);
  };
  const inTop = (x, y) => ((x - top.cx) / (top.rx * 0.8)) ** 2 + ((y - top.cy) / (top.ry * 0.8)) ** 2 <= 1;
  const body = (from, to) => (x, y) => !inTop(x, y) && y > top.cy + top.ry && y < bottom - (bottom - top.cy) * 0.12 && x >= left + (right - left) * from && x <= left + (right - left) * to;
  return {
    surface: hex(pick(inTop)),
    stops: [0.1, 0.3, 0.5, 0.7, 0.9].map((at) => hex(pick(body(at - 0.07, at + 0.07)))),
  };
}

function simplify(points, tolerance) {
  const distance = ([x, y], [x1, y1], [x2, y2]) => {
    const dx = x2 - x1;
    const dy = y2 - y1;
    const length = Math.hypot(dx, dy) || 1;
    return Math.abs(dy * x - dx * y + x2 * y1 - y2 * x1) / length;
  };
  const reduce = (list) => {
    if (list.length < 3) return list;
    let worst = 0;
    let index = 0;
    for (let i = 1; i < list.length - 1; i++) {
      const d = distance(list[i], list[0], list[list.length - 1]);
      if (d > worst) {
        worst = d;
        index = i;
      }
    }
    if (worst <= tolerance) return [list[0], list[list.length - 1]];
    return [...reduce(list.slice(0, index + 1)).slice(0, -1), ...reduce(list.slice(index))];
  };
  const half = Math.floor(points.length / 2);
  return [...reduce([...points.slice(0, half + 1)]).slice(0, -1), ...reduce([...points.slice(half), points[0]]).slice(0, -1)];
}

function round(value) {
  return Math.round(value * 10) / 10;
}

await mkdir(out, { recursive: true });
const browser = await launch();
const juice = {};
try {
  for (const hero of heroes) {
    const empty = await openStill(browser, { beat: 0, hero: true, ...hero });
    const shot = await empty.page.screenshot({ type: 'png' });
    const shape = await empty.page.evaluate(() => window.__juice());
    await empty.context.close();
    const file = join(out, `${hero.file}.webp`);
    await writeFile(file, await sharp(shot).webp({ quality: hero.quality, effort: 6 }).toBuffer());
    const full = await openStill(browser, { beat: 0, hero: true, juice: true, ...hero });
    const reference = await full.page.screenshot({ type: 'png' });
    await full.context.close();
    const scale = hero.scale;
    const colors = await juiceColors(reference, shape, scale);
    const hull = simplify(shape.hull.map(([x, y]) => [x * scale, y * scale]), 0.6).map(([x, y]) => `${Math.round(x)} ${Math.round(y)}`);
    const top = { cx: round(shape.top.cx * scale), cy: round(shape.top.cy * scale), rx: round(shape.top.rx * scale), ry: round(shape.top.ry * scale) };
    const bottom = round(shape.bottom * scale);
    const xs = shape.hull.map(([x]) => x * scale);
    juice[hero.name] = {
      width: Math.round(hero.width * scale),
      height: Math.round(hero.height * scale),
      hull: `M${hull.join('L')}Z`,
      left: round(Math.min(...xs)),
      right: round(Math.max(...xs)),
      top,
      bottom,
      drop: round(bottom - (top.cy - top.ry)),
      ...colors,
    };
    console.log('wrote', file, JSON.stringify(colors));
  }
  await writeFile(join(site, 'src', 'home', 'juice.json'), `${JSON.stringify(juice, null, 2)}\n`);
  for (const size of heroOnly ? [] : sizes) {
    for (let beat = 0; beat < BEATS; beat++) {
      const { context, page } = await openStill(browser, { beat, ...size });
      const shot = await page.screenshot({ type: 'png' });
      await context.close();
      let image = sharp(shot);
      if (size.crop) image = image.extract({ left: 0, top: 0, ...size.crop });
      const file = join(out, `${size.name}-${beat + 1}.webp`);
      await writeFile(file, await image.webp({ quality: 78, effort: 6 }).toBuffer());
      console.log('wrote', file);
    }
  }
} finally {
  await browser.close();
}
