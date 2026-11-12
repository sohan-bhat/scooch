import { chromium } from 'playwright-core';

export const origin = process.env.SITE_URL ?? 'http://localhost:4173/scooch/';

export function launch() {
  return chromium.launch({
    channel: 'chrome',
    headless: true,
    args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--enable-gpu-rasterization'],
  });
}

export async function openStill(browser, { beat, width, height, scale, share = false }) {
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: scale });
  const page = await context.newPage();
  page.on('pageerror', (error) => console.error('pageerror', error.message));
  const query = `still=${beat}${share ? '&share' : ''}`;
  await page.goto(`${origin}?${query}`, { waitUntil: 'load' });
  await page.waitForFunction(() => window.__stillReady === true, null, { timeout: 60000 });
  await page.evaluate(() => document.fonts.ready);
  return { context, page };
}
