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

const models = [
  {
    id: 'classic_laptop',
    resolution: '4k',
    file: 'laptop.glb',
    textures: { color: 2048, normal: 1024, arm: 1024 },
    pivots: ['classic_laptop_screen'],
  },
  {
    id: 'metal_office_desk',
    resolution: '2k',
    file: 'desk.glb',
    textures: { color: 2048, normal: 1024, arm: 1024 },
    merge: true,
  },
  {
    id: 'desk_lamp_arm_01',
    resolution: '2k',
    file: 'lamp.glb',
    textures: { color: 1024, normal: 1024, arm: 1024 },
  },
  {
    id: 'office_notepads',
    resolution: '1k',
    file: 'notepads.glb',
    textures: { color: 1024, normal: 512, arm: 512 },
    keep: ['office_notepads_yellow_pad', 'office_notepads_note_stack', 'office_notepads_sticky_stack'],
  },
  {
    id: 'stationery_supplies',
    resolution: '1k',
    file: 'stationery.glb',
    textures: { color: 1024, normal: 512, arm: 512 },
    keep: [
      'stationery_supplies_pencilcup',
      'stationery_supplies_pencil_new_a',
      'stationery_supplies_pencil_used',
      'stationery_supplies_pen_fancy',
      'stationery_supplies_eraser',
    ],
  },
];

const environment = { id: 'studio_small_08', resolution: '1k', file: 'studio.hdr', width: 512 };

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

async function fetchModel(model) {
  const entry = (await listing(model.id)).gltf[model.resolution].gltf;
  const folder = joinPath(cache, model.id, model.resolution);
  const main = joinPath(folder, entry.url.split('/').pop());
  await download(entry.url, main);
  for (const [relative, file] of Object.entries(entry.include)) await download(file.url, joinPath(folder, relative));
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
  for (const model of models) {
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
