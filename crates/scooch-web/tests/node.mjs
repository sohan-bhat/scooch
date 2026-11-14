import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createRequire } from 'node:module';

const [pkg, dir] = process.argv.slice(2);
const require = createRequire(import.meta.url);
const { Engine } = require(join(pkg, 'scooch_web.js'));

function bytes(t) {
  const parts = [t.map(), t.field(), t.look(), t.pad(), t.paints(), t.ramps(), t.constants(), new Uint8Array([t.canvasFits ? 1 : 0])];
  const total = parts.reduce((n, p) => n + p.byteLength, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    out.set(new Uint8Array(p.buffer, p.byteOffset, p.byteLength), at);
    at += p.byteLength;
  }
  return out;
}

function run(caseDir, meta) {
  const picture = readFileSync(join(caseDir, 'picture.rgb'));
  const analysis = readFileSync(join(caseDir, 'analysis.bin'));
  const engine = Engine.create(picture, meta.picture[0], meta.picture[1], analysis);
  engine.screen(meta.screen[0], meta.screen[1]);
  const outs = [];
  for (const step of meta.steps) {
    const icons = new Float32Array(step.icons);
    const t = step.op === 'drop' ? engine.drop(icons) : engine.drag(icons, step.held, 1e12);
    outs.push(bytes(t));
    t.free();
  }
  engine.free();
  return outs;
}

const cases = JSON.parse(readFileSync(join(dir, 'cases.json'), 'utf8'));
let failed = 0;
for (const name of cases) {
  const caseDir = join(dir, name);
  const meta = JSON.parse(readFileSync(join(caseDir, 'walk.json'), 'utf8'));
  const expected = readFileSync(join(caseDir, 'expected.bin'));
  const first = run(caseDir, meta);
  const second = run(caseDir, meta);
  for (let n = 0; n < first.length; n++) {
    const want = expected.subarray(n * meta.size, (n + 1) * meta.size);
    const same = Buffer.compare(Buffer.from(first[n]), want) === 0;
    const again = Buffer.compare(Buffer.from(first[n]), Buffer.from(second[n])) === 0;
    if (!same || !again) {
      failed++;
      let at = 0;
      while (at < want.length && first[n][at] === want[at]) at++;
      console.log(`${name} step ${n} ${meta.steps[n].op}: ${same ? 'matches native' : `differs from native at byte ${at} of ${want.length}`}, ${again ? 'repeats' : 'does not repeat'}`);
    }
  }
  console.log(`${name}: ${first.length} steps checked`);
}
if (failed) {
  console.log(`${failed} steps failed`);
  process.exit(1);
}
console.log('all tables match the native session and repeat');
