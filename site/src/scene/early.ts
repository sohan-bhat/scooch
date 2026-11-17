import { FileLoader, ImageBitmapLoader, ImageLoader } from 'three';
import { takeEarly } from '@/story/early';

const AGAIN_AFTER = 500;
const retried = new Set<string>();

type Load = (this: Loader, url: string, onLoad?: (data: never) => void, onProgress?: (event: ProgressEvent) => void, onError?: (error: unknown) => void) => unknown;
type Loader = { manager: { resolveURL: (url: string) => string }; path?: string };

function worthAgain(url: string, error: unknown) {
  if (retried.has(url)) return false;
  const status = (error as { response?: { status?: number } } | null)?.response?.status;
  if (status !== undefined && status < 500) return false;
  retried.add(url);
  return true;
}

function once(original: Load): Load {
  return function load(this: Loader, url, onLoad, onProgress, onError) {
    const full = this.manager.resolveURL((this.path ?? '') + (url ?? ''));
    return original.call(this, url, onLoad, onProgress, (error) => {
      if (!worthAgain(full, error)) {
        onError?.(error);
        return;
      }
      window.setTimeout(() => original.call(this, url, onLoad, onProgress, onError), AGAIN_AFTER);
    });
  };
}

ImageLoader.prototype.load = once(ImageLoader.prototype.load as unknown as Load) as never;
ImageBitmapLoader.prototype.load = once(ImageBitmapLoader.prototype.load as unknown as Load) as never;

const read: Partial<Record<string, (response: Response) => Promise<unknown>>> = {
  arraybuffer: (response) => response.arrayBuffer(),
  json: (response) => response.json(),
  text: (response) => response.text(),
  '': (response) => response.text(),
};

const load = once(FileLoader.prototype.load as unknown as Load);

FileLoader.prototype.load = function (this: FileLoader, url, onLoad, onProgress, onError) {
  const reader = read[this.responseType];
  const full = this.manager.resolveURL((this.path ?? '') + (url ?? ''));
  const pending = reader ? takeEarly(full) : null;
  if (!reader || !pending) return load.call(this, url, onLoad as never, onProgress, onError) as never;
  this.manager.itemStart(full);
  pending
    .then((response) => (response ? reader(response) : Promise.reject(new Error('early miss'))))
    .then(
      (data) => {
        onLoad?.(data as never);
        this.manager.itemEnd(full);
      },
      () => {
        this.manager.itemEnd(full);
        retried.add(full);
        window.setTimeout(() => load.call(this, url, onLoad as never, onProgress, onError), AGAIN_AFTER);
      },
    );
};
