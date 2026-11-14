import { mkdir, readFile, stat, writeFile } from 'node:fs/promises';
import { dirname, join as joinPath, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { NodeIO } from '@gltf-transform/core';
import { ALL_EXTENSIONS } from '@gltf-transform/extensions';
import { dedup, flatten, join, meshopt, prune, textureCompress, weld } from '@gltf-transform/functions';
import { ktx2 } from 'ktx2-encoder/gltf-transform';
import { MeshoptEncoder } from 'meshoptimizer';
import sharp from 'sharp';

const site = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const cache = joinPath(site, '.cache', 'polyhaven');
const out = joinPath(site, 'public', 'models');
const agent = { 'User-Agent': 'scooch-site-build/1.0' };

const small = { color: 512, normal: 256, arm: 256 };

const models = [
  {
    id: 'classic_laptop',
    resolution: '4k',
    file: 'laptop.glb',
    textures: { color: 2048, normal: 512, arm: 1024 },
    pivots: ['classic_laptop_screen'],
    lite: true,
  },
  {
    id: 'metal_office_desk',
    resolution: '2k',
    file: 'desk.glb',
    textures: { color: 2048, normal: 1024, arm: 1024 },
    merge: true,
    lite: true,
  },
  {
    id: 'desk_lamp_arm_01',
    resolution: '2k',
    file: 'lamp.glb',
    textures: { color: 1024, normal: 512, arm: 512 },
    lite: true,
  },
  {
    id: 'television_02',
    resolution: '2k',
    file: 'television.glb',
    textures: { color: 1024, normal: 1024, arm: 512 },
    glass: { material: 'television_02_screen', uv: [0.68, 1, 0, 0.25], facing: 0.5 },
    lite: true,
  },
  {
    id: 'office_notepads',
    resolution: '1k',
    file: 'notepads.glb',
    textures: { color: 1024, normal: 256, arm: 256 },
    keep: ['office_notepads_yellow_pad', 'office_notepads_note_stack', 'office_notepads_sticky_stack'],
  },
  {
    id: 'stationery_supplies',
    resolution: '1k',
    file: 'stationery.glb',
    textures: { color: 512, normal: 256, arm: 256 },
    keep: [
      'stationery_supplies_pencilcup',
      'stationery_supplies_pencil_new_a',
      'stationery_supplies_pencil_used',
      'stationery_supplies_pen_fancy',
      'stationery_supplies_eraser',
    ],
  },
  { id: 'alarm_clock_01', resolution: '1k', file: 'clock.glb', textures: small },
  { id: 'cassette_player', resolution: '1k', file: 'cassette.glb', textures: small, merge: true },
  { id: 'potted_plant_04', resolution: '1k', file: 'plant.glb', textures: small, merge: true },
  { id: 'vintage_stapler', resolution: '1k', file: 'stapler.glb', textures: small, merge: true },
  { id: 'round_spectacles', resolution: '1k', file: 'glasses.glb', textures: small, merge: true },
  { id: 'digital_wrist_watch', resolution: '1k', file: 'watch.glb', textures: small, merge: true },
  { id: 'gamepad', resolution: '1k', file: 'gamepad.glb', textures: small, merge: true },
  {
    id: 'Television_01',
    resolution: '1k',
    file: 'console.glb',
    textures: { color: 1024, normal: 512, arm: 512 },
    glass: { material: 'television_01_screen', uv: [0.04, 0.338, 0.712, 0.962], facing: 0.7 },
    paint: [[0.494, 0.526, 0.733, 0.838, 0.51, 0.722]],
  },
  { id: 'tea_set_01', resolution: '1k', file: 'cup.glb', textures: small, keep: ['tea_set_01_cup_small_01'] },
];

const environment = { id: 'studio_small_08', resolution: '1k', file: 'studio.hdr', width: 256 };

const variants = [
  { name: 'full', folder: out, scale: 1 },
  { name: 'lite', folder: joinPath(out, 'lite'), scale: 0.5 },
];
const only = process.argv.find((arg) => arg.startsWith('--variant='))?.slice(10);

async function exists(path) {
  return stat(path).then(() => true, () => false);
}

async function download(url, path) {
  if (await exists(path)) return;
  const response = await fetch(url, { headers: agent });
  if (!response.ok) throw new Error(`${response.status} ${url}`);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, Buffer.from(await response.arrayBuffer()));
}

async function listing(id) {
  const response = await fetch(`https://api.polyhaven.com/files/${id}`, { headers: agent });
  if (!response.ok) throw new Error(`${response.status} files/${id}`);
  return response.json();
}

