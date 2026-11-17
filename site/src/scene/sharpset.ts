import heroset from './heroset.json';

const query = new URLSearchParams(window.location.search);
const only = query.get('sharp');
const chosen = query.get('heroset');
const hero = chosen === null ? heroset.hero : chosen.split(',');

export function sharpAllowed(name: string) {
  return only === null || only.split(',').includes(name);
}

export function inHeroSet(name: string) {
  return hero.includes(name);
}
