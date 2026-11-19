import { execFileSync } from 'node:child_process';
import { mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';
import { launch, origin } from './browser.mjs';
import { encodeLowest } from './stills-gate.mjs';
import { juiceColors, round, simplify } from './juice.mjs';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(site, 'public', 'stills');
const strips = [
  { name: 'desktop-900', width: 2304, height: 900, ratios: [1, 2] },
  { name: 'desktop-1080', width: 2765, height: 1080, ratios: [1, 2], juice: 'desktop' },
  { name: 'square', width: 1440, height: 1500, ratios: [1, 2], juice: 'square' },
  { name: 'phone', width: 792, height: 932, ratios: [2], phone: true, juice: 'phone' },
];

async function shoot(browser, strip, empty, ratio) {
  const context = await browser.newContext({ viewport: { width: strip.width, height: strip.height }, deviceScaleFactor: ratio, ...(strip.phone ? { isMobile: true, hasTouch: true } : {}) });
  const page = await context.newPage();
  page.on('pageerror', (error) => console.error('pageerror', error.message));
  await page.goto(`${origin}?poster${empty ? '&hero' : ''}`, { waitUntil: 'load' });
  await page.waitForFunction(() => performance.getEntriesByName('scooch:swap').length > 0, null, { timeout: 180000, polling: 100 });
  await page.waitForTimeout(500);
  const shot = await page.screenshot({ type: 'png' });
  const shape = empty ? await page.evaluate(() => window.__juice()) : null;
  await context.close();
  return { shot, shape };
}

const turn = () => process.env.SHOOT_YIELD && execFileSync(process.env.SHOOT_YIELD, { stdio: 'inherit' });
const only = process.argv.find((arg) => arg.startsWith('--only='))?.slice(7).split(',');
const chosen = strips.filter((strip) => !only || only.includes(strip.name));
await mkdir(out, { recursive: true });
for (const name of await readdir(out)) if (/^hero-/.test(name) && chosen.some((strip) => name.startsWith(`hero-${strip.name}-`))) await rm(join(out, name));
const failures = [];
const browser = await launch();
const juice = only ? JSON.parse(await readFile(join(site, 'src', 'home', 'juice.json'), 'utf8')) : {};
try {
  for (const strip of chosen) {
    let shape = null;
    for (const ratio of strip.ratios) {
      turn();
      const empty = await shoot(browser, strip, true, ratio);
      shape = empty.shape;
      const render = await sharp(empty.shot).png().toBuffer();
      const encoded = await encodeLowest(render, 'avif');
      await writeFile(join(out, `hero-${strip.name}-${ratio}x.avif`), encoded.buffer);
      await writeFile(join(out, `hero-${strip.name}-${ratio}x.webp`), await sharp(render).webp({ quality: 85, effort: 6 }).toBuffer());
      const line = `hero-${strip.name}-${ratio}x.avif quality ${encoded.quality}, ${Math.round(encoded.buffer.length / 1024)} KB, against its render: worst region ${encoded.region.toFixed(2)}, largest energy loss ${(encoded.loss * 100).toFixed(1)} percent at ${encoded.cell}, ${encoded.pass ? 'pass' : 'FAIL'}`;
      console.log(line);
      if (!encoded.pass) failures.push(line);
    }
    if (!strip.juice) continue;
    const scale = strip.ratios.at(-1);
    const full = await shoot(browser, strip, false, scale);
    const colors = await juiceColors(full.shot, shape, scale);
    const hull = simplify(shape.hull.map(([x, y]) => [x * scale, y * scale]), 0.6).map(([x, y]) => `${Math.round(x)} ${Math.round(y)}`);
    const top = { cx: round(shape.top.cx * scale), cy: round(shape.top.cy * scale), rx: round(shape.top.rx * scale), ry: round(shape.top.ry * scale) };
    const bottom = round(shape.bottom * scale);
    const xs = shape.hull.map(([x]) => x * scale);
    juice[strip.juice] = {
      width: strip.width * scale,
      height: strip.height * scale,
      hull: `M${hull.join('L')}Z`,
      left: round(Math.min(...xs)),
      right: round(Math.max(...xs)),
      top,
      bottom,
      drop: round(bottom - (top.cy - top.ry)),
      ...colors,
    };
    console.log('poster', strip.name, JSON.stringify(colors));
  }
  await writeFile(join(site, 'src', 'home', 'juice.json'), `${JSON.stringify(juice, null, 2)}\n`);
} finally {
  await browser.close();
}
if (failures.length) {
  console.error(`poster gate failed on ${failures.length}:\n${failures.join('\n')}`);
  process.exitCode = 1;
}
