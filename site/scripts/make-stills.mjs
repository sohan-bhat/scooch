import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';
import { launch, openStill } from './browser.mjs';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(site, 'public', 'stills');
const sizes = [
  { name: 'desktop', width: 1600, height: 1000, scale: 1, crop: null },
  { name: 'phone', width: 390, height: 844, scale: 2, crop: { width: 780, height: 940 } },
];

const heroes = [
  { name: 'hero-desktop', width: 2304, height: 900, scale: 1.5, quality: 80 },
  { name: 'hero-square', width: 1280, height: 1278, scale: 1.5, quality: 80 },
  { name: 'hero-phone', width: 720, height: 847, scale: 2, quality: 80 },
];

const heroOnly = process.argv.includes('--hero');

await mkdir(out, { recursive: true });
const browser = await launch();
try {
  for (const hero of heroes) {
    const { context, page } = await openStill(browser, { beat: 0, hero: true, ...hero });
    const shot = await page.screenshot({ type: 'png' });
    await context.close();
    const file = join(out, `${hero.name}.webp`);
    await writeFile(file, await sharp(shot).webp({ quality: hero.quality, effort: 6 }).toBuffer());
    console.log('wrote', file);
  }
  for (const size of heroOnly ? [] : sizes) {
    for (let beat = 0; beat < 7; beat++) {
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
