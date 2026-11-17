type Early = Map<string, Promise<Response | null>>;

export function takeEarly(url: string): Promise<Response | null> | null {
  const map = (window as Window & { __early?: Early }).__early;
  if (!map) return null;
  const key = new URL(url, location.href).href;
  const found = map.get(key);
  if (!found) return null;
  map.delete(key);
  return found.then((response) => (response && response.ok ? response : null));
}

const AGAIN_AFTER = 500;

const pause = (ms: number) => new Promise<void>((resolve) => window.setTimeout(resolve, ms));

export async function fetchEarly(url: string) {
  const early = takeEarly(url);
  if (early) {
    const response = await early;
    if (response) return response;
  } else {
    const response = await fetch(url).catch(() => null);
    if (response && response.status < 500) return response;
  }
  await pause(AGAIN_AFTER);
  return fetch(url);
}
