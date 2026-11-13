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

export async function fetchEarly(url: string) {
  return (await takeEarly(url)) ?? fetch(url);
}
