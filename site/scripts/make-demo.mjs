import { execFile } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import sharp from 'sharp';

const run = promisify(execFile);
const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const config = JSON.parse(await readFile(join(site, 'scripts', 'demo.config.json'), 'utf8'));
const binary = resolve(site, process.env.SCOOCH_BIN ?? config.binary);
const photo = resolve(site, config.photo);
const work = join(site, '.cache', 'demo');
const out = join(site, 'public', 'demo');
const { width, height } = config.screen;
const mapScale = 4;

function rects(layout) {
  return config.icons.map((icon) => [...layout[icon.id], config.icon.width, config.icon.height]);
}

function lookup(table, t) {
  const p = Math.min(Math.max(t, 0), 1) * (table.length - 1);
  const i = Math.min(Math.floor(p), table.length - 2);
  return table[i] + (table[i + 1] - table[i]) * (p - i);
}

function inverse(table, value) {
  const rising = table[table.length - 1] >= table[0];
  let low = 0;
  let high = 1;
  for (let step = 0; step < 24; step++) {
    const middle = (low + high) / 2;
    if (lookup(table, middle) < value === rising) low = middle;
    else high = middle;
  }
  return (low + high) / 2;
}

function sample(map, mapWidth, mapHeight, u, v) {
  const x = Math.min(Math.max(u * mapWidth - 0.5, 0), mapWidth - 1);
  const y = Math.min(Math.max(v * mapHeight - 0.5, 0), mapHeight - 1);
  const x0 = Math.floor(x);
  const y0 = Math.floor(y);
  const x1 = Math.min(x0 + 1, mapWidth - 1);
  const y1 = Math.min(y0 + 1, mapHeight - 1);
  const fx = x - x0;
  const fy = y - y0;
  const top = map[y0 * mapWidth + x0] * (1 - fx) + map[y0 * mapWidth + x1] * fx;
  const bottom = map[y1 * mapWidth + x0] * (1 - fx) + map[y1 * mapWidth + x1] * fx;
  return top * (1 - fy) + bottom * fy;
}

function onScreen(analysis, placement) {
  const w = Math.round(width / mapScale);
  const h = Math.round(height / mapScale);
  const pixels = Buffer.alloc(w * h);
  for (let y = 0; y < h; y++) {
    const v = lookup(placement.fy, (y + 0.5) / h);
    for (let x = 0; x < w; x++) {
      const u = lookup(placement.fx, (x + 0.5) / w);
      const value = sample(analysis.saliency, analysis.width, analysis.height, u, v);
      pixels[y * w + x] = Math.round(Math.min(Math.max(value, 0), 1) * 255);
    }
  }
  return sharp(pixels, { raw: { width: w, height: h, channels: 1 } }).png({ compressionLevel: 9 });
}

function subjectOnScreen(analysis, placement) {
  const { x, y, w, h } = analysis.subject;
  const left = inverse(placement.fx, x);
  const right = inverse(placement.fx, x + w);
  const top = inverse(placement.fy, y);
  const bottom = inverse(placement.fy, y + h);
  return [Math.min(left, right), Math.min(top, bottom), Math.abs(right - left), Math.abs(bottom - top)].map(round);
}

function round(value) {
  return Math.round(value * 1e4) / 1e4;
}

async function average(path) {
  const { channels } = await sharp(path).stats();
  return channels.slice(0, 3).map((channel) => round(channel.mean / 255));
}

function dropped(start, subject) {
  const { icon, to, onto } = config.drag;
  if (to) return { ...start, [icon]: to };
  if (onto !== 'subject') throw new Error('drag needs a to position or onto subject');
  const [sx, sy, sw, sh] = subject;
  const snap = (value) => Math.round(value / 10) * 10;
  const x = snap(Math.min(Math.max((sx + sw * 0.5) * width - config.icon.width / 2, 24), width - config.icon.width - 24));
  const y = snap(Math.min(Math.max((sy + sh * 0.4) * height - config.icon.height / 2, 24), height - config.icon.height - 24));
  return { ...start, [icon]: [x, y] };
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

async function render(state, layout) {
  const icons = rects(layout);
  const scenePath = join(work, `${state.id}.scene.json`);
  const framePath = join(work, `${state.id}.jpg`);
  const jsonPath = join(work, `${state.id}.json`);
  await writeFile(scenePath, JSON.stringify({ width, height, icons }));
  const args = ['render', photo, '--icons', scenePath, '--out', framePath, '--json', jsonPath];
  if (!state.solve) args.push(config.fill);
  try {
    await run(binary, args);
  } catch (error) {
    fail(`scooch render failed on the ${state.id} frame, so the engine build at ${binary} is probably too old for this demo. Rebuild it with cargo build -p scooch and run npm run demo again.\n${error.stderr ?? error.message}`);
  }
  const data = JSON.parse(await readFile(jsonPath, 'utf8'));
  const hidden = data.hidden ?? data.placement?.hidden;
  if (typeof hidden !== 'number') fail(`The scooch build at ${binary} does not report hidden in its JSON, so it is too old for this demo. Rebuild it with cargo build -p scooch and run npm run demo again.`);
  return { state, icons, framePath, data, hidden };
}

function check({ state, hidden }) {
  const value = hidden.toFixed(3);
  if (!state.solve && hidden < config.limits.before) fail(`The ${state.id} frame hides only ${value} of the subject, below ${config.limits.before}, so the problem frame would not show icons on the subject.`);
  if (state.solve && hidden > config.limits.after) fail(`The ${state.id} frame still hides ${value} of the subject, above ${config.limits.after}, so it would undercut the pitch.`);
}

async function write({ state, icons, framePath, data, hidden }) {
  const { placement, analysis } = data;
  await sharp(framePath).webp({ quality: 86, effort: 6 }).toFile(join(out, `${state.id}.webp`));
  await onScreen(analysis, placement).toFile(join(out, `${state.id}.saliency.png`));
  return {
    frame: `${state.id}.webp`,
    saliency: `${state.id}.saliency.png`,
    average: await average(framePath),
    hidden: round(hidden),
    flat: !analysis.saliency.some((value) => value > 0),
    solved: state.solve,
    zoom: round(placement.zoom),
    mirrored: placement.mirrored,
    subject: subjectOnScreen(analysis, placement),
    icons: Object.fromEntries(
      config.icons.map((icon, index) => {
        const [x, y, w, h] = icons[index];
        return [icon.id, [x / width, y / height, w / width, h / height].map(round)];
      }),
    ),
  };
}

await mkdir(work, { recursive: true });
const layouts = { ...config.layouts };
const rendered = [];
for (const state of config.states) {
  if (state.layout === 'dropped') {
    const fixed = rendered.find((item) => item.state.id === 'fixed');
    layouts.dropped = dropped(layouts.start, subjectOnScreen(fixed.data.analysis, fixed.data.placement));
  }
  const result = await render(state, layouts[state.layout]);
  console.log(state.id, 'hidden', result.hidden.toFixed(3), 'zoom', round(result.data.placement.zoom));
  rendered.push(result);
}
for (const result of rendered) check(result);
await mkdir(out, { recursive: true });
const frames = {};
for (const result of rendered) frames[result.state.id] = await write(result);
const normalize = ([x, y]) => [round(x / width), round(y / height)];
const manifest = {
  screen: config.screen,
  icons: config.icons,
  frames,
  drag: {
    icon: config.drag.icon,
    from: normalize(layouts.start[config.drag.icon]),
    to: normalize(layouts.dropped[config.drag.icon]),
  },
};
await writeFile(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2));
console.log('wrote', Object.keys(frames).length, 'frames to', out);
