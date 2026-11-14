/* tslint:disable */
/* eslint-disable */

export class Engine {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    static create(picture: Uint8Array | number[], width: number, height: number, analysis: Uint8Array | number[]): Engine;
    drag(icons: Float32Array | number[], held: number, budget_ms: number): Tables;
    drop(icons: Float32Array | number[]): Tables;
    screen(width: number, height: number): void;
}

export class Tables {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    canvas(): Uint8Array;
    constants(): Float32Array;
    field(): Uint16Array;
    look(): Uint8Array;
    map(): Float32Array;
    pad(): Uint8Array;
    paints(): Uint8Array;
    ramps(): Uint8Array;
    readonly canvasFits: boolean;
    readonly canvasHeight: number;
    readonly canvasWidth: number;
    readonly gridHeight: number;
    readonly gridWidth: number;
    readonly height: number;
    readonly mapWidth: number;
    readonly mirrored: boolean;
    readonly paintHeight: number;
    readonly paintWidth: number;
    readonly width: number;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_engine_free: (a: number, b: number) => void;
    readonly __wbg_tables_free: (a: number, b: number) => void;
    readonly engine_create: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number];
    readonly engine_drag: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly engine_drop: (a: number, b: number, c: number) => number;
    readonly engine_screen: (a: number, b: number, c: number) => void;
    readonly tables_canvas: (a: number) => [number, number];
    readonly tables_canvasFits: (a: number) => number;
    readonly tables_canvasHeight: (a: number) => number;
    readonly tables_canvasWidth: (a: number) => number;
    readonly tables_constants: (a: number) => [number, number];
    readonly tables_field: (a: number) => [number, number];
    readonly tables_gridHeight: (a: number) => number;
    readonly tables_gridWidth: (a: number) => number;
    readonly tables_height: (a: number) => number;
    readonly tables_look: (a: number) => [number, number];
    readonly tables_map: (a: number) => [number, number];
    readonly tables_mapWidth: (a: number) => number;
    readonly tables_mirrored: (a: number) => number;
    readonly tables_pad: (a: number) => [number, number];
    readonly tables_paintHeight: (a: number) => number;
    readonly tables_paintWidth: (a: number) => number;
    readonly tables_paints: (a: number) => [number, number];
    readonly tables_ramps: (a: number) => [number, number];
    readonly tables_width: (a: number) => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
