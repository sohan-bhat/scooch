declare module '@engine/engine.js' {
  import type { Tables } from '@/scene/screen/scenery';
  export class Remote {
    constructor(worker: Worker);
    call(op: 'create', args: [Uint8Array, number, number, Uint8Array]): Promise<null>;
    screen(width: number, height: number): Promise<null>;
    drop(icons: Float32Array): Promise<Tables>;
    drag(icons: Float32Array, held: number, budget: number): Promise<Tables | null>;
    free(): Promise<void>;
  }
}