async function paintOver(path, rects) {
  const image = sharp(await readFile(path));
  const { width, height } = await image.metadata();
  const layers = [];
  for (const [u0, u1, v0, v1, su, sv] of rects) {
    const sample = await sharp(await readFile(path)).extract({ left: Math.round(su * width), top: Math.round(sv * height), width: 4, height: 4 }).resize(1, 1).raw().toBuffer();
    const w = Math.round((u1 - u0) * width);
    const h = Math.round((v1 - v0) * height);
    layers.push({ input: { create: { width: w, height: h, channels: 3, background: { r: sample[0], g: sample[1], b: sample[2] } } }, left: Math.round(u0 * width), top: Math.round(v0 * height) });
  }
  await writeFile(path, await image.composite(layers).jpeg({ quality: 95 }).toBuffer());
}

async function fetchModel(model) {
  const entry = (await listing(model.id)).gltf[model.resolution].gltf;
  const folder = joinPath(cache, model.id, model.resolution);
  const main = joinPath(folder, entry.url.split('/').pop());
  await download(entry.url, main);
  for (const [relative, file] of Object.entries(entry.include)) {
    const path = joinPath(folder, relative);
    const fresh = !(await exists(path));
    await download(file.url, path);
    if (fresh && model.paint && /diff/.test(relative)) await paintOver(path, model.paint);
  }
  return main;
}

function addPivots(document, names) {
  const scene = document.getRoot().listScenes()[0];
  for (const name of names) {
    const node = document.getRoot().listNodes().find((candidate) => candidate.getName() === name);
    const pivot = document.createNode(`${name}_pivot`).setTranslation(node.getTranslation());
    node.setTranslation([0, 0, 0]);
    scene.removeChild(node);
    pivot.addChild(node);
    scene.addChild(pivot);
  }
}

function keepOnly(document, names) {
  for (const node of document.getRoot().listNodes()) if (!names.includes(node.getName())) node.dispose();
}

function splitGlass(document, glass) {
  const [u0, u1, v0, v1] = glass.uv;
  for (const mesh of document.getRoot().listMeshes()) {
    for (const primitive of mesh.listPrimitives()) {
      const indices = primitive.getIndices();
      const uv = primitive.getAttribute('TEXCOORD_0');
      const normal = primitive.getAttribute('NORMAL');
      const keepers = [];
      const screen = [];
      const coordinate = [];
      const direction = [];
      for (let triangle = 0; triangle < indices.getCount() / 3; triangle++) {
        const corners = [0, 1, 2].map((corner) => indices.getScalar(triangle * 3 + corner));
        let u = 0;
        let v = 0;
        let z = 0;
        for (const index of corners) {
          uv.getElement(index, coordinate);
          normal.getElement(index, direction);
          u += coordinate[0] / 3;
          v += coordinate[1] / 3;
          z += direction[2] / 3;
        }
        const inside = u >= u0 && u <= u1 && v >= v0 && v <= v1 && z > glass.facing;
        (inside ? screen : keepers).push(...corners);
      }
      if (screen.length === 0) continue;
      const IndexArray = indices.getArray().constructor;
      indices.setArray(new IndexArray(keepers));
      const material = document.createMaterial(glass.material).setBaseColorFactor([0.02, 0.02, 0.02, 1]).setRoughnessFactor(0.1).setMetallicFactor(0);
      const split = primitive.clone().setIndices(document.createAccessor().setType('SCALAR').setArray(new IndexArray(screen)).setBuffer(indices.getBuffer())).setMaterial(material);
      mesh.addPrimitive(split);
    }
  }
}

function plainGlass(document) {
  for (const material of document.getRoot().listMaterials()) {
    if (material.getAlphaMode() !== 'BLEND') continue;
    material.setBaseColorTexture(null).setMetallicRoughnessTexture(null).setNormalTexture(null).setOcclusionTexture(null);
    material.setBaseColorFactor([0.86, 0.9, 0.92, 0.18]).setRoughnessFactor(0.06).setMetallicFactor(0);
    for (const extension of material.listExtensions()) material.setExtension(extension.extensionName, null);
  }
}

function shareOcclusion(document) {
  for (const material of document.getRoot().listMaterials()) {
    const packed = material.getMetallicRoughnessTexture();
    if (packed) material.setOcclusionTexture(packed);
  }
}

async function imageDecoder(buffer) {
  const { data, info } = await sharp(buffer).ensureAlpha().raw().toBuffer({ resolveWithObject: true });
  return { data: new Uint8Array(data), width: info.width, height: info.height };
}

function resized(slots, size) {
  return textureCompress({ encoder: sharp, targetFormat: 'png', slots, resize: [size, size] });
}

const color = { isUASTC: false, qualityLevel: 210, compressionLevel: 3, isSetKTX2SRGBTransferFunc: true, isPerceptual: true, generateMipmap: true, imageDecoder };
const data = { isUASTC: false, qualityLevel: 190, compressionLevel: 3, isSetKTX2SRGBTransferFunc: false, isPerceptual: false, generateMipmap: true, imageDecoder };
const normal = { isUASTC: true, needSupercompression: true, enableRDO: true, rdoQualityLevel: 2.5, isSetKTX2SRGBTransferFunc: false, isPerceptual: false, generateMipmap: true, imageDecoder };

