import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { readFile, writeFile, mkdtemp, rm } from 'node:fs/promises';
import { extname, join, normalize, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const crates = normalize(join(here, '..', '..'));
const [page = 'bench', out = join(here, 'out')] = process.argv.slice(2);
const port = 8765;
const debug = 9338;
const chrome = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json', '.glsl': 'text/plain' };

const server = createServer(async (req, res) => {
  const path = normalize(join(crates, decodeURIComponent(new URL(req.url ?? '/', 'http://x').pathname)));
  if (!path.startsWith(crates)) {
    res.writeHead(403).end();
    return;
  }
  try {
    const body = await readFile(path);
    res.writeHead(200, { 'content-type': types[extname(path)] ?? 'application/octet-stream', 'cache-control': 'no-store' }).end(body);
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((resolve) => server.listen(port, '127.0.0.1', resolve));
const profile = await mkdtemp(join(tmpdir(), 'scooch-web-chrome-'));
const browser = spawn(chrome, ['--headless=new', `--remote-debugging-port=${debug}`, `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', '--disable-background-timer-throttling', 'about:blank'], { stdio: 'ignore' });

/** @param {number} ms */
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function finish(code) {
  browser.kill('SIGTERM');
  await new Promise((resolve) => (browser.exitCode !== null ? resolve(null) : browser.once('exit', resolve)));
  server.close();
  await rm(profile, { recursive: true, force: true });
  process.exit(code);
}

try {
  let targets = null;
  for (let i = 0; i < 100 && !targets; i++) {
    try {
      targets = await (await fetch(`http://127.0.0.1:${debug}/json/list`)).json();
    } catch {
      await sleep(100);
    }
  }
  const tab = targets.find((t) => t.type === 'page');
  const socket = new WebSocket(tab.webSocketDebuggerUrl);
  await new Promise((resolve) => socket.addEventListener('open', resolve, { once: true }));
  let count = 0;
  const pending = new Map();
  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    if (message.id && pending.has(message.id)) {
      pending.get(message.id)(message.result);
      pending.delete(message.id);
    }
  });
  const send = (method, params = {}) => new Promise((resolve) => {
    const id = ++count;
    pending.set(id, resolve);
    socket.send(JSON.stringify({ id, method, params }));
  });
  const value = async (expression) => (await send('Runtime.evaluate', { expression, returnByValue: true })).result.value;
  await send('Page.enable');
  await send('Page.navigate', { url: `http://127.0.0.1:${port}/scooch-web/demo/${page}.html` });
  const key = page === 'bench' ? 'result' : 'strip';
  let done = null;
  for (let i = 0; i < 1200 && done === null; i++) {
    await sleep(250);
    done = await value(`window.${key} === undefined ? null : window.${key}`);
  }
  if (page === 'bench') {
    console.log(JSON.stringify(done, null, 2));
  } else if (typeof done === 'string' && done.startsWith('data:image/png;base64,')) {
    await writeFile(`${out}.png`, Buffer.from(done.slice(22), 'base64'));
    console.log(`wrote ${out}.png`);
  } else {
    console.log(done);
  }
  socket.close();
  await finish(0);
} catch (error) {
  console.error(error);
  await finish(1);
}
