import type { Backend } from './types';

declare const __MOCK__: boolean;

export async function connect(): Promise<Backend> {
  if (__MOCK__) return (await import('./mock')).mock;
  throw new Error('The Tauri backend arrives with the shell.');
}

export type { Backend } from './types';
