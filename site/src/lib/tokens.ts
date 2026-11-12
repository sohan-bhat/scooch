import { Color } from 'three';

const cache = new Map<string, string>();

export function token(name: string) {
  const cached = cache.get(name);
  if (cached) return cached;
  const value = getComputedStyle(document.documentElement).getPropertyValue(`--${name}`).trim();
  cache.set(name, value);
  return value;
}

export function tokenColor(name: string) {
  return new Color(token(`color-${name}`));
}
