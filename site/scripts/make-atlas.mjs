import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import sharp from 'sharp';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(site, 'package.json'));
const config = JSON.parse(await readFile(join(site, 'scripts', 'demo.config.json'), 'utf8'));
const tokens = await readFile(join(site, 'src', 'styles', 'tokens.css'), 'utf8');
const color = (name) => tokens.match(new RegExp(`--color-${name}:\\s*(#[0-9a-f]{6})`, 'i'))[1];

const glyphs = {
  folder: 'folder-simple',
  'file-text': 'file-text',
  note: 'note',
  'file-image': 'file-image',
};

async function svg(name, weight) {
  const suffix = weight === 'fill' ? '-fill' : '';
  const path = require.resolve(`@phosphor-icons/core/assets/${weight}/${name}${suffix}.svg`);
  return readFile(path, 'utf8');
}

const font = await readFile(require.resolve('@fontsource-variable/instrument-sans/files/instrument-sans-latin-wdth-normal.woff2'));
const icons = [];
for (const icon of config.icons) {
  icons.push({
    label: icon.label,
    folder: icon.glyph === 'folder',
    fill: await svg(glyphs[icon.glyph], 'fill'),
    line: await svg(glyphs[icon.glyph], 'regular'),
  });
}
const cursor = await svg('cursor', 'fill');

const browser = await chromium.launch({ channel: 'chrome', headless: true });
const page = await browser.newPage();
await page.setContent(`<style>@font-face{font-family:Label;src:url(data:font/woff2;base64,${font.toString('base64')}) format("woff2");font-weight:400 700;}</style><p style="font-family:Label">x</p>`);
const data = await page.evaluate(
  async ({ icons, cursor, palette }) => {
    await document.fonts.load('600 30px Label');
    const cell = 256;
    const canvas = document.createElement('canvas');
    canvas.width = cell * 4;
    canvas.height = cell * 4;
    const context = canvas.getContext('2d');
    const image = async (source, fill) => {
      const sized = source.replace('<svg ', '<svg width="512" height="512" ').replace(/currentColor/g, fill);
      const element = new Image();
      element.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(sized)}`;
      await element.decode();
      return element;
    };
    const place = (index, row) => [(index % 4) * cell, (row + Math.floor(index / 4)) * cell];
    const glyph = 142;
    for (const [index, icon] of icons.entries()) {
      const [left, top] = place(index, 0);
      const body = await image(icon.fill, icon.folder ? palette.pad : palette.paper);
      context.save();
      context.shadowColor = 'rgba(10, 8, 6, 0.65)';
      context.shadowBlur = 10;
      context.shadowOffsetY = 3;
      context.drawImage(body, left + (cell - glyph) / 2, top + 14, glyph, glyph);
      context.font = '600 30px Label';
      context.textAlign = 'center';
      context.textBaseline = 'alphabetic';
      context.shadowBlur = 6;
      context.shadowOffsetY = 1;
      context.shadowColor = 'rgba(10, 8, 6, 0.9)';
      context.fillStyle = palette.paper;
      context.fillText(icon.label, left + cell / 2, top + 206, cell - 16);
      context.fillText(icon.label, left + cell / 2, top + 206, cell - 16);
      context.restore();
      const [lineLeft, lineTop] = place(index, 2);
      const line = await image(icon.line, palette.pad);
      context.drawImage(line, lineLeft + (cell - glyph) / 2, lineTop + 14, glyph, glyph);
      context.font = '500 30px Label';
      context.textAlign = 'center';
      context.fillStyle = palette.pad;
      context.fillText(icon.label, lineLeft + cell / 2, lineTop + 206, cell - 16);
    }
    const [left, top] = place(icons.length, 0);
    const size = 132;
    const outline = await image(cursor, palette.ink);
    const body = await image(cursor, palette.paper);
    context.save();
    context.shadowColor = 'rgba(10, 8, 6, 0.5)';
    context.shadowBlur = 8;
    context.shadowOffsetY = 4;
    context.drawImage(outline, left, top, size, size);
    context.restore();
    context.drawImage(body, left + size * 0.06, top + size * 0.06, size * 0.88, size * 0.88);
    return canvas.toDataURL('image/png');
  },
  { icons, cursor, palette: { pad: color('pad'), paper: color('paper'), ink: color('ink') } },
);
await browser.close();
const png = Buffer.from(data.split(',')[1], 'base64');
const out = join(site, 'public', 'demo', 'atlas.png');
await writeFile(out, await sharp(png).png({ compressionLevel: 9, adaptiveFiltering: true }).toBuffer());
console.log('wrote', out);
