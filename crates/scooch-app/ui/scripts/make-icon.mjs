import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright-core';

const root = resolve(import.meta.dirname, '..');
const site = resolve(root, '../../../site');
const out = resolve(process.argv[2] ?? join(root, '..', 'icons', 'source'));
const brand = await readFile(join(site, 'scripts', 'make-brand.mjs'), 'utf8');
const markup = brand.match(/iconPage\.setContent\(`([\s\S]*?)`\);/)?.[1];
if (!markup) throw new Error('The icon mark was not found in site/scripts/make-brand.mjs.');
const tokens = await readFile(join(site, 'src', 'styles', 'tokens.css'), 'utf8');
const color = (name) => tokens.match(new RegExp(`--color-${name}:\\s*(#[0-9a-f]{6})`, 'i'))[1];
const font = await readFile(createRequire(join(root, 'package.json')).resolve('@fontsource-variable/anybody/files/anybody-latin-wdth-normal.woff2'));
const page = new Function('font', 'color', `return \`${markup}\`;`)(font, color);

const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  const context = await browser.newContext({ viewport: { width: 512, height: 512 }, deviceScaleFactor: 2 });
  const tab = await context.newPage();
  await tab.setContent(page);
  await tab.evaluate(() => document.fonts.ready);
  const mark = await tab.screenshot({ type: 'png', omitBackground: true });
  await writeFile(`${out}.png`, mark);
  const framed = await browser.newContext({ viewport: { width: 1024, height: 1024 }, deviceScaleFactor: 1 });
  const frame = await framed.newPage();
  await frame.setContent(`<!doctype html><style>html,body{margin:0;width:1024px;height:1024px;background:transparent;display:grid;place-items:center}img{width:824px;height:824px}</style><img src="data:image/png;base64,${mark.toString('base64')}">`);
  await frame.waitForFunction(() => document.images[0]?.complete);
  await writeFile(`${out}-macos.png`, await frame.screenshot({ type: 'png', omitBackground: true }));
  console.log(`Wrote ${out}.png and ${out}-macos.png`);
} finally {
  await browser.close();
}
