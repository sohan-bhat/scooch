import heroset from './heroset.json';

const query = new URLSearchParams(window.location.search);
const only = query.get('sharp');
const chosen = query.get('heroset');
const hero = chosen === null ? heroset.hero : chosen.split(',');
const NAMES = ['television', 'desk', 'laptop', 'lamp', 'clock', 'cassette', 'plant', 'stapler', 'glasses', 'gamepad', 'main', 'gallery', 'labels'];
const group = (name: string) => (NAMES.includes(name) ? name : 'gallery');

export function sharpAllowed(name: string) {
  return only === null || only.split(',').includes(name);
}

export function inHeroSet(name: string) {
  return hero.includes(name);
}

export function restOrder() {
  const order = (heroset as { order?: string[] }).order ?? [];
  const known = [...order, ...NAMES.filter((name) => !order.includes(name))];
  return known.filter((name) => !inHeroSet(group(name)) && sharpAllowed(group(name)));
}
