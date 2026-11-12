import { readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const demo = join(site, 'public', 'demo');
const target = resolve(site, '..', 'docs', 'demo.jpg');
const manifest = JSON.parse(await readFile(join(demo, 'manifest.json'), 'utf8'));
const tokens = await readFile(join(site, 'src', 'styles', 'tokens.css'), 'utf8');
const room = tokens.match(/--color-room:\s*(#[0-9a-f]{6})/i)[1];
const { width, height } = manifest.screen;
const total = 1600;
const gap = 16;
const panel = (total - gap) / 2;
const panelHeight = Math.round((panel * height) / width);
const cell = 256;
const grid = 4;

async function icon(index, size) {
  const left = (index % grid) * cell;
  const top = Math.floor(index / grid) * cell;
  return sharp(join(demo, 'atlas.png')).extract({ left, top, width: cell, height: cell }).resize(size, size).png().toBuffer();
}

async function frame(name) {
  const data = manifest.frames[name];
  const layers = [];
  for (const [index, entry] of manifest.icons.entries()) {
    const [x, y, w] = data.icons[entry.id];
    const size = Math.round(w * width);
    layers.push({ input: await icon(index, size), left: Math.round(x * width), top: Math.round(y * height) });
  }
  const full = await sharp(join(demo, data.frame)).composite(layers).png().toBuffer();
  return sharp(full).resize(panel, panelHeight).png().toBuffer();
}

const [before, fixed] = await Promise.all([frame('before'), frame('fixed')]);
let quality = 88;
const draw = (value) =>
  sharp({ create: { width: total, height: panelHeight, channels: 3, background: room } })
    .composite([
      { input: before, left: 0, top: 0 },
      { input: fixed, left: panel + gap, top: 0 },
    ])
    .jpeg({ quality: value, mozjpeg: true })
    .toBuffer();
let bytes = await draw(quality);
while (bytes.length > 245000 && quality > 50) {
  quality -= 4;
  bytes = await draw(quality);
}
await writeFile(target, bytes);
console.log('wrote', target, Math.round(bytes.length / 1024), 'kB at quality', quality);
