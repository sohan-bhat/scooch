type Deadline = { timeRemaining: () => number; didTimeout?: boolean };
type IdleWindow = Window & { requestIdleCallback?: (callback: (deadline: Deadline) => void, options?: { timeout: number }) => number };

export const WAIT = 250;
const FRAME = 1000 / 60;
const HIDDEN = 200;

const starve = { calmAt: performance.now() };

export function calmNow() {
  starve.calmAt = performance.now();
}

export function starved() {
  return performance.now() - starve.calmAt > WAIT;
}

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

const slots: ((left: number) => void)[] = [];
let pumping = false;

async function pump() {
  if (pumping) return;
  pumping = true;
  while (slots.length > 0) {
    const left = await afterFrame();
    slots.shift()?.(left);
  }
  pumping = false;
}

export function frameSlot() {
  return new Promise<number>((resolve) => {
    slots.push(resolve);
    void pump();
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
  for (;;) {
    if (starved()) {
      const left = await frameSlot();
      if (left >= budget && !held()) calmNow();
      return;
    }
    const left = WAIT - (performance.now() - starve.calmAt);
    const free = await idle(Math.max(1, left));
    if (free >= budget && !held()) {
      calmNow();
      return;
    }
  }
}
