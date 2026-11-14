export type Load = {
  on: boolean;
  stage: number;
  at: number;
  ready: boolean;
  full: Promise<void>;
  cb: ((keep: boolean) => void)[];
  release(keep?: boolean): void;
  done(): void;
};

export function loader() {
  return typeof window === 'undefined' ? undefined : (window as Window & { __load?: Load }).__load;
}

export function stage(value: number) {
  const load = loader();
  if (load) load.stage = Math.max(load.stage, value);
}

export function release(keep = true) {
  loader()?.release(keep);
}
