type IdleWindow = Window & { requestIdleCallback?: (callback: (deadline: { timeRemaining: () => number }) => void, options?: { timeout: number }) => number };

export function idle(timeout = 400) {
  return new Promise<number>((resolve) => {
    const host = window as IdleWindow;
    if (host.requestIdleCallback) host.requestIdleCallback((deadline) => resolve(deadline.timeRemaining()), { timeout });
    else window.setTimeout(() => resolve(8), 32);
  });
}

const fast = { on: () => false };

export function quietFast(test: () => boolean) {
  fast.on = test;
}

function nextTask() {
  return new Promise<void>((resolve) => {
    const channel = new MessageChannel();
    channel.port1.onmessage = () => resolve();
    channel.port2.postMessage(0);
  });
}

const holds = new Set<() => boolean>();
const HOLD_LIMIT = 1000;

export function holdQuiet(test: () => boolean) {
  holds.add(test);
  return () => {
    holds.delete(test);
  };
}

function held() {
  for (const test of holds) if (test()) return true;
  return false;
}

export async function quiet(budget = 6, timeout = 400) {
  if (fast.on()) {
    await nextTask();
    return;
  }
  for (let attempt = 0; attempt < 8; attempt++) {
    const left = await idle(timeout);
    if (left >= budget) break;
  }
  const until = performance.now() + HOLD_LIMIT;
  while (held() && performance.now() < until) await new Promise((resolve) => window.setTimeout(resolve, 16));
}
