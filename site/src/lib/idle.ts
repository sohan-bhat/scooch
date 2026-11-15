type Deadline = { timeRemaining: () => number; didTimeout?: boolean };
type IdleWindow = Window & { requestIdleCallback?: (callback: (deadline: Deadline) => void, options?: { timeout: number }) => number };

export const WAIT = 250;
const FRAME = 1000 / 60;
const HIDDEN = 200;

export function afterFrame() {
  return new Promise<number>((resolve) => {
    let done = false;
    const finish = (left: number) => {
      if (done) return;
      done = true;
      window.clearTimeout(timer);
      resolve(left);
    };
    const timer = window.setTimeout(() => finish(FRAME / 2), HIDDEN);
    requestAnimationFrame((start) => {
      const channel = new MessageChannel();
      channel.port1.onmessage = () => {
        channel.port1.close();
        finish(Math.max(0, FRAME - (performance.now() - start)));
      };
      channel.port2.postMessage(0);
    });
  });
}

export function idle(timeout = WAIT) {
  const host = window as IdleWindow;
  if (!host.requestIdleCallback) return afterFrame();
  return new Promise<number>((resolve) => {
    host.requestIdleCallback!((deadline) => resolve(deadline.didTimeout ? 0 : deadline.timeRemaining()), { timeout });
  });
}

const holds = new Set<() => boolean>();

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

export async function quiet(budget = 6) {
  const start = performance.now();
  for (;;) {
    const left = WAIT - (performance.now() - start);
    if (left <= 0) break;
    const free = await idle(Math.max(1, left));
    if (free >= budget && !held()) return;
  }
  await afterFrame();
}
