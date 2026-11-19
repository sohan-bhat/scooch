import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';
import { launch } from './browser.mjs';
import { compare, liveShots, restPlaces } from './stills-gate.mjs';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(site, 'public', 'stills');
const BEATS = 11;
const sizes = [
  { name: 'desktop', width: 1600, height: 1000, scale: 1, crop: null },
  { name: 'desktop', suffix: '-2x', width: 1600, height: 1000, scale: 2, crop: null },
  { name: 'phone', width: 390, height: 844, scale: 2, crop: { width: 780, height: 940 } },
];

const only = process.argv.find((arg) => arg.startsWith('--only='))?.slice(7).split(',');

await mkdir(out, { recursive: true });
const places = await restPlaces(site);
const failures = [];
const browser = await launch();
try {
  for (const size of sizes) {
    const name = `${size.name}${size.suffix ?? ''}`;
    if (only && !only.includes(name)) continue;
    const files = [];
    const shots = await liveShots(browser, size, places.slice(0, BEATS));
    for (let beat = 0; beat < BEATS; beat++) {
      let image = sharp(shots[beat]);
      if (size.crop) image = image.extract({ left: 0, top: 0, ...size.crop });
      const file = join(out, `${size.name}-${beat + 1}${size.suffix ?? ''}.webp`);
      const encoded = await image.webp({ quality: 78, effort: 6 }).toBuffer();
      await writeFile(file, encoded);
      files.push(encoded);
      console.log('wrote', file, `${Math.round(encoded.length / 1024)} KB`);
    }
    const live = await liveShots(browser, size, places.slice(0, BEATS));
    for (let beat = 0; beat < BEATS; beat++) {
      const view = size.crop ? await sharp(live[beat]).extract({ left: 0, top: 0, ...size.crop }).png().toBuffer() : live[beat];
      const result = await compare(files[beat], view);
      const line = `${name} beat ${beat + 1} against live at rest: worst region ${result.region.toFixed(2)}, largest energy loss ${(result.loss * 100).toFixed(1)} percent at ${result.cell}, ${result.pass ? 'pass' : 'FAIL'}`;
      console.log(line);
      if (!result.pass) failures.push(line);
    }
  }
} finally {
  await browser.close();
}
if (failures.length) {
  console.error(`stills gate failed on ${failures.length}:\n${failures.join('\n')}`);
  process.exitCode = 1;
}
