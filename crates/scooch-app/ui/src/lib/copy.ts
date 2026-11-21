import en from '~/copy/en.json';

export const copy = en;

export function fill(text: string, values: Record<string, string | number>) {
  return text.replace(/\{(\w+)\}/g, (_, key: string) => String(values[key] ?? ''));
}
