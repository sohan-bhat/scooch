import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';
import { launch } from './browser.mjs';
import { compare, liveShots, restPlaces, sharpSet } from './stills-gate.mjs';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(site, 'public', 'stills');
const BEATS = 11;
const sizes = [
  { name: 'desktop', width: 1600, height: 1000, scale: 2, crop: null, files: [{ suffix: '-2x', width: 3200 }, { suffix: '', width: 1600 }], proof: true },
  { name: 'phone', width: 390, height: 844, scale: 2, crop: { width: 780, height: 940 }, files: [{ suffix: '', width: 780 }], proof: false },
];

const only = process.argv.find((arg) => arg.startsWith('--only='))?.slice(7).split(',');
const encode = (image) => image.webp({ quality: 78, effort: 6 }).toBuffer();

await mkdir(out, { recursive: true });
const places = (await restPlaces(site)).slice(0, BEATS);
const set = await sharpSet(site);
const failures = [];
const browser = await launch();
try {
  for (const size of sizes) {
    if (only && !only.includes(size.name)) continue;
    const view = (buffer, width) => {
      let image = sharp(buffer);
      if (size.crop) image = image.extract({ left: 0, top: 0, ...size.crop });
      return width === (size.crop?.width ?? size.width * size.scale) ? image : image.resize(width);
    };
    const shots = await liveShots(browser, size, places, size.proof ? set : null);
    const written = [];
    for (let beat = 0; beat < BEATS; beat++) {
      for (const file of size.files) {
        const path = join(out, `${size.name}-${beat + 1}${file.suffix}.webp`);
        const shot = await view(shots[beat], file.width).png().toBuffer();
        const encoded = await encode(sharp(shot));
        await writeFile(path, encoded);
        written.push({ beat, file, shot });
        console.log('wrote', path, `${Math.round(encoded.length / 1024)} KB`);
      }
    }
    const live = await liveShots(browser, size, places, size.proof ? set : null);
    for (const { beat, file, shot } of written) {
      const reference = await view(live[beat], file.width).png().toBuffer();
      const result = await compare(shot, reference);
      const line = `${size.name}${file.suffix} beat ${beat + 1} against live at rest: worst region ${result.region.toFixed(2)}, largest energy loss ${(result.loss * 100).toFixed(1)} percent at ${result.cell}, ${result.pass ? 'pass' : 'FAIL'}`;
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
