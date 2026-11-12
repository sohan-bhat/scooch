import { readFile, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';
import { launch, openStill } from './browser.mjs';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(join(site, 'package.json'));
const tokens = await readFile(join(site, 'src', 'styles', 'tokens.css'), 'utf8');
const color = (name) => tokens.match(new RegExp(`--color-${name}:\\s*(#[0-9a-f]{6})`, 'i'))[1];
const font = await readFile(require.resolve('@fontsource-variable/anybody/files/anybody-latin-wdth-normal.woff2'));
const publicDir = join(site, 'public');

const browser = await launch();
try {
  const { context, page } = await openStill(browser, { beat: 0, width: 1200, height: 630, scale: 1, share: true });
  await page.addStyleTag({ content: '[data-action]{visibility:hidden!important} header{visibility:visible!important} header nav, header [role=progressbar]{visibility:hidden!important}' });
  await page.waitForTimeout(300);
  const share = await page.screenshot({ type: 'png' });
  await context.close();
  let quality = 86;
  let jpeg = await sharp(share).jpeg({ quality, mozjpeg: true }).toBuffer();
  while (jpeg.length > 290000 && quality > 50) {
    quality -= 6;
    jpeg = await sharp(share).jpeg({ quality, mozjpeg: true }).toBuffer();
  }
  await writeFile(join(publicDir, 'share.jpg'), jpeg);
  console.log('share.jpg', Math.round(jpeg.length / 1024), 'kB at quality', quality);

  const iconPage = await browser.newPage({ viewport: { width: 512, height: 512 } });
  await iconPage.setContent(`<!doctype html><style>
    @font-face{font-family:Mark;src:url(data:font/woff2;base64,${font.toString('base64')}) format("woff2");font-weight:100 900;font-stretch:50% 150%}
    html,body{margin:0;width:512px;height:512px;background:transparent}
    div{width:512px;height:512px;border-radius:112px;background:${color('room')};display:grid;place-items:center;overflow:hidden;position:relative}
    span{font-family:Mark;font-weight:800;font-stretch:122%;font-size:420px;line-height:1;letter-spacing:-0.04em;color:${color('paper')};transform:translateY(-14px)}
    i{position:absolute;left:96px;right:96px;bottom:70px;height:22px;border-radius:11px;background:${color('lamp')}}
  </style><div><span>S</span><i></i></div>`);
  await iconPage.evaluate(() => document.fonts.ready);
  const mark = await iconPage.screenshot({ type: 'png', omitBackground: true });
  for (const [file, size] of [['favicon.png', 48], ['icon-192.png', 192], ['apple-touch-icon.png', 180]]) {
    await writeFile(join(publicDir, file), await sharp(mark).resize(size, size).png({ compressionLevel: 9 }).toBuffer());
    console.log('wrote', file);
  }
} finally {
  await browser.close();
}
