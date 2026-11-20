export type System = 'macos' | 'windows' | 'linux';

export type Rect = { x: number; y: number; w: number; h: number };

export type Monitor = {
  id: string;
  frame: Rect;
  picture: string | null;
  icons: Rect[] | null;
};

export type WallpaperKind = 'slideshow' | 'video' | 'color' | 'moving' | 'unreadable' | 'made';

export type Problem =
  | { kind: 'automation' }
  | { kind: 'asking' }
  | { kind: 'finder' }
  | { kind: 'move' }
  | { kind: 'wallpaper'; what: WallpaperKind }
  | { kind: 'gnome' }
  | { kind: 'unknown'; desktop: string }
  | { kind: 'admin' }
  | { kind: 'startup' }
  | { kind: 'stopped' }
  | { kind: 'screen' }
  | { kind: 'other' };

export type SwitchName = 'glide' | 'scenery' | 'flip' | 'predict';

export type Switches = Partial<Record<SwitchName, boolean>>;

export type Phase = 'on' | 'off' | 'starting' | 'stopping';

export type View = {
  system: System;
  phase: Phase;
  pending: number;
  monitors: Monitor[];
  switches: Switches;
  problem: Problem | null;
  flipHelps: boolean;
  current: string | null;
  textScale: number;
};

export type Picture = { path: string; thumb: string };

export interface Backend {
  view(): Promise<View>;
  pictures(): Promise<Picture[]>;
  turnOn(): Promise<void>;
  turnOff(): Promise<void>;
  set(name: SwitchName, on: boolean): Promise<void>;
  use(path: string): Promise<void>;
  pick(): Promise<string | null>;
  fix(problem: Problem): Promise<void>;
  subscribe(listener: () => void): () => void;
  onDrop(listener: (hovering: boolean, path: string | null) => void): () => void;
}
