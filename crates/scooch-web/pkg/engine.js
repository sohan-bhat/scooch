import init, { Engine as Raw } from './scooch_web.js';

/**
 * One answer of the engine, ready to upload as textures. Lengths and formats follow the
 * texture table in the scooch-web crate docs.
 * @typedef {Object} Tables
 * @property {number} width output width in pixels
 * @property {number} height output height in pixels
 * @property {number} canvasWidth
 * @property {number} canvasHeight
 * @property {number} gridWidth quarter grid width
 * @property {number} gridHeight quarter grid height
 * @property {number} paintWidth sixteenth grid width
 * @property {number} paintHeight sixteenth grid height
 * @property {number} mapWidth max(width, height), the width of the map texture
 * @property {boolean} canvasFits true when the page's canvas texture still matches
 * @property {boolean} mirrored
 * @property {Float32Array} map mapWidth x 2, row 0 xmap, row 1 ymap
 * @property {Uint16Array} field gridWidth x gridHeight half floats
 * @property {Uint8Array} look gridWidth x gridHeight RGBA
 * @property {Uint8Array} pad gridWidth x gridHeight RGBA
 * @property {Uint8Array} paints paintWidth x paintHeight x 8 layers RGBA
 * @property {Uint8Array} ramps 512 x 2 RGBA
 * @property {Float32Array} constants the 28 floats of the Frame uniform block
 * @property {Uint8Array} [canvas] RGB canvas, only present when canvasFits is false
 */

let ready = null;
/** @type {any} */
let instance = null;

/**
 * Loads the WebAssembly once. Later calls return the same promise.
 * @param {string | URL} [wasm] where scooch_web_bg.wasm lives, next to this file by default
 * @returns {Promise<void>}
 */
export function load(wasm = new URL('./scooch_web_bg.wasm', import.meta.url)) {
  ready ??= init({ module_or_path: wasm }).then((exports) => {
    instance = exports;
  });
  return ready;
}

/**
 * @param {Raw | any} raw
 * @returns {Tables}
 */
function unpack(raw) {
  /** @type {Tables} */
  const out = {
    width: raw.width,
    height: raw.height,
    canvasWidth: raw.canvasWidth,
    canvasHeight: raw.canvasHeight,
    gridWidth: raw.gridWidth,
    gridHeight: raw.gridHeight,
    paintWidth: raw.paintWidth,
    paintHeight: raw.paintHeight,
    mapWidth: raw.mapWidth,
    canvasFits: raw.canvasFits,
    mirrored: raw.mirrored,
    map: raw.map(),
    field: raw.field(),
    look: raw.look(),
    pad: raw.pad(),
    paints: raw.paints(),
    ramps: raw.ramps(),
    constants: raw.constants(),
  };
  if (!out.canvasFits) out.canvas = raw.canvas();
  raw.free();
  return out;
}

/**
 * The typed arrays of a Tables, for postMessage transfer lists.
 * @param {Tables} tables
 * @returns {ArrayBuffer[]}
 */
export function transferables(tables) {
  const arrays = [tables.map, tables.field, tables.look, tables.pad, tables.paints, tables.ramps, tables.constants];
  if (tables.canvas) arrays.push(tables.canvas);
  return arrays.map((a) => /** @type {ArrayBuffer} */ (a.buffer));
}

/** The engine in the current thread. Each call blocks this thread while it runs. */
export class Engine {
  /** @param {Raw} raw */
  constructor(raw) {
    /** @private */
    this.raw = raw;
  }

  /**
   * @param {Uint8Array} picture the export's canvas decoded to RGB, width * height * 3 bytes
   * @param {number} width
   * @param {number} height
   * @param {Uint8Array} analysis the export's analysis.bin
   * @returns {Promise<Engine>}
   */
  static async create(picture, width, height, analysis) {
    await load();
    return new Engine(Raw.create(picture, width, height, analysis));
  }

  /**
   * @param {number} width output width in pixels
   * @param {number} height output height in pixels
   * @returns {Promise<void>}
   */
  async screen(width, height) {
    this.raw.screen(width, height);
  }

  /**
   * The full answer for icons at rest.
   * @param {Float32Array} icons x, y, w, h for each icon, in output pixels
   * @returns {Promise<Tables>}
   */
  async drop(icons) {
    return unpack(this.raw.drop(icons));
  }

  /**
   * The quick answer while an icon is held. A drop follows when it is let go.
   * @param {Float32Array} icons x, y, w, h for each icon, in output pixels
   * @param {number} held index of the held icon
   * @param {number} budgetMs time budget for the placement search
   * @returns {Promise<Tables>}
   */
  async drag(icons, held, budgetMs) {
    return unpack(this.raw.drag(icons, held, budgetMs));
  }

  /**
   * Bytes of WebAssembly memory the engine holds in this thread.
   * @returns {Promise<number>}
   */
  async memory() {
    return instance ? instance.memory.buffer.byteLength : 0;
  }

  /** @returns {Promise<void>} */
  async free() {
    this.raw.free();
  }
}

/** The engine in a Web Worker running worker.js. The page's thread never waits on it. */
export class Remote {
  /**
   * @param {Worker} worker
   * @private
   */
  constructor(worker) {
    this.worker = worker;
    this.count = 0;
    /** @type {Map<number, {resolve: (value: any) => void, reject: (reason: any) => void}>} */
    this.waiting = new Map();
    worker.onmessage = (event) => {
      const { id, ok, value, error } = event.data;
      const entry = this.waiting.get(id);
      if (!entry) return;
      this.waiting.delete(id);
      if (ok) entry.resolve(value);
      else entry.reject(new Error(error));
    };
  }

  /**
   * @param {Uint8Array} picture the export's canvas decoded to RGB
   * @param {number} width
   * @param {number} height
   * @param {Uint8Array} analysis the export's analysis.bin
   * @param {string | URL} [script] where worker.js lives, next to this file by default
   * @returns {Promise<Remote>}
   */
  static async create(picture, width, height, analysis, script = new URL('./worker.js', import.meta.url)) {
    const remote = new Remote(new Worker(script, { type: 'module' }));
    await remote.call('create', [picture, width, height, analysis]);
    return remote;
  }

  /**
   * @param {string} op
   * @param {any[]} args
   * @returns {Promise<any>}
   * @private
   */
  call(op, args) {
    const id = ++this.count;
    return new Promise((resolve, reject) => {
      this.waiting.set(id, { resolve, reject });
      this.worker.postMessage({ id, op, args });
    });
  }

  /**
   * @param {number} width
   * @param {number} height
   * @returns {Promise<void>}
   */
  screen(width, height) {
    return this.call('screen', [width, height]);
  }

  /**
   * @param {Float32Array} icons
   * @returns {Promise<Tables>}
   */
  drop(icons) {
    return this.call('drop', [icons]);
  }

  /**
   * Resolves to null when a newer drag replaced this one before it ran.
   * @param {Float32Array} icons
   * @param {number} held
   * @param {number} budgetMs
   * @returns {Promise<Tables | null>}
   */
  drag(icons, held, budgetMs) {
    return this.call('drag', [icons, held, budgetMs]);
  }

  /**
   * Bytes of WebAssembly memory the engine holds in its worker.
   * @returns {Promise<number>}
   */
  memory() {
    return this.call('memory', []);
  }

  /** @returns {Promise<void>} */
  async free() {
    await this.call('free', []);
    this.worker.terminate();
  }
}
