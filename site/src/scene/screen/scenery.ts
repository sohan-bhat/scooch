import {
  BufferGeometry,
  DataArrayTexture,
  DataTexture,
  DataUtils,
  Float32BufferAttribute,
  FloatType,
  GLSL3,
  HalfFloatType,
  ImageBitmapLoader,
  LinearFilter,
  LinearMipmapLinearFilter,
  Mesh,
  NearestFilter,
  NoColorSpace,
  OrthographicCamera,
  RawShaderMaterial,
  RedFormat,
  RGBAFormat,
  Scene,
  SRGBColorSpace,
  Texture,
  UnsignedByteType,
  WebGLRenderTarget,
  type WebGLRenderer,
} from 'three';
import frame from '../../../../crates/scooch-core/shaders/frame.glsl?raw';
import { asset } from '@/lib/base';
import { quiet } from '@/lib/idle';
import { fetchEarly } from '@/story/early';

type Table = { file: string; width: number; height: number; layers?: number };

export type BundleState = {
  name: string;
  tables: {
    constants: Record<string, number | number[]>;
    map: Table;
    field: Table;
    look: Table;
    pad: Table;
    paints: Table;
    ramps: Table;
  };
};

export type SceneryInfo = {
  folder: string;
  width: number;
  height: number;
  grid_cell: number;
  paint_cell: number;
  canvas: { file: string; width: number; height: number };
  states: BundleState[];
  fill?: Table;
};

export type Look = {
  map: Texture;
  field: Texture;
  look: Texture;
  pad: Texture;
  paints: Texture;
  ramps: Texture;
};

export type Scenery = {
  info: SceneryInfo;
  canvas: Texture;
  states: Look[];
  complete: boolean;
};

const vertex = `
in vec3 position;
void main() {
  gl_Position = vec4(position.xy, 0.0, 1.0);
}
`;

