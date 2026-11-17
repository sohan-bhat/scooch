import { mkdir, readdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';
import { launch, origin } from './browser.mjs';
import { juiceColors, round, simplify } from './juice.mjs';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(site, 'public', 'stills');
const strips = [
  { name: 'desktop', width: 2765, height: 1080, widths: [2765, 4608, 5530] },
  { name: 'square', width: 1440, height: 1500, widths: [1440, 2048, 2880] },
  { name: 'phone', width: 792, height: 932, widths: [792, 1188, 1584], phone: true },
];
const SCALE = 2;

async function shoot(browser, strip, empty) {
  const context = await browser.newContext({ viewport: { width: strip.width, height: strip.height }, deviceScaleFactor: SCALE, ...(strip.phone ? { isMobile: true, hasTouch: true } : {}) });
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

await mkdir(out, { recursive: true });
for (const name of await readdir(out)) if (/^hero-/.test(name)) await rm(join(out, name));
const browser = await launch();
const juice = {};
try {
  for (const strip of strips) {
    const empty = await shoot(browser, strip, true);
    for (const width of strip.widths) {
      const image = sharp(empty.shot).resize({ width, kernel: 'lanczos3' });
      await writeFile(join(out, `hero-${strip.name}-${width}.avif`), await image.clone().avif({ quality: 50, effort: 6 }).toBuffer());
      await writeFile(join(out, `hero-${strip.name}-${width}.webp`), await image.clone().webp({ quality: 82, effort: 6 }).toBuffer());
    }
    const full = await shoot(browser, strip, false);
    const shape = empty.shape;
    const colors = await juiceColors(full.shot, shape, SCALE);
    const hull = simplify(shape.hull.map(([x, y]) => [x * SCALE, y * SCALE]), 0.6).map(([x, y]) => `${Math.round(x)} ${Math.round(y)}`);
    const top = { cx: round(shape.top.cx * SCALE), cy: round(shape.top.cy * SCALE), rx: round(shape.top.rx * SCALE), ry: round(shape.top.ry * SCALE) };
    const bottom = round(shape.bottom * SCALE);
    const xs = shape.hull.map(([x]) => x * SCALE);
    juice[strip.name] = {
      width: strip.width * SCALE,
      height: strip.height * SCALE,
      hull: `M${hull.join('L')}Z`,
      left: round(Math.min(...xs)),
      right: round(Math.max(...xs)),
      top,
      bottom,
      drop: round(bottom - (top.cy - top.ry)),
      ...colors,
    };
    console.log('poster', strip.name, strip.widths.join(' '), JSON.stringify(colors));
  }
  await writeFile(join(site, 'src', 'home', 'juice.json'), `${JSON.stringify(juice, null, 2)}\n`);
} finally {
  await browser.close();
}
