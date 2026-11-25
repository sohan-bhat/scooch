import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright-core';

const root = resolve(import.meta.dirname, '..');
const site = resolve(root, '../../../site');
const out = resolve(process.argv[2] ?? join(root, '..', 'icons', 'tray-template.png'));
const height = 36;
const room = 2;
const brand = await readFile(join(site, 'scripts', 'make-brand.mjs'), 'utf8');
const markup = brand.match(/iconPage\.setContent\(`([\s\S]*?)`\);/)?.[1];
if (!markup) throw new Error('The icon mark was not found in site/scripts/make-brand.mjs.');
const font = await readFile(createRequire(join(root, 'package.json')).resolve('@fontsource-variable/anybody/files/anybody-latin-wdth-normal.woff2'));
const ink = (name) => (name === 'room' ? 'transparent' : '#000000');
const page = new Function('font', 'color', `return \`${markup}\`;`)(font, ink);

const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const context = await browser.newContext({ viewport: { width: 512, height: 512 }, deviceScaleFactor: 4 });
  const tab = await context.newPage();
  await tab.setContent(page);
  await tab.evaluate(() => document.fonts.ready);
  const mark = await tab.screenshot({ type: 'png', omitBackground: true });
  const small = await tab.evaluate(
    async ({ data, height, room }) => {
      const image = new Image();
      image.src = `data:image/png;base64,${data}`;
      await image.decode();
      let canvas = new OffscreenCanvas(image.width, image.height);
      canvas.getContext('2d').drawImage(image, 0, 0);
      const pixels = canvas.getContext('2d').getImageData(0, 0, image.width, image.height).data;
      let [left, top, right, bottom] = [image.width, image.height, 0, 0];
      for (let y = 0; y < image.height; y++) {
        for (let x = 0; x < image.width; x++) {
          if (pixels[(y * image.width + x) * 4 + 3] > 8) {
            left = Math.min(left, x);
            top = Math.min(top, y);
            right = Math.max(right, x);
            bottom = Math.max(bottom, y);
          }
        }
      }
      let [x, y, w, h] = [left, top, right - left + 1, bottom - top + 1];
      const inner = height - room * 2;
      while (h / 2 > inner) {
        const half = new OffscreenCanvas(Math.ceil(w / 2), Math.ceil(h / 2));
        const pen = half.getContext('2d');
        pen.imageSmoothingQuality = 'high';
        pen.drawImage(canvas, x, y, w, h, 0, 0, half.width, half.height);
        canvas = half;
        [x, y, w, h] = [0, 0, half.width, half.height];
      }
      const width = Math.round((w * inner) / h) + room * 2;
      const done = new OffscreenCanvas(width, height);
      const pen = done.getContext('2d');
      pen.imageSmoothingQuality = 'high';
      pen.drawImage(canvas, x, y, w, h, room, room, width - room * 2, inner);
      const bytes = new Uint8Array(await (await done.convertToBlob({ type: 'image/png' })).arrayBuffer());
      let text = '';
      for (const byte of bytes) text += String.fromCharCode(byte);
      return btoa(text);
    },
    { data: mark.toString('base64'), height, room },
  );
  await writeFile(out, Buffer.from(small, 'base64'));
  console.log(`Wrote ${out}`);
} finally {
  await browser.close();
}