async function bytes(url: string) {
  const response = await fetchEarly(url);
  if (!response.ok) throw new Error(`${response.status} ${url}`);
  const buffer = await response.arrayBuffer();
  const head = new Uint8Array(buffer, 0, Math.min(2, buffer.byteLength));
  if (head[0] !== 0x1f || head[1] !== 0x8b) return buffer;
  return new Response(new Blob([buffer]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer();
}

function data(texture: DataTexture | DataArrayTexture, filter: typeof LinearFilter | typeof NearestFilter) {
  texture.magFilter = filter;
  texture.minFilter = filter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}

async function mapTexture(url: string, table: Table) {
  return data(new DataTexture(new Float32Array(await bytes(url)), table.width, table.height, RedFormat, FloatType), NearestFilter);
}

async function fieldTexture(url: string, table: Table) {
  return data(new DataTexture(new Uint16Array(await bytes(url)), table.width, table.height, RedFormat, HalfFloatType), LinearFilter);
}

async function pixels(url: string) {
  const bitmap = await createImageBitmap(new Blob([await bytes(url)]), { premultiplyAlpha: 'none', colorSpaceConversion: 'none' });
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const context = canvas.getContext('2d', { willReadFrequently: true });
  if (!context) throw new Error('no 2d context');
  context.drawImage(bitmap, 0, 0);
  bitmap.close();
  return new Uint8Array(context.getImageData(0, 0, canvas.width, canvas.height).data.buffer);
}

async function paintsTexture(url: string, table: Table) {
  const texture = data(new DataArrayTexture(await pixels(url), table.width, table.height, table.layers ?? 8), LinearFilter);
  texture.format = RGBAFormat;
  texture.type = UnsignedByteType;
  texture.colorSpace = SRGBColorSpace;
  return texture;
}

const bitmaps = new ImageBitmapLoader().setOptions({ imageOrientation: 'none', premultiplyAlpha: 'none', colorSpaceConversion: 'none' });

async function imageTexture(url: string, srgb: boolean) {
  const bitmap = await bitmaps.loadAsync(url);
  const texture = new Texture(bitmap as ImageBitmap);
  texture.flipY = false;
  texture.premultiplyAlpha = false;
  texture.generateMipmaps = false;
  texture.magFilter = LinearFilter;
  texture.minFilter = LinearFilter;
  texture.colorSpace = srgb ? SRGBColorSpace : NoColorSpace;
  texture.needsUpdate = true;
  return texture;
}

function plainField(value: number) {
  return data(new DataTexture(new Uint16Array([DataUtils.toHalfFloat(value)]), 1, 1, RedFormat, HalfFloatType), LinearFilter);
}

function emptyPad() {
  const texture = data(new DataTexture(new Uint8Array([0, 0, 0, 0]), 1, 1, RGBAFormat, UnsignedByteType), LinearFilter);
  texture.colorSpace = SRGBColorSpace;
  return texture;
}

async function lookOf(base: string, state: BundleState): Promise<Look> {
  const t = state.tables;
  const [map, field, look, pad, paints, ramps] = await Promise.all([
    mapTexture(`${base}/${t.map.file}`, t.map),
    fieldTexture(`${base}/${t.field.file}`, t.field),
    imageTexture(`${base}/${t.look.file}`, false),
    imageTexture(`${base}/${t.pad.file}`, true),
    paintsTexture(`${base}/${t.paints.file}`, t.paints),
    imageTexture(`${base}/${t.ramps.file}`, false),
  ]);
  return { map, field, look, pad, paints, ramps };
}

function blank(srgb: boolean) {
  const texture = data(new DataTexture(new Uint8Array([0, 0, 0, 0]), 1, 1, RGBAFormat, UnsignedByteType), LinearFilter);
  if (srgb) texture.colorSpace = SRGBColorSpace;
  return texture;
}

function blankLayers() {
  const texture = data(new DataArrayTexture(new Uint8Array(4 * 8), 1, 1, 8), LinearFilter);
  texture.format = RGBAFormat;
  texture.type = UnsignedByteType;
  texture.colorSpace = SRGBColorSpace;
  return texture;
}

async function canvasTexture(base: string, info: SceneryInfo, longest = 0) {
  const url = `${base}/${info.canvas.file}`;
  const response = await fetchEarly(url);
  if (!response.ok) throw new Error(`${response.status} ${url}`);
  const blob = await response.blob();
  const { width, height } = info.canvas;
  const shrink = longest > 0 ? Math.min(1, longest / Math.max(width, height)) : 1;
  const bitmap = shrink < 1 ? await createImageBitmap(blob, { resizeWidth: Math.round(width * shrink), resizeHeight: Math.round(height * shrink), resizeQuality: 'high' }) : await createImageBitmap(blob);
  const canvas = new Texture(bitmap);
  canvas.colorSpace = SRGBColorSpace;
  canvas.flipY = false;
  canvas.generateMipmaps = true;
  canvas.minFilter = LinearMipmapLinearFilter;
  canvas.anisotropy = 4;
  canvas.needsUpdate = true;
  return canvas;
}

export async function loadScenery(info: SceneryInfo, plainStates: boolean, longest = 0): Promise<Scenery> {
  const base = asset(`demo/${info.folder}`);
  if (!plainStates || !info.fill) {
    const [canvas, ...states] = await Promise.all([canvasTexture(base, info, longest), ...info.states.map((state) => lookOf(base, state))]);
    return { info, canvas, states, complete: true };
  }
  const [canvas, fill] = await Promise.all([canvasTexture(base, info), mapTexture(`${base}/${info.fill.file}`, info.fill)]);
  const reach = Number(info.states[0].tables.constants.reach);
  const plain = plainField(reach + 0.04);
  const pad = emptyPad();
  const placeholder = { field: plain, look: blank(false), pad, paints: blankLayers(), ramps: blank(false) };
  return { info, canvas, states: [{ ...placeholder, map: fill }], complete: false };
}

export async function streamScenery(scenery: Scenery) {
  if (scenery.complete) return;
  const { info } = scenery;
  const base = asset(`demo/${info.folder}`);
  const looks = await Promise.all(info.states.map((state) => lookOf(base, state)));
  const fill = scenery.states[0];
  const first = { ...looks[0], field: fill.field, pad: fill.pad };
  scenery.states.splice(0, scenery.states.length, { ...first, map: fill.map }, first, ...looks);
  scenery.complete = true;
}

const PIXEL = 'vec2 p = vec2(gl_FragCoord.x, size.y - gl_FragCoord.y);';
if (!frame.includes(PIXEL)) throw new Error('frame.glsl no longer reads the pixel the way the site maps it');

export function createFrame(scenery: Scenery, view?: { width: number; height: number }) {
  const { info } = scenery;
  const constants = info.states[0].tables.constants;
  const number = (key: string) => Number(constants[key]);
  const pair = (key: string) => constants[key] as number[];
  const uniforms = {
    size: { value: [info.width, info.height] },
    view: { value: [view?.width ?? info.width, view?.height ?? info.height] },
    grid: { value: [info.width / info.grid_cell, info.height / info.grid_cell] },
    paint_grid: { value: [info.width / info.paint_cell, info.height / info.paint_cell] },
    s: { value: 0 },
    unit: { value: number('unit') },
    ramp_low: { value: number('ramp_low') },
    ramp_high: { value: number('ramp_high') },
    reach: { value: number('reach') },
    grain_amount: { value: number('grain') },
    grain_scale: { value: number('grain_scale') },
    grain_floor: { value: number('grain_floor') },
    grain_lod: { value: Math.log2((4 * info.canvas.width) / info.width) },
    core_lift: { value: number('core_lift') },
    core_drop: { value: number('core_drop') },
    volume_lit: { value: number('volume_lit') },
    volume_dark: { value: number('volume_dark') },
    shadow_at: { value: pair('shadow') },
    rim_at: { value: pair('rim') },
    belly_at: { value: pair('belly') },
    canvas: { value: scenery.canvas },
    map_a: { value: null as Texture | null },
    map_b: { value: null as Texture | null },
    field_a: { value: null as Texture | null },
    field_b: { value: null as Texture | null },
    look_a: { value: null as Texture | null },
    look_b: { value: null as Texture | null },
    pad_a: { value: null as Texture | null },
    pad_b: { value: null as Texture | null },
    paints_a: { value: null as Texture | null },
    paints_b: { value: null as Texture | null },
    ramps_a: { value: null as Texture | null },
    ramps_b: { value: null as Texture | null },
  };
  const material = new RawShaderMaterial({
    name: 'scooch-frame',
    glslVersion: GLSL3,
    vertexShader: vertex,
    fragmentShader: frame
      .replace(/^\s*#version 300 es\s*/, '')
      .replace('uniform vec2 size;', 'uniform vec2 size;\nuniform vec2 view;')
      .replace(PIXEL, 'vec2 p = vec2(gl_FragCoord.x, view.y - gl_FragCoord.y) * (size / view);'),
    uniforms,
    depthTest: false,
    depthWrite: false,
  });
  const geometry = new BufferGeometry();
  geometry.setAttribute('position', new Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3));
  const mesh = new Mesh(geometry, material);
  mesh.frustumCulled = false;
  const scene = new Scene();
  scene.add(mesh);
  const camera = new OrthographicCamera(-1, 1, 1, -1, 0, 1);
  const target = new WebGLRenderTarget(view?.width ?? info.width, view?.height ?? info.height, {
    colorSpace: SRGBColorSpace,
    depthBuffer: false,
    generateMipmaps: true,
    minFilter: LinearMipmapLinearFilter,
    magFilter: LinearFilter,
  });
  let last = '';
  let custom = false;
  const keys = ['unit', 'ramp_low', 'ramp_high', 'reach', 'grain_amount', 'grain_scale', 'grain_floor', 'core_lift', 'core_drop', 'volume_lit', 'volume_dark', 'shadow_at', 'rim_at', 'belly_at'] as const;
  const defaults = Object.fromEntries(keys.map((key) => [key, uniforms[key].value]));

  const bind = (suffix: 'a' | 'b', look: Look) => {
    uniforms[`map_${suffix}`].value = look.map;
    uniforms[`field_${suffix}`].value = look.field;
    uniforms[`look_${suffix}`].value = look.look;
    uniforms[`pad_${suffix}`].value = look.pad;
    uniforms[`paints_${suffix}`].value = look.paints;
    uniforms[`ramps_${suffix}`].value = look.ramps;
  };

  const select = (place: number) => {
    const count = scenery.states.length;
    const clamped = Math.min(Math.max(place, 0), count - 1);
    const from = Math.min(Math.floor(clamped), Math.max(count - 2, 0));
    const to = Math.min(from + 1, count - 1);
    return { from, to, s: count > 1 ? clamped - from : 0 };
  };

  const textures = () => {
    const found = new Set<Texture>([scenery.canvas]);
    for (const look of scenery.states) for (const texture of Object.values(look)) found.add(texture);
    return [...found];
  };

  return {
    texture: target.texture,
    material,
    select,
    textures,
    compile(gl: WebGLRenderer) {
      const previous = gl.getRenderTarget();
      gl.setRenderTarget(target);
      const pending = gl.compileAsync(scene, camera);
      gl.setRenderTarget(previous);
      return pending;
    },
    render(gl: WebGLRenderer, place: number, part?: { width: number; height: number }) {
      if (custom) {
        custom = false;
        for (const key of keys) (uniforms[key] as { value: unknown }).value = defaults[key];
      }
      const { from, to, s } = select(place);
      const key = `${from}:${to}:${s.toFixed(4)}:${scenery.states.length}:${part ? `${part.width}x${part.height}` : 'full'}`;
      if (key === last) return false;
      last = key;
      bind('a', scenery.states[from]);
      bind('b', scenery.states[to]);
      uniforms.s.value = s;
      const full = uniforms.view.value;
      if (part) {
        target.viewport.set(0, 0, part.width, part.height);
        uniforms.view.value = [part.width, part.height];
      }
      const previous = gl.getRenderTarget();
      gl.setRenderTarget(target);
      gl.render(scene, camera);
      gl.setRenderTarget(previous);
      if (part) {
        target.viewport.set(0, 0, target.width, target.height);
        uniforms.view.value = full;
      }
      return true;
    },
    async renderIdle(gl: WebGLRenderer, place: number, pixels = 120000) {
      const { from, to, s } = select(place);
      last = `${from}:${to}:${s.toFixed(4)}:${scenery.states.length}`;
      const rows = Math.max(8, Math.floor(pixels / target.width));
      for (let y = 0; y < target.height; y += rows) {
        await quiet();
        bind('a', scenery.states[from]);
        bind('b', scenery.states[to]);
        uniforms.s.value = s;
        target.scissor.set(0, y, target.width, Math.min(rows, target.height - y));
        target.scissorTest = true;
        const previous = gl.getRenderTarget();
        gl.setRenderTarget(target);
        gl.render(scene, camera);
        gl.setRenderTarget(previous);
      }
      target.scissorTest = false;
    },
    renderLooks(gl: WebGLRenderer, a: Look, b: Look, s: number, constants: Float32Array | null, part?: { width: number; height: number }) {
      last = '';
      bind('a', a);
      bind('b', b);
      uniforms.s.value = s;
      if (constants) {
        custom = true;
        uniforms.unit.value = constants[7];
        uniforms.ramp_low.value = constants[8];
        uniforms.ramp_high.value = constants[9];
        uniforms.reach.value = constants[10];
        uniforms.grain_amount.value = constants[11];
        uniforms.grain_scale.value = constants[12];
        uniforms.grain_floor.value = constants[13];
        uniforms.core_lift.value = constants[15];
        uniforms.core_drop.value = constants[16];
        uniforms.volume_lit.value = constants[17];
        uniforms.volume_dark.value = constants[18];
        uniforms.shadow_at.value = [constants[20], constants[21]];
        uniforms.rim_at.value = [constants[22], constants[23]];
        uniforms.belly_at.value = [constants[24], constants[25]];
      }
      const full = uniforms.view.value;
      if (part) {
        target.viewport.set(0, 0, part.width, part.height);
        uniforms.view.value = [part.width, part.height];
      }
      const previous = gl.getRenderTarget();
      gl.setRenderTarget(target);
      gl.render(scene, camera);
      gl.setRenderTarget(previous);
      if (part) {
        target.viewport.set(0, 0, target.width, target.height);
        uniforms.view.value = full;
      }
    },
    states: scenery.states,
    scenery,
    dispose() {
      target.dispose();
      geometry.dispose();
      material.dispose();
    },
  };
}

export type Frame = ReturnType<typeof createFrame>;

export type Tables = {
  width: number;
  height: number;
  gridWidth: number;
  gridHeight: number;
  paintWidth: number;
  paintHeight: number;
  mapWidth: number;
  canvasFits: boolean;
  map: Float32Array;
  field: Uint16Array;
  look: Uint8Array;
  pad: Uint8Array;
  paints: Uint8Array;
  ramps: Uint8Array;
  constants: Float32Array;
};

export function lookFromTables(tables: Tables): Look {
  const rgba = (bytes: Uint8Array, width: number, height: number, srgb: boolean) => {
    const texture = data(new DataTexture(bytes, width, height, RGBAFormat, UnsignedByteType), LinearFilter);
    texture.colorSpace = srgb ? SRGBColorSpace : NoColorSpace;
    return texture;
  };
  const paints = data(new DataArrayTexture(tables.paints, tables.paintWidth, tables.paintHeight, 8), LinearFilter);
  paints.format = RGBAFormat;
  paints.type = UnsignedByteType;
  paints.colorSpace = SRGBColorSpace;
  return {
    map: data(new DataTexture(tables.map, tables.mapWidth, 2, RedFormat, FloatType), NearestFilter),
    field: data(new DataTexture(tables.field, tables.gridWidth, tables.gridHeight, RedFormat, HalfFloatType), LinearFilter),
    look: rgba(tables.look, tables.gridWidth, tables.gridHeight, false),
    pad: rgba(tables.pad, tables.gridWidth, tables.gridHeight, true),
    paints,
    ramps: rgba(tables.ramps, tables.ramps.length / 8, 2, false),
  };
}

export function disposeLook(look: Look) {
  for (const texture of Object.values(look)) texture.dispose();
}
