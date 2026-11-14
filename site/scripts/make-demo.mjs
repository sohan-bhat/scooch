import { execFile } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { gzipSync } from 'node:zlib';
import sharp from 'sharp';

const run = promisify(execFile);
const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const config = JSON.parse(await readFile(join(site, 'scripts', 'demo.config.json'), 'utf8'));
const binary = resolve(site, process.env.SCOOCH_BIN ?? config.binary);
const bundler = resolve(site, process.env.SCOOCH_BUNDLE ?? config.bundle);
const photo = resolve(site, config.photo);
const work = join(site, '.cache', 'demo');
const out = join(site, 'public', 'demo');
let { width, height } = config.screen;
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

async function render(state, layout, source = photo, prefix = '') {
  const icons = rects(layout);
  const scenePath = join(work, `${prefix}${state.id}.scene.json`);
  const framePath = join(work, `${prefix}${state.id}.jpg`);
  const jsonPath = join(work, `${prefix}${state.id}.json`);
  await writeFile(scenePath, JSON.stringify({ width, height, icons }));
  const args = ['render', source, '--icons', scenePath, '--out', framePath, '--json', jsonPath];
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

async function writeSaliency(analysis, file) {
  const { width: w, height: h } = analysis;
  const pixels = Buffer.alloc(w * h);
  for (let i = 0; i < w * h; i++) pixels[i] = Math.round(Math.min(Math.max(analysis.saliency[i], 0), 1) * 255);
  await sharp(pixels, { raw: { width: w, height: h, channels: 1 } }).png({ compressionLevel: 9 }).toFile(join(out, file));
  return file;
}

async function writePhoto(results, source = photo, file = 'photo.webp') {
  const span = (table) => Math.abs(table[table.length - 1] - table[0]);
  const spanX = Math.min(...results.map((result) => span(result.data.placement.fx)));
  const spanY = Math.min(...results.map((result) => span(result.data.placement.fy)));
  const meta = await sharp(source).rotate().metadata();
  const needed = Math.max(width / (spanX * meta.width), height / (spanY * meta.height));
  const limit = 2048 / Math.max(meta.width, meta.height);
  const scale = Math.min(1, needed * 1.25, limit);
  const size = { width: Math.round(meta.width * scale), height: Math.round(meta.height * scale) };
  await sharp(source).rotate().resize(size.width, size.height).webp({ quality: 88, effort: 6 }).toFile(join(out, file));
  return { file, ...size };
}

function tables(results) {
  return Object.fromEntries(
    results.map(({ state, data }) => [state.id, { fx: data.placement.fx.map(round), fy: data.placement.fy.map(round) }]),
  );
}

function objects(layout) {
  return { width, height, icons: rects(layout).map(([x, y, w, h]) => ({ x, y, w, h })) };
}

async function scenery(id, source, layouts) {
  const folder = join(work, `scenery-${id}`);
  const target = join(out, 'scenery', id);
  await mkdir(folder, { recursive: true });
  await mkdir(target, { recursive: true });
  const files = [];
  for (const [index, layout] of layouts.entries()) {
    const file = join(folder, `layout-${index}.json`);
    await writeFile(file, JSON.stringify(objects(layout)));
    files.push(file);
  }
  try {
    await run(bundler, [source, ...files, folder]);
  } catch (error) {
    fail(`The scenery bundle export failed for ${id}. Build it with cargo build --release -p scooch-core --example bundle.\n${error.stderr ?? error.message}`);
  }
  const bundle = JSON.parse(await readFile(join(folder, 'bundle.json'), 'utf8'));
  const longest = 1600;
  const canvas = sharp(join(folder, bundle.canvas.file)).resize({ width: longest, height: longest, fit: 'inside', withoutEnlargement: true }).jpeg({ quality: 74, mozjpeg: true });
  const info = await canvas.toFile(join(target, bundle.canvas.file));
  bundle.canvas.width = info.width;
  bundle.canvas.height = info.height;
  for (const state of bundle.states) {
    const tables = state.tables;
    for (const key of ['map', 'look', 'pad', 'paints', 'ramps']) await writeFile(join(target, tables[key].file), await readFile(join(folder, tables[key].file)));
    tables.field.file = `${tables.field.file}.gz`;
    await writeFile(join(target, tables.field.file), gzipSync(await readFile(join(folder, tables.field.file.slice(0, -3))), { level: 9 }));
  }
  return { folder: `scenery/${id}`, ...bundle };
}

async function fillMap(id, placement) {
  const long = Math.max(width, height);
  const map = new Float32Array(long * 2);
  for (let i = 0; i < long; i++) {
    map[i] = lookup(placement.fx, (Math.min(i, width - 1) + 0.5) / width);
    map[long + i] = lookup(placement.fy, (Math.min(i, height - 1) + 0.5) / height);
  }
  await writeFile(join(out, 'scenery', id, 'fill.map.bin'), Buffer.from(map.buffer));
  return { file: 'fill.map.bin', format: 'f32le', width: long, height: 2 };
}

async function picture(entry) {
  const screen = entry.screen ?? config.screen;
  width = screen.width;
  height = screen.height;
  try {
    return await pictureAt(entry);
  } finally {
    ({ width, height } = config.screen);
  }
}

async function pictureAt(entry) {
  const source = resolve(site, entry.photo);
  const states = [
    { id: 'before', layout: entry.layout, solve: false },
    { id: 'fixed', layout: entry.layout, solve: true },
  ];
  const results = [];
  for (const state of states) {
    const result = await render(state, config.layouts[entry.layout], source, `${entry.id}-`);
    console.log(entry.id, state.id, 'hidden', result.hidden.toFixed(3), 'zoom', round(result.data.placement.zoom));
    results.push(result);
  }
  for (const result of results) if (result.state.solve || entry.problem !== false) check(result);
  return {
    id: entry.id,
    screen: { width, height },
    order: states.map((state) => state.id),
    tables: tables(results),
    frames: Object.fromEntries(
      await Promise.all(
        results.map(async ({ state, framePath, hidden, data }) => [
          state.id,
          { average: await average(framePath), hidden: round(hidden), subject: subjectOnScreen(data.analysis, data.placement) },
        ]),
      ),
    ),
    icons: Object.fromEntries(config.icons.map((icon, index) => [icon.id, rects(config.layouts[entry.layout])[index].map((value, at) => round(value / (at % 2 === 0 ? width : height)))])),
    scenery: await scenery(entry.id, source, [config.layouts[entry.layout]]),
  };
}

await mkdir(work, { recursive: true });
await mkdir(out, { recursive: true });
if (process.argv.includes('--saliency')) {
  const manifest = JSON.parse(await readFile(join(out, 'manifest.json'), 'utf8'));
  const cached = JSON.parse(await readFile(join(work, 'before.json'), 'utf8'));
  manifest.photo.saliency = await writeSaliency(cached.analysis, 'photo.saliency.png');
  await writeFile(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2));
  process.exit(0);
}
if (process.argv.includes('--pictures')) {
  const manifest = JSON.parse(await readFile(join(out, 'manifest.json'), 'utf8'));
  manifest.pictures = [];
  for (const entry of config.pictures ?? []) manifest.pictures.push(await picture(entry));
  await writeFile(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2));
  process.exit(0);
}
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
  photo: { ...(await writePhoto(rendered)), saliency: await writeSaliency(rendered[0].data.analysis, 'photo.saliency.png') },
  order: rendered.map((result) => result.state.id),
  tables: tables(rendered),
  frames,
  drag: {
    icon: config.drag.icon,
    from: normalize(layouts.start[config.drag.icon]),
    to: normalize(layouts.dropped[config.drag.icon]),
  },
};
const before = rendered.find((result) => result.state.id === 'before');
manifest.scenery = await scenery('main', photo, [layouts.start, layouts.dropped]);
manifest.scenery.fill = await fillMap('main', before.data.placement);
manifest.pictures = [];
for (const entry of config.pictures ?? []) manifest.pictures.push(await picture(entry));
await writeFile(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2));
console.log('wrote', Object.keys(frames).length, 'frames to', out);
