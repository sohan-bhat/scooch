import assert from 'node:assert/strict';
import { test } from 'node:test';

const answers = {};
globalThis.window = {
  __TAURI_INTERNALS__: {
    convertFileSrc: (path, protocol) => `${protocol}://localhost/${encodeURIComponent(path)}`,
    invoke: async (command) => answers[command],
  },
};
const { tauri } = await import('./tauri.ts');

const path = '/home/ada/.local/share/scooch/out/main-shown.jpg';
const bare = `scooch://localhost/${encodeURIComponent(path)}`;
const frame = { x: 0, y: 0, w: 1920, h: 1080 };

async function seen(picture, stamp) {
  answers.view = { system: 'linux', on: true, running: true, restoring: false, pending: 0, monitors: [{ id: 'main', frame, picture, places: null, stamp }], switches: { scenery: true, flip: false }, problem: null, flipHelps: false, current: null, counted: null, reduceMotion: false, live: false, textScale: 1 };
  return (await tauri.view()).monitors[0];
}

test('a monitor picture with a stamp ends in v and that stamp', async () => {
  assert.deepEqual(await seen(path, 1700000000123), { id: 'main', frame, picture: `${bare}?v=1700000000123`, places: null });
});

test('the same path with a new stamp gets a new address, so the desk shows the new picture', async () => {
  const before = await seen(path, 1700000000123);
  const after = await seen(path, 1700000000456);
  assert.notEqual(before.picture, after.picture);
  assert.equal(after.picture, `${bare}?v=1700000000456`);
});

test('a monitor with no stamp keeps the bare address and one with no picture keeps none', async () => {
  assert.equal((await seen(path, null)).picture, bare);
  assert.equal((await seen(null, null)).picture, null);
});

test('a thumbnail keeps the thumb address and gets no stamp', async () => {
  answers.pictures = [path];
  assert.deepEqual(await tauri.pictures(), [{ path, thumb: `${bare}?thumb` }]);
});
