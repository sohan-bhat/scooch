import { readdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(site, 'dist');
const pages = [
  { page: 'home', file: 'index.html' },
  { page: 'sources', file: 'sources/index.html' },
  { page: 'missing', file: '404.html' },
];

const server = await createServer({ root: site, configFile: join(site, 'vite.config.ts'), server: { middlewareMode: true, hmr: false }, appType: 'custom', logLevel: 'error' });
try {
  const { render } = await server.ssrLoadModule('/src/ssr.tsx');
  const assets = await readdir(join(dist, 'assets'));
  const fonts = assets.filter((name) => /^(anybody|instrument-sans)-latin-wdth-normal-.*\.woff2$/.test(name));
  const preload = fonts.map((name) => `<link rel="preload" as="font" type="font/woff2" crossorigin href="/scooch/assets/${name}" />`).join('\n    ');
  for (const { page, file } of pages) {
    const path = join(dist, file);
    const html = await readFile(path, 'utf8');
    if (!html.includes('<div id="root"></div>')) throw new Error(`${file} has no empty root`);
    const body = render(page);
    const next = html.replace('<div id="root"></div>', `<div id="root">${body}</div>`).replace('</title>', `</title>\n    ${preload}`);
    await writeFile(path, next);
    console.log('prerendered', file, `${Math.round(body.length / 1024)} kB`);
  }
} finally {
  await server.close();
}
