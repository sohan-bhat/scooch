import { execFileSync } from 'node:child_process';
import { mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const repo = resolve(root, '../../..');
const photos = join(repo, 'assets/photos');
const program = process.env.SCOOCH_BIN;
if (!program) {
  console.error('Set SCOOCH_BIN to a built scooch program.');
  process.exit(1);
}

const out = join(root, 'mock');
const scratch = join(tmpdir(), `scooch-mock-${process.pid}`);
mkdirSync(scratch, { recursive: true });
for (const folder of ['thumbs', 'drawn']) {
  rmSync(join(out, folder), { recursive: true, force: true });
  mkdirSync(join(out, folder), { recursive: true });
}

function column(x, top, step, count, w, h) {
  return Array.from({ length: count }, (_, i) => [x, top + i * step, w, h]);
}

const layouts = {
  mac: { width: 1512, height: 982, icons: [...column(1404, 38, 102, 7, 92, 92), ...column(1300, 38, 102, 3, 92, 92)] },
  wide: { width: 1920, height: 1080, icons: [] },
  win: { width: 1536, height: 864, icons: [...column(6, 6, 96, 8, 78, 88), ...column(90, 6, 96, 8, 78, 88), ...column(174, 6, 96, 5, 78, 88)] },
  gnome: { width: 1920, height: 1080, icons: [...column(16, 52, 112, 5, 100, 104), ...column(124, 52, 112, 2, 100, 104)] },
  xfce: { width: 1920, height: 1080, icons: [...column(12, 12, 104, 9, 92, 96), ...column(110, 12, 104, 6, 92, 96)] },
};

const drawn = [
  ['seagull-sky', 'mac'],
  ['seagull-sky', 'wide'],
  ['golden-tree', 'win'],
  ['lone-canoe', 'gnome'],
  ['great-wave', 'xfce'],
];

function render(photo, layout, target, fill) {
  const scene = join(scratch, `${layout}-${fill ? 'fill' : 'set'}.json`);
  writeFileSync(scene, JSON.stringify(layouts[layout]));
  const args = ['render', join(photos, `${photo}.jpg`), '--icons', scene, '--out', target];
  if (fill) args.push('--fill');
  execFileSync(program, args, { stdio: 'inherit' });
  execFileSync('sips', ['-Z', '1400', '-s', 'format', 'jpeg', '-s', 'formatOptions', '72', target, '--out', target], { stdio: 'ignore' });
}

for (const [photo, layout] of drawn) {
  render(photo, layout, join(out, 'drawn', `${photo}-${layout}.jpg`), false);
  render(photo, layout, join(out, 'drawn', `${photo}-${layout}-plain.jpg`), true);
}

const thumb = join(scratch, 'thumb.json');
writeFileSync(thumb, JSON.stringify({ width: 320, height: 200, icons: [] }));
for (const name of readdirSync(photos).filter((file) => file.endsWith('.jpg'))) {
  execFileSync(program, ['render', join(photos, name), '--icons', thumb, '--out', join(out, 'thumbs', name), '--fill'], { stdio: 'inherit' });
}

writeFileSync(join(out, 'layouts.json'), `${JSON.stringify(layouts, null, 2)}\n`);
rmSync(scratch, { recursive: true, force: true });
console.log(`Wrote ${basename(out)}/drawn and ${basename(out)}/thumbs.`);