async function buildModel(io, model, variant) {
  const document = await io.read(await fetchModel(model));
  if (model.keep) keepOnly(document, model.keep);
  if (model.pivots) addPivots(document, model.pivots);
  if (model.glass) splitGlass(document, model.glass);
  plainGlass(document);
  shareOcclusion(document);
  const steps = [prune(), dedup()];
  if (model.merge) steps.push(flatten(), join());
  steps.push(
    weld(),
    resized(/baseColor/, model.textures.color * variant.scale),
    resized(/normal/, model.textures.normal * variant.scale),
    resized(/metallicRoughness|occlusion/, model.textures.arm * variant.scale),
    ktx2({ ...color, slots: /baseColor/ }),
    ktx2({ ...data, slots: /metallicRoughness|occlusion/ }),
    ktx2({ ...normal, slots: /normal/ }),
    meshopt({ encoder: MeshoptEncoder, level: 'medium' }),
  );
  await document.transform(...steps);
  const bytes = await io.writeBinary(document);
  await writeFile(joinPath(variant.folder, model.file), bytes);
  return bytes.byteLength;
}

function readRadiance(buffer) {
  let cursor = 0;
  const line = () => {
    const end = buffer.indexOf(10, cursor);
    const text = buffer.toString('latin1', cursor, end);
    cursor = end + 1;
    return text;
  };
  while (line() !== '');
  const [, height, , width] = line().split(' ').map(Number);
  const pixels = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y++) {
    cursor += 4;
    for (let channel = 0; channel < 4; channel++) {
      let x = 0;
      while (x < width) {
        let count = buffer[cursor++];
        if (count > 128) {
          count -= 128;
          const value = buffer[cursor++];
          while (count--) pixels[(y * width + x++) * 4 + channel] = value;
        } else {
          while (count--) pixels[(y * width + x++) * 4 + channel] = buffer[cursor++];
        }
      }
    }
  }
  return { width, height, pixels };
}

function toLinear(pixels, count) {
  const linear = new Float32Array(count * 3);
  for (let i = 0; i < count; i++) {
    const scale = pixels[i * 4 + 3] ? 2 ** (pixels[i * 4 + 3] - 136) : 0;
    for (let c = 0; c < 3; c++) linear[i * 3 + c] = pixels[i * 4 + c] * scale;
  }
  return linear;
}

function shrink(image, factor) {
  const width = image.width / factor;
  const height = image.height / factor;
  const source = toLinear(image.pixels, image.width * image.height);
  const pixels = new Uint8Array(width * height * 4);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const sum = [0, 0, 0];
      for (let dy = 0; dy < factor; dy++) {
        for (let dx = 0; dx < factor; dx++) {
          const at = ((y * factor + dy) * image.width + x * factor + dx) * 3;
          for (let c = 0; c < 3; c++) sum[c] += source[at + c];
        }
      }
      const rgb = sum.map((value) => value / (factor * factor));
      const peak = Math.max(...rgb);
      const at = (y * width + x) * 4;
      if (peak < 1e-32) continue;
      const exponent = Math.ceil(Math.log2(peak) + 1e-9);
      const scale = 256 / 2 ** exponent;
      for (let c = 0; c < 3; c++) pixels[at + c] = Math.min(255, Math.floor(rgb[c] * scale));
      pixels[at + 3] = exponent + 128;
    }
  }
  return { width, height, pixels };
}

function writeRadiance(image) {
  const header = Buffer.from(`#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y ${image.height} +X ${image.width}\n`, 'latin1');
  return Buffer.concat([header, Buffer.from(image.pixels)]);
}

async function buildEnvironment() {
  const entry = (await listing(environment.id)).hdri[environment.resolution].hdr;
  const source = joinPath(cache, environment.id, entry.url.split('/').pop());
  await download(entry.url, source);
  const image = readRadiance(await readFile(source));
  const bytes = writeRadiance(shrink(image, image.width / environment.width));
  await mkdir(joinPath(site, 'public', 'hdri'), { recursive: true });
  await writeFile(joinPath(site, 'public', 'hdri', environment.file), bytes);
  return bytes.byteLength;
}

await MeshoptEncoder.ready;
const io = new NodeIO().registerExtensions(ALL_EXTENSIONS).registerDependencies({ 'meshopt.encoder': MeshoptEncoder });
for (const variant of variants.filter((candidate) => !only || candidate.name === only)) {
  await mkdir(variant.folder, { recursive: true });
  let total = 0;
  for (const model of models.filter((candidate) => variant.name === 'full' || candidate.lite)) {
    if (process.argv.includes('--only') && !process.argv.includes(model.file)) continue;
    const size = await buildModel(io, model, variant);
    total += size;
    console.log(variant.name, model.file, (size / 1e6).toFixed(2), 'MB');
  }
  console.log(variant.name, 'total', (total / 1e6).toFixed(2), 'MB');
}
if (!only) {
  const environmentSize = await buildEnvironment();
  console.log(environment.file, (environmentSize / 1e6).toFixed(2), 'MB');
}
