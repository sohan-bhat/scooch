import { FileLoader } from 'three';
import { takeEarly } from '@/story/early';

const read: Partial<Record<string, (response: Response) => Promise<unknown>>> = {
  arraybuffer: (response) => response.arrayBuffer(),
  json: (response) => response.json(),
  text: (response) => response.text(),
  '': (response) => response.text(),
};

const load = FileLoader.prototype.load;

FileLoader.prototype.load = function (this: FileLoader, url, onLoad, onProgress, onError) {
  const reader = read[this.responseType];
  const full = this.manager.resolveURL((this.path ?? '') + (url ?? ''));
  const pending = reader ? takeEarly(full) : null;
  if (!reader || !pending) return load.call(this, url, onLoad, onProgress, onError);
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
        load.call(this, url, onLoad, onProgress, onError);
      },
    );
};
