const KEY = 'scooch:sound';
const listeners = new Set<(on: boolean) => void>();

function stored() {
  try {
    return window.localStorage.getItem(KEY) !== 'off';
  } catch {
    return true;
  }
}

export const sound = { on: typeof window === 'undefined' ? true : stored() };

export function onSound(listener: (on: boolean) => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function save(on: boolean) {
  try {
    window.localStorage.setItem(KEY, on ? 'on' : 'off');
    return true;
  } catch {
    return false;
  }
}

export function setSound(on: boolean) {
  sound.on = on;
  save(on);
  listeners.forEach((listener) => listener(on));
}
