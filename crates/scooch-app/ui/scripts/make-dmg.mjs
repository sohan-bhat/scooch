import { execFileSync } from 'node:child_process';
import { readFile, rm } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright-core';

const root = resolve(import.meta.dirname, '..');
const site = resolve(root, '../../../site');
const out = resolve(root, '..', 'macos');
const size = { width: 660, height: 400 };
const tokens = await readFile(join(site, 'src', 'styles', 'tokens.css'), 'utf8');
const color = (name) => tokens.match(new RegExp(`--color-${name}:\\s*(#[0-9a-f]{6})`, 'i'))[1];
const hex = (value) => value.match(/[0-9a-f]{2}/gi).map((pair) => parseInt(pair, 16));
const linear = (channel) => {
  const value = channel / 255;
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
};
const luminance = ([r, g, b]) => 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
const steel = hex(color('steel-2'));
const plastic = hex(color('plastic'));
let plate = steel;
for (let share = 0; share <= 1; share += 0.01) {
  const mixed = steel.map((channel, index) => Math.round(channel + (plastic[index] - channel) * share));
  if (luminance(mixed) >= 0.175) {
    plate = mixed;
    break;
  }
}
const light = luminance(plate);
console.log(`label plate rgb(${plate.join(',')}), black text ${((light + 0.05) / 0.05).toFixed(2)} to 1, white text ${(1.05 / (light + 0.05)).toFixed(2)} to 1`);
const copy = JSON.parse(await readFile(join(root, 'src', 'copy', 'en.json'), 'utf8'));
const require = createRequire(join(root, 'package.json'));
const font = async (name) => (await readFile(require.resolve(name))).toString('base64');
const display = await font('@fontsource-variable/anybody/files/anybody-latin-wdth-normal.woff2');
const text = await font('@fontsource-variable/instrument-sans/files/instrument-sans-latin-wdth-normal.woff2');

const page = `<!doctype html><style>
  @font-face{font-family:Display;src:url(data:font/woff2;base64,${display}) format("woff2");font-weight:100 900;font-stretch:50% 150%}
  @font-face{font-family:Text;src:url(data:font/woff2;base64,${text}) format("woff2");font-weight:400 700;font-stretch:75% 100%}
  html,body{margin:0;width:${size.width}px;height:${size.height}px;overflow:hidden}
  body{position:relative;background:radial-gradient(110% 85% at 50% 48%,${color('room-2')},${color('room')} 72%)}
  h1{position:absolute;left:36px;top:26px;margin:0;font-family:Display;font-weight:800;font-stretch:122%;font-size:30px;letter-spacing:-0.045em;line-height:1;color:${color('paper')}}
  i{position:absolute;top:281px;width:132px;height:26px;margin-left:-66px;border-radius:13px;background:rgb(${plate.join(',')})}
  p{position:absolute;left:0;right:0;bottom:34px;margin:0;text-align:center;font-family:Text;font-weight:600;font-size:15px;line-height:1.3;color:${color('plastic')}}
</style><h1>${copy.name}</h1><i style="left:180px"></i><i style="left:480px"></i><p>${copy.dmg.first}</p>`;

const browser = await chromium.launch({ channel: 'chrome', headless: true });
try {
  for (const [scale, file] of [[1, 'dmg-1x.png'], [2, 'dmg-2x.png']]) {
    const context = await browser.newContext({ viewport: size, deviceScaleFactor: scale });
    const tab = await context.newPage();
    await tab.setContent(page);
    await tab.evaluate(() => document.fonts.ready);
    await tab.screenshot({ path: join(out, file), type: 'png' });
    await context.close();
  }
} finally {
  await browser.close();
}
for (const [file, dpi] of [['dmg-1x', 72], ['dmg-2x', 144]]) {
  execFileSync('sips', ['-s', 'format', 'tiff', '-s', 'formatOptions', 'lzw', '-s', 'dpiWidth', String(dpi), '-s', 'dpiHeight', String(dpi), join(out, `${file}.png`), '--out', join(out, `${file}.tiff`)], { stdio: 'ignore' });
}
execFileSync('tiffutil', ['-cat', join(out, 'dmg-1x.tiff'), join(out, 'dmg-2x.tiff'), '-out', join(out, 'dmg-background.tiff')], { stdio: 'inherit' });
for (const file of ['dmg-1x.png', 'dmg-2x.png', 'dmg-1x.tiff', 'dmg-2x.tiff']) await rm(join(out, file));
console.log(`Wrote ${join(out, 'dmg-background.tiff')}`);
