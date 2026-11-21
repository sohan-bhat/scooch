import assert from 'node:assert/strict';
import { test } from 'node:test';
import { choose } from '../src/parts/choice.ts';

const startAgain = 'Start again';
const toggleOff = 'Turn off';
const again = (command: 'turn-on' | 'turn-off') => (command === 'turn-off' ? 'Turn off again' : 'Turn on again');

test('a failed Turn off with the Stopped problem offers Turn off alone', () => {
  assert.deepEqual(choose('turn-off', startAgain, toggleOff, again), { pill: 'Turn off again', quiet: null });
});

test('a failed Turn on offers Turn on alone', () => {
  assert.deepEqual(choose('turn-on', startAgain, 'Turn on', again), { pill: 'Turn on again', quiet: null });
});

test('a failed fix offers that same fix alone', () => {
  assert.deepEqual(choose('fix', 'Move to Applications', null, again), { pill: 'Move to Applications', quiet: null });
});

test('a failed switch or picture keeps the problem fix off the pill', () => {
  assert.deepEqual(choose('other', startAgain, toggleOff, again), { pill: toggleOff, quiet: null });
});

test('with no failure the problem fix takes the pill and the toggle sits beside it', () => {
  assert.deepEqual(choose(null, startAgain, toggleOff, again), { pill: startAgain, quiet: toggleOff });
  assert.deepEqual(choose(null, null, toggleOff, again), { pill: toggleOff, quiet: null });
});
