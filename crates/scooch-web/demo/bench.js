import { Remote } from '../pkg/engine.js';

/**
 * @param {number} width
 * @param {number} height
 * @param {number} count
 * @returns {Float32Array}
 */
function icons(width, height, count) {
  const per = Math.max(1, Math.floor((height - 45) / 100));
  const out = new Float32Array(count * 4);
  for (let i = 0; i < count; i++) {
    const col = Math.floor(i / per);
    const row = i % per;
    out.set([width - 10 - (col + 1) * 90 + 7, 35 + row * 100 + 3, 76, 94], i * 4);
  }
  return out;
}

/** @param {number[]} values */
function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

async function main() {
  const meta = await (await fetch('assets/picture.json')).json();
  const picture = new Uint8Array(await (await fetch('assets/picture.rgb')).arrayBuffer());
  const analysis = new Uint8Array(await (await fetch('assets/analysis.bin')).arrayBuffer());
  /** @type {Record<string, any>} */
  const results = {};
  for (const [width, height] of [[1280, 960], [1920, 1080]]) {
    let t = performance.now();
    const engine = await Remote.create(picture, meta.width, meta.height, analysis);
    const create = performance.now() - t;
    await engine.screen(width, height);
    const set = icons(width, height, 12);
    t = performance.now();
    await engine.drop(set);
    const first = performance.now() - t;
    const drops = [];
    for (let k = 0; k < 6; k++) {
      const i = (k * 5) % 12;
      set[i * 4] -= k % 2 === 0 ? 300 : -300;
      t = performance.now();
      await engine.drop(set);
      drops.push(performance.now() - t);
    }
    const drags = [];
    const held = 4;
    for (let k = 0; k < 16; k++) {
      set[held * 4] -= 12;
      set[held * 4 + 1] += 6;
      t = performance.now();
      await engine.drag(set, held, 15);
      drags.push(performance.now() - t);
    }
    await engine.drop(set);
    await engine.free();
    results[`${width}x${height}`] = { create, first, drop: median(drops), dropMax: Math.max(...drops), drag: median(drags), dragMax: Math.max(...drags) };
  }
  /** @type {any} */ (window).result = results;
}

main().catch((error) => {
  /** @type {any} */ (window).result = { error: String(error) };
});
