import { asset } from '@/lib/base';

export type Rect = [number, number, number, number];

export type DemoFrame = {
  frame: string;
  saliency: string;
  average: [number, number, number];
  flat: boolean;
  solved: boolean;
  subject: Rect;
  icons: Record<string, Rect>;
};

export type DemoManifest = {
  screen: { width: number; height: number };
  icons: { id: string; label: string; glyph: string }[];
  photo: { file: string; width: number; height: number };
  order: string[];
  tables: Record<string, { fx: number[]; fy: number[] }>;
  frames: Record<string, DemoFrame> & { before: DemoFrame; fixed: DemoFrame; moved: DemoFrame };
  drag: { icon: string; from: [number, number]; to: [number, number] };
};

let pending: Promise<DemoManifest> | null = null;

export function loadManifest() {
  pending ??= fetch(asset('demo/manifest.json')).then((response) => {
    if (!response.ok) throw new Error(`manifest ${response.status}`);
    return response.json() as Promise<DemoManifest>;
  });
  return pending;
}
