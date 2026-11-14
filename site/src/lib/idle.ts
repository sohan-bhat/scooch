type IdleWindow = Window & { requestIdleCallback?: (callback: (deadline: { timeRemaining: () => number }) => void, options?: { timeout: number }) => number };

export function idle(timeout = 400) {
  return new Promise<number>((resolve) => {
    const host = window as IdleWindow;
    if (host.requestIdleCallback) host.requestIdleCallback((deadline) => resolve(deadline.timeRemaining()), { timeout });
    else window.setTimeout(() => resolve(8), 32);
  });
}

export async function quiet(budget = 6, timeout = 400) {
  for (let attempt = 0; attempt < 8; attempt++) {
    const left = await idle(timeout);
    if (left >= budget) return;
  }
}
