import { Engine, transferables } from './engine.js';

/** @type {Engine | null} */
let engine = null;
/** @type {{id: number, op: string, args: any[]}[]} */
const queue = [];
let busy = false;

/**
 * @param {number} id
 * @param {any} value
 * @param {ArrayBuffer[]} [transfer]
 */
function reply(id, value, transfer = []) {
  self.postMessage({ id, ok: true, value }, transfer);
}

/**
 * @param {{id: number, op: string, args: any[]}} job
 */
async function run(job) {
  const { id, op, args } = job;
  if (op === 'create') {
    const [picture, width, height, analysis] = args;
    engine = await Engine.create(picture, width, height, analysis);
    reply(id, null);
  } else if (!engine) {
    throw new Error('create the engine first');
  } else if (op === 'screen') {
    await engine.screen(args[0], args[1]);
    reply(id, null);
  } else if (op === 'drop' || op === 'drag') {
    const tables = op === 'drop' ? await engine.drop(args[0]) : await engine.drag(args[0], args[1], args[2]);
    reply(id, tables, transferables(tables));
  } else if (op === 'memory') {
    reply(id, await engine.memory());
  } else if (op === 'free') {
    await engine.free();
    engine = null;
    reply(id, null);
  } else {
    throw new Error(`unknown call ${op}`);
  }
}

function next() {
  if (busy || queue.length === 0) return;
  const job = /** @type {{id: number, op: string, args: any[]}} */ (queue.shift());
  if (job.op === 'drag' && queue.some((later) => later.op === 'drag')) {
    reply(job.id, null);
    setTimeout(next, 0);
    return;
  }
  busy = true;
  run(job)
    .catch((error) => self.postMessage({ id: job.id, ok: false, error: String(error && error.message ? error.message : error) }))
    .finally(() => {
      busy = false;
      setTimeout(next, 0);
    });
}

self.onmessage = (event) => {
  queue.push(event.data);
  setTimeout(next, 0);
};
