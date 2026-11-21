import { mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { chromium } from 'playwright-core';
import { createServer } from 'vite';

const root = resolve(import.meta.dirname, '..');
const out = resolve(process.env.SHOTS_DIR ?? join(root, 'shots'));
const only = process.argv.slice(2);
const scheme = process.env.SHOTS_SCHEME ?? 'dark';
mkdirSync(out, { recursive: true });

const sizes = [
  [900, 620],
  [640, 480],
];

const states = [
  ['mac-on'],
  ['mac-two'],
  ['mac-off'],
  ['mac-pending'],
  ['mac-starting'],
  ['mac-flip'],
  ['mac-reduced', { reducedMotion: 'reduce' }],
  ['mac-large-text'],
  ['mac-focus', { state: 'mac-on', tabs: 3, tip: true }],
  ['mac-focus-pill', { state: 'mac-on', tabs: 4 }],
  ['mac-focus-picture', { state: 'mac-on', tabs: 6 }],
  ['mac-drop', { state: 'mac-on', query: '&drop' }],
  ['mac-automation'],
  ['mac-asking'],
  ['mac-finder'],
  ['mac-move'],
  ['mac-slideshow'],
  ['mac-moving'],
  ['mac-stopped'],
  ['mac-other'],
  ['win-on'],
  ['win-off'],
  ['win-video'],
  ['win-admin'],
  ['win-startup'],
  ['win-text-150'],
  ['win-text-200'],
  ['win-text-200-problem'],
  ['gnome-two'],
  ['gnome-icons'],
  ['gnome-missing'],
  ['mac-move-failed'],
  ['gnome-color'],
  ['linux-unknown'],
  ['xfce-on'],
  ['xfce-off'],
  ['no-screen'],
].filter(([name]) => only.length === 0 || only.includes(name));

const server = await createServer({ root, mode: 'mock', logLevel: 'error', server: { port: 5181, strictPort: true } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ channel: 'chrome', headless: true });
  for (const [width, height] of sizes) {
    for (const [name, options = {}] of states) {
      const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2, colorScheme: scheme, reducedMotion: options.reducedMotion ?? 'no-preference' });
      try {
        const page = await context.newPage();
        page.on('pageerror', (error) => console.error(name, error.message));
        await page.goto(`http://localhost:5181/?state=${options.state ?? name}${options.query ?? ''}`, { waitUntil: 'load' });
        await page.waitForSelector('main');
        await page.evaluate(() => document.documentElement.setAttribute('data-still', ''));
        await page.evaluate(() => document.fonts.ready);
        await page.waitForFunction(() =>
          [...document.images]
            .filter((image) => {
              const box = image.getBoundingClientRect();
              return box.right > 0 && box.left < innerWidth && box.bottom > 0 && box.top < innerHeight;
            })
            .every((image) => image.complete && image.naturalWidth > 0)
        );
        for (let step = 0; step < (options.tabs ?? 0); step += 1) await page.keyboard.press('Tab');
        if (options.tip) await page.waitForSelector('[data-slot="tooltip-content"]', { timeout: 5000 }).catch(() => console.error(name, 'no tooltip'));
        const file = join(out, `${name}-${scheme}-${width}x${height}.png`);
        await page.screenshot({ path: file });
        console.log(file);
      } finally {
        await context.close();
      }
    }
  }
} finally {
  await browser?.close();
  await server.close();
}
