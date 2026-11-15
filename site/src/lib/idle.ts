type IdleWindow = Window & { requestIdleCallback?: (callback: (deadline: { timeRemaining: () => number }) => void, options?: { timeout: number }) => number };

const FRAME = 1000 / 60;
const HIDDEN = 200;

function afterFrame() {
  return new Promise<number>((resolve) => {
    let done = false;
    const finish = (left: number) => {
      if (done) return;
      done = true;
      window.clearTimeout(timer);
      resolve(left);
    };
    const timer = window.setTimeout(() => finish(8), HIDDEN);
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

export function idle(timeout = 400) {
  const host = window as IdleWindow;
  if (!host.requestIdleCallback) return afterFrame();
  return new Promise<number>((resolve) => {
    host.requestIdleCallback!((deadline) => resolve(deadline.timeRemaining()), { timeout });
  });
}

export async function quiet(budget = 6, timeout = 400) {
  for (let attempt = 0; attempt < 8; attempt++) {
    const left = await idle(timeout);
    if (left >= budget) return;
  }
}
