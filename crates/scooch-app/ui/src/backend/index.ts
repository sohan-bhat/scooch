import type { Backend } from './types';

declare const __MOCK__: boolean;

export async function connect(): Promise<Backend> {
  if (__MOCK__) return (await import('./mock')).mock;
  return (await import('./tauri')).tauri;
}

export type { Backend } from './types';
