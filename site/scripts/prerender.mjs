import { readdir, readFile, stat, writeFile } from 'node:fs/promises';
import { gzipSync } from 'node:zlib';
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
    [`/scooch/demo/${manifest.scenery.folder}/${manifest.scenery.canvas.file}`, 'low'],
    ...['laptop', 'desk', 'lamp', 'television'].map((model) => [`/scooch/models/lite/${model}.glb`, 'high']),
    ['/scooch/models/tumbler.glb', 'high'],
    ...['basis_transcoder.js', 'basis_transcoder.wasm'].map((file) => [`/scooch/basis/${file}`, 'high']),
    ['/scooch/hdri/studio.hdr', 'low'],
  ];
  const images = [manifest.photo.saliency, 'atlas.png'];
  const home = await readFile(join(dist, 'index.html'), 'utf8');
  const entry = home.match(/<script type="module" crossorigin src="\/scooch\/assets\/([^"]+\.js)"/)?.[1];
  const sheets = [...home.matchAll(/<link rel="stylesheet" crossorigin href="\/scooch\/assets\/([^"]+\.css)"/g)].map((match) => match[1]);
  const imports = async (name) => [...(await readFile(join(dist, 'assets', name), 'utf8')).matchAll(/(?:from|import)\s*"\.\/([^"]+\.js)"/g)].map((match) => match[1]);
  const closure = async (start) => {
    const seen = new Set();
    const walk = async (name) => {
      if (!name || seen.has(name)) return;
      seen.add(name);
      for (const next of await imports(name)) await walk(next);
    };
    await walk(start);
    return seen;
  };
  const entryChunks = await closure(entry);
  const stageChunks = [...(await closure(stage))].filter((name) => !entryChunks.has(name));
  const scene = [
    `<link rel="preload" as="image" href="/scooch/stills/hero-desktop.webp" media="${wide}" fetchpriority="high" />`,
    `<link rel="preload" as="image" href="/scooch/stills/hero-square.webp" media="${square}" fetchpriority="high" />`,
    `<link rel="preload" as="image" href="/scooch/stills/hero-phone.webp" media="${tall}" fetchpriority="high" />`,
    ...stageChunks.map((name) => `<link rel="modulepreload" crossorigin href="/scooch/assets/${name}" media="${story}" fetchpriority="high" />`),
    `<script>if(!/[?&]stills/.test(location.search)&&matchMedia('${story}').matches){var e=window.__early=new Map(),g=window.__got={};${JSON.stringify(early)}.forEach(function(f){var u=new URL(f[0],location.href).href;e.set(u,fetch(u,{credentials:'same-origin',priority:f[1]}).then(function(r){if(!r.ok||!r.body)return r;var k=r.body.getReader(),c=[];g[f[0]]=0;return(function p(){return k.read().then(function(x){if(x.done)return new Response(new Blob(c),{status:r.status,statusText:r.statusText,headers:r.headers});c.push(x.value);g[f[0]]+=x.value.length;return p()})})()}).catch(function(){return null}))})}</script>`,
    ...images.map((file) => `<link rel="preload" as="image" crossorigin href="/scooch/demo/${file}" media="${story}" fetchpriority="low" />`),
  ]
    .filter(Boolean)
    .join('\n    ');
  const needed = [
    ...[...entryChunks, ...stageChunks, ...sheets].map((name) => `/scooch/assets/${name}`),
    ...early.map(([path]) => path),
    ...images.map((name) => `/scooch/demo/${name}`),
  ];
  const sizes = {};
  for (const path of needed) {
    const local = join(dist, path.replace(/^\/scooch\//, ''));
    const raw = (await stat(local)).size;
    sizes[path] = [/\.(js|css|json)$/.test(path) ? gzipSync(await readFile(local)).length : raw, raw];
  }
  const loader = `<script>(function(){var d=document.documentElement;if(/[?&]still/.test(location.search)||d.classList.contains('stills-mode')||!matchMedia('${story}').matches)return;var F=${JSON.stringify(sizes)},T=0,k;for(k in F)T+=F[k][0];var G=window.__got||{},D={},L=window.__load={on:true,stage:0,ready:false,shown:0,target:0,at:0,cb:[]};d.classList.add('loading');try{history.scrollRestoration='manual'}catch(x){}var n=performance.getEntriesByType('navigation')[0];try{if(n&&n.type!=='navigate')L.at=+sessionStorage.getItem('scooch:at')||0}catch(x){}addEventListener('pagehide',function(){try{sessionStorage.setItem('scooch:at',L.on?L.at:scrollY)}catch(x){}});function no(e){if(L.on)e.preventDefault()}addEventListener('wheel',no,{passive:false});addEventListener('touchmove',no,{passive:false});addEventListener('keydown',function(e){var t=e.target;if(L.on&&/^( |Spacebar|PageUp|PageDown|End|Home|ArrowUp|ArrowDown)$/.test(e.key)&&!(t&&t.closest&&t.closest('input,textarea,select,button,a,[contenteditable]')))e.preventDefault()});addEventListener('scroll',function(){if(L.on&&scrollY)scrollTo(0,0)});try{new PerformanceObserver(function(l){l.getEntries().forEach(function(x){var p=new URL(x.name).pathname;if(F[p])D[p]=1})}).observe({type:'resource',buffered:true})}catch(x){}var R;L.full=new Promise(function(r){R=r});L.release=function(keep){if(!L.on)return;L.on=false;clearTimeout(L.t);d.classList.remove('loading');if(keep!==false&&L.at)scrollTo(0,L.at);performance.mark('scooch:released');L.cb.forEach(function(f){f(keep!==false)})};L.done=function(){L.ready=true};L.t=setTimeout(function(){if(L.on&&!L.ready){L.dead=true;d.classList.add('stills-mode');L.release(false);dispatchEvent(new Event('scooch:stills'))}},8000);var last=performance.now(),el,bar=-1,seen=0,M=matchMedia('(prefers-reduced-motion: reduce)').matches;function tick(now){if(L.dead)return;el=el||document.getElementById('glass');var b=0,p;for(p in F)b+=F[p][0]*(D[p]?1:Math.min(1,(G[p]||0)/F[p][1]));L.target=Math.max(L.target,L.ready?1:Math.min(.97,.9*b/T+.1*L.stage));var dt=Math.min(.05,(now-last)/1e3),g=L.target-L.shown;last=now;if(g>0){var s=M?g:L.ready?Math.min(g,dt*1.65):Math.min(g*(1-Math.exp(-dt*4)),dt*1.65);L.shown+=s}if(!seen&&L.shown>.01){seen=1;performance.mark('scooch:juice')}if(el){el.style.setProperty('--fill',L.shown.toFixed(4));var v=Math.round(L.shown*100);if(v!==bar){bar=v;el.setAttribute('aria-valuenow',v)}}if(L.ready&&L.shown>=.999){L.shown=1;if(el)el.style.setProperty('--fill','1');performance.mark('scooch:full');R();return}requestAnimationFrame(tick)}requestAnimationFrame(tick)})()</script>`;
  console.log('first frame set', Object.keys(sizes).length, 'files', (Object.values(sizes).reduce((a, [b]) => a + b, 0) / 1024).toFixed(0), 'KB over the wire');
  for (const { page, file } of pages) {
    const path = join(dist, file);
    const html = await readFile(path, 'utf8');
    if (!html.includes('<div id="root"></div>')) throw new Error(`${file} has no empty root`);
    const body = render(page);
    const hints = page === 'home' ? `${preload}\n    ${scene}` : preload;
    let next = html.replace('<div id="root"></div>', `<div id="root">${body}</div>`).replace('</title>', `</title>\n    ${hints}`);
    if (page === 'home') next = next.replace('<body>', `<body>\n    ${loader}`);
    await writeFile(path, next);
    console.log('prerendered', file, `${Math.round(body.length / 1024)} kB`);
  }
} finally {
  await server.close();
}
