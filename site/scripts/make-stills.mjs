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

await mkdir(out, { recursive: true });
const browser = await launch();
try {
  for (const size of sizes) {
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
