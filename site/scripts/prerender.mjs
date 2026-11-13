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
  const manifest = JSON.parse(await readFile(join(dist, 'demo', 'manifest.json'), 'utf8'));
  const stage = assets.find((name) => /^Stage-.*\.js$/.test(name));
  const story = '(prefers-reduced-motion: no-preference)';
  const wide = '(min-width: 768px) and (orientation: landscape) and (min-aspect-ratio: 1601/1000)';
  const square = '(min-width: 768px) and (orientation: landscape) and (max-aspect-ratio: 16/10)';
  const tall = '(max-width: 767px), (orientation: portrait)';
  const early = [
    ['/scooch/demo/manifest.json', 'high'],
    [`/scooch/demo/${manifest.scenery.folder}/${manifest.scenery.fill.file}`, 'high'],
    ...['laptop', 'desk', 'lamp', 'television'].map((model) => [`/scooch/models/lite/${model}.glb`, 'high']),
    ...['basis_transcoder.js', 'basis_transcoder.wasm'].map((file) => [`/scooch/basis/${file}`, 'high']),
    ['/scooch/hdri/studio.hdr', 'low'],
  ];
  const images = [manifest.photo.saliency, `${manifest.scenery.folder}/${manifest.scenery.canvas.file}`, 'atlas.png'];
  const scene = [
    `<link rel="preload" as="image" href="/scooch/stills/hero-desktop.webp" media="${wide}" fetchpriority="high" />`,
    `<link rel="preload" as="image" href="/scooch/stills/hero-square.webp" media="${square}" fetchpriority="high" />`,
    `<link rel="preload" as="image" href="/scooch/stills/hero-phone.webp" media="${tall}" fetchpriority="high" />`,
    stage ? `<link rel="modulepreload" crossorigin href="/scooch/assets/${stage}" media="${story}" fetchpriority="high" />` : '',
    `<script>if(!/[?&]stills/.test(location.search)&&matchMedia('${story}').matches){var e=window.__early=new Map();${JSON.stringify(early)}.forEach(function(f){var u=new URL(f[0],location.href).href;e.set(u,fetch(u,{credentials:'same-origin',priority:f[1]}).catch(function(){return null}))})}</script>`,
    ...images.map((file) => `<link rel="preload" as="image" crossorigin href="/scooch/demo/${file}" media="${story}" fetchpriority="low" />`),
  ]
    .filter(Boolean)
    .join('\n    ');
  for (const { page, file } of pages) {
    const path = join(dist, file);
    const html = await readFile(path, 'utf8');
    if (!html.includes('<div id="root"></div>')) throw new Error(`${file} has no empty root`);
    const body = render(page);
    const hints = page === 'home' ? `${preload}\n    ${scene}` : preload;
    const next = html.replace('<div id="root"></div>', `<div id="root">${body}</div>`).replace('</title>', `</title>\n    ${hints}`);
    await writeFile(path, next);
    console.log('prerendered', file, `${Math.round(body.length / 1024)} kB`);
  }
} finally {
  await server.close();
}
