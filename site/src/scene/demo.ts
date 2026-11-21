import { Texture, type Matrix4, type PerspectiveCamera, type Quaternion, type Vector2, type Vector3, type Vector4 } from 'three';
import { asset } from '@/lib/base';
import { shot } from '@/lib/audio';
import { changed, story } from '@/story/state';
import { ask, begin, dragged, end, held } from './drag';
import { warm } from './engine';
import { quality } from './quality';
import { createFrame, loadScenery, type Frame } from './screen/scenery';
import { CHANNELS, hooks, live } from './touch';

const GLIDE = 600;
const BLEND = 420;
const BLEND_REDUCED = 200;
const SWITCH = 200;
const OWN = 'canoe';
const FILL = { height: 0.78, width: 0.92 };
const STEP = 1 / 40;
const reduced = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches;

type Uniforms = Record<string, { value: unknown }>;
type Listener = () => void;

export const demo = {
  open: false,
  since: 0,
  closing: 0,
  used: false,
  amount: 0,
  listeners: new Set<Listener>(),
  version: 0,
  camera: null as PerspectiveCamera | null,
  picture: OWN,
};

export const PICTURES = CHANNELS;

type Slot = { id: string; frame: Frame | null; icons: Vector4[]; base: Vector4[]; gone: boolean };
const slots = new Map<string, Slot>();
const view = { frame: null as Frame | null, main: null as Frame | null };

const saved = { frame: null as Texture | null, icons: [] as Vector4[], scale: null as Vector2 | null, grid: null as Vector2 | null, atlas: null as Texture | null, opener: null as HTMLElement | null };
const sharper = { atlas: null as Texture | null, loading: false };
const fade = { mix: 0, at: 0, target: 0, span: BLEND };
const work = {} as { target: Vector3; center: Vector3; normal: Vector3; up: Vector3; point: Vector3; ray: Vector3; quaternion: Quaternion; matrix: Matrix4; answers: number; timer: number; nudged: number };

function ready() {
  if (work.target || !hooks.laptop) return;
  const mesh = hooks.laptop;
  const vector = () => mesh.position.clone().set(0, 0, 0);
  Object.assign(work, { target: vector(), center: vector(), normal: vector(), up: vector(), point: vector(), ray: vector(), quaternion: mesh.quaternion.clone(), matrix: mesh.matrix.clone(), answers: 0, timer: 0, nudged: -1 });
}
const drag = { index: -1, dx: 0, dy: 0, x: 0, y: 0, moved: false, slid: false, pointer: -1 };

function emit() {
  demo.version += 1;
  for (const listener of demo.listeners) listener();
}

export function subscribe(listener: Listener) {
  demo.listeners.add(listener);
  return () => {
    demo.listeners.delete(listener);
  };
}

function lcd() {
  return hooks.screen?.lcd as Uniforms | undefined;
}

function crt() {
  return hooks.screen?.crt as Uniforms | undefined;
}

function screenFrame() {
  ready();
  const mesh = hooks.laptop;
  const uniforms = lcd();
  if (!mesh || !uniforms) return null;
  const origin = uniforms.uOrigin.value as Vector2;
  const extent = uniforms.uExtent.value as Vector2;
  if (!mesh.geometry.boundingBox) mesh.geometry.computeBoundingBox();
  const front = mesh.geometry.boundingBox?.max.z ?? 0;
  mesh.updateWorldMatrix(true, false);
  const world = mesh.matrixWorld;
  work.center.set(origin.x + extent.x / 2, origin.y + extent.y / 2, front).applyMatrix4(world);
  work.normal.set(0, 0, 1).transformDirection(world);
  work.up.set(0, 1, 0).transformDirection(world);
  const sx = work.point.setFromMatrixColumn(world, 0).length();
  const sy = work.point.setFromMatrixColumn(world, 1).length();
  return { width: extent.x * sx, height: extent.y * sy, origin, extent, front };
}

function ease(t: number) {
  const u = Math.min(Math.max(t, 0), 1);
  return 1 - (1 - u) ** 3;
}

function level(now: number) {
  if (demo.open) {
    if (reduced()) return 1;
    return ease((now - demo.since) / GLIDE);
  }
  if (demo.closing) {
    const t = (now - demo.closing) / GLIDE;
    if (t >= 1 || reduced()) {
      finish();
      return 0;
    }
    return 1 - ease(t);
  }
  return 0;
}

function glide(camera: PerspectiveCamera) {
  demo.camera = camera;
  const now = performance.now();
  const amount = level(now);
  demo.amount = amount;
  if (amount <= 0) return;
  const frame = screenFrame();
  if (!frame) return;
  if (work.normal.dot(work.point.copy(camera.position).sub(work.center)) < 0) work.normal.negate();
  const half = Math.tan((camera.fov * Math.PI) / 360);
  const tall = camera.aspect < 0.9;
  const distance = tall ? frame.width / FILL.width / 2 / (half * camera.aspect) : frame.height / FILL.height / 2 / half;
  work.target.copy(work.center).addScaledVector(work.normal, distance);
  work.matrix.lookAt(work.target, work.center, work.up);
  work.quaternion.setFromRotationMatrix(work.matrix);
  camera.position.lerp(work.target, amount);
  camera.quaternion.slerp(work.quaternion, amount);
  if (camera.view) {
    camera.view.offsetX *= 1 - amount;
    camera.view.offsetY *= 1 - amount;
  }
  camera.updateProjectionMatrix();
  camera.updateMatrixWorld();
}

function infoOf(id: string) {
  const manifest = hooks.manifest;
  if (!manifest) return null;
  return id === 'main' ? manifest.scenery : (manifest.pictures.find((picture) => picture.id === id)?.scenery ?? null);
}

export function thumbOf(id: string) {
  const info = infoOf(id);
  return info ? `demo/${info.folder}/${info.canvas.file}` : null;
}

function layoutOf(id: string) {
  const manifest = hooks.manifest;
  if (!manifest) return null;
  return id === 'main' ? manifest.frames.before.icons : (manifest.pictures.find((picture) => picture.id === id)?.icons ?? null);
}

function slotOf(id: string) {
  const existing = slots.get(id);
  if (existing) return existing;
  const screen = lcd();
  const layout = layoutOf(id);
  const manifest = hooks.manifest;
  if (!screen || !layout || !manifest) return null;
  const sample = (screen.uIcons.value as Vector4[])[0];
  const base = manifest.icons.map((icon) => {
    const [x, y, w, h] = layout[icon.id];
    return sample.clone().set(x, y, w, h);
  });
  const slot: Slot = { id, frame: null, icons: base.map((icon) => icon.clone()), base, gone: false };
  slots.set(id, slot);
  void build(slot);
  return slot;
}

function neededRows() {
  const frame = screenFrame();
  const aspect = frame ? frame.width / frame.height : 4 / 3;
  const tall = window.innerWidth / window.innerHeight < 0.9;
  const rows = tall ? (FILL.width * window.innerWidth) / aspect : FILL.height * window.innerHeight;
  return rows * Math.max(quality.nativeDpr, 1);
}

async function sharpenAtlas(screen: Uniforms) {
  const atlas = screen.uAtlas.value as Texture | null;
  const slot = current();
  const frame = screenFrame();
  if (!atlas || !slot || !frame || sharper.loading || sharper.atlas) return;
  const image = atlas.image as { width: number };
  const widest = Math.max(...slot.base.map((icon) => icon.z));
  const iconPixels = widest * neededRows() * (frame.width / frame.height);
  if (image.width / 4 >= iconPixels) return;
  sharper.loading = true;
  const element = new Image();
  element.src = asset('demo/atlas-2x.png');
  try {
    await element.decode();
  } catch {
    sharper.loading = false;
    return;
  }
  sharper.loading = false;
  if (!demo.open) return;
  const texture = new Texture(element);
  texture.colorSpace = atlas.colorSpace;
  texture.flipY = atlas.flipY;
  texture.generateMipmaps = atlas.generateMipmaps;
  texture.minFilter = atlas.minFilter;
  texture.magFilter = atlas.magFilter;
  texture.anisotropy = atlas.anisotropy;
  texture.wrapS = atlas.wrapS;
  texture.wrapT = atlas.wrapT;
  texture.needsUpdate = true;
  sharper.atlas = texture;
  saved.atlas = atlas;
  screen.uAtlas.value = texture;
  changed();
}

async function build(slot: Slot) {
  const gl = hooks.gl;
  const info = infoOf(slot.id);
  const main = view.main;
  if (!gl || !info || !main) return;
  void warm(info, rects(slot, -1, 0, 0)).catch(() => undefined);
  const size = main.live.image as { width: number; height: number };
  const large = main.rows !== size.height ? { width: Math.round((main.rows * info.width) / info.height), height: main.rows } : undefined;
  const need = neededRows();
  const rows = large ? size.height : Math.min(Math.max(size.height, Math.ceil(need)), info.height * 2);
  const scenery = await loadScenery(info, false);
  if (slot.gone) return;
  const frame = createFrame(scenery, { width: Math.round((rows * info.width) / info.height), height: rows }, large);
  frame.pin(0);
  frame.prepare(gl);
  await frame.compile(gl);
  await frame.renderIdle(gl, 0);
  if (slot.gone) {
    discard(frame);
    return;
  }
  slot.frame = frame;
  const shown = Math.min(large ? large.height : rows, need);
  const canvas = scenery.canvas.image as { height: number };
  if (canvas.height < shown * 0.98) void frame.sharpen(gl).catch(() => false);
}

function discard(frame: Frame) {
  if (held.frame === frame) end();
  for (const texture of frame.textures()) texture.dispose();
  frame.dispose();
}

function retire(slot: Slot) {
  slot.gone = true;
  slots.delete(slot.id);
  if (slot.frame && slot.frame !== view.frame) discard(slot.frame);
  else if (slot.frame) retired.add(slot.frame);
}

const retired = new Set<Frame>();

function current() {
  return slots.get(demo.picture) ?? null;
}

function startFade(target: number, span: number) {
  fade.target = target;
  fade.span = span;
  live.fadeUntil = Math.max(live.fadeUntil, performance.now() + span + 150);
  changed();
}

function holdFrom(screen: Uniforms) {
  screen.uFrom.value = screen.uFrame.value;
  (screen.uFromScale.value as Vector2).copy(screen.uFrameScale.value as Vector2);
  const from = screen.uFromIcons.value as Vector4[];
  (screen.uIcons.value as Vector4[]).forEach((icon, i) => from[i]?.copy(icon));
}

function after() {
  const screen = lcd();
  const gl = hooks.gl;
  if (!screen || !gl || demo.amount <= 0) return;
  const now = performance.now();
  const slot = current();
  if (demo.open && slot?.frame && slot.frame !== view.frame) {
    holdFrom(screen);
    fade.mix = 1;
    startFade(0, SWITCH);
    view.frame = slot.frame;
    hooks.frame = slot.frame;
  }
  const frame = view.frame;
  if (frame && slot && slot.frame === frame) {
    const holding = held.active && held.frame === frame;
    if (demo.open && !holding) frame.render(gl, 0);
    screen.uFrame.value = frame.texture;
    const icons = screen.uIcons.value as Vector4[];
    slot.icons.forEach((icon, i) => icons[i]?.copy(icon));
    (screen.uFrameScale.value as Vector2).set(holding ? hooks.scale[0] : 1, holding ? hooks.scale[1] : 1);
  }
  const step = (now - fade.at) / (reduced() ? Math.min(fade.span, BLEND_REDUCED) : fade.span);
  fade.at = now;
  fade.mix = fade.target > fade.mix ? Math.min(fade.mix + step, fade.target) : Math.max(fade.mix - step, fade.target);
  screen.uFromMix.value = fade.mix * fade.mix * (3 - 2 * fade.mix);
  if (fade.mix <= 0 && retired.size) {
    for (const old of [...retired]) {
      if (old === view.frame || [...slots.values()].some((kept) => kept.frame === old)) continue;
      retired.delete(old);
      discard(old);
    }
  }
  if (demo.camera) quality.focus(work.center, demo.camera.position, 1, work.center, work.point, story.dof * (1 - demo.amount));
  if (held.answers !== work.answers) {
    work.answers = held.answers;
    if (drag.moved && !drag.slid) {
      drag.slid = true;
      void shot('slide', { gain: 0.22 });
    }
  }
}

export function choose(id: string) {
  if (!demo.open || id === demo.picture || !PICTURES.includes(id)) return;
  if (held.active) end();
  drag.index = -1;
  live.pressing = false;
  live.dragging = false;
  const previous = current();
  demo.picture = id;
  const slot = slotOf(id);
  if (slot) slot.icons.forEach((icon, i) => icon.copy(slot.base[i]));
  if (previous && previous.id !== OWN) retire(previous);
  emit();
}

function fit(buffer: Vector2, size: { width: number; height: number }) {
  const camera = demo.camera;
  const frame = screenFrame();
  const mesh = hooks.laptop;
  if (!camera || !frame || !mesh) return 1;
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;
  for (const [u, v] of [[0, 0], [1, 0], [0, 1], [1, 1]]) {
    work.point.set(frame.origin.x + u * frame.extent.x, frame.origin.y + v * frame.extent.y, frame.front).applyMatrix4(mesh.matrixWorld).project(camera);
    minX = Math.min(minX, work.point.x);
    maxX = Math.max(maxX, work.point.x);
    minY = Math.min(minY, work.point.y);
    maxY = Math.max(maxY, work.point.y);
  }
  return Math.max(((maxX - minX) / 2) * buffer.x / size.width, ((maxY - minY) / 2) * buffer.y / size.height);
}

function rects(slot: Slot, index: number, x: number, y: number) {
  const out = new Float32Array(slot.icons.length * 4);
  slot.icons.forEach((rect, i) => {
    out[i * 4] = (i === index ? x : rect.x) * 1280;
    out[i * 4 + 1] = (i === index ? y : rect.y) * 960;
    out[i * 4 + 2] = rect.z * 1280;
    out[i * 4 + 3] = rect.w * 960;
  });
  return out;
}

function close() {
  if (!demo.open) return;
  demo.open = false;
  demo.closing = performance.now();
  live.fadeUntil = Math.max(live.fadeUntil, demo.closing + GLIDE + 150);
  document.documentElement.classList.remove('demo-open');
  if (saved.opener?.isConnected) saved.opener.focus({ preventScroll: true });
  saved.opener = null;
  const screen = lcd();
  if (screen && saved.frame) {
    screen.uFrom.value = saved.frame;
    if (saved.scale) (screen.uFromScale.value as Vector2).copy(saved.scale);
    const from = screen.uFromIcons.value as Vector4[];
    saved.icons.forEach((icon, i) => from[i]?.copy(icon));
    startFade(1, BLEND);
  }
  drag.index = -1;
  live.pressing = false;
  live.dragging = false;
  window.removeEventListener('scroll', onScroll);
  window.removeEventListener('keydown', onKey);
  live.lastScroll = -1e9;
  emit();
}

function finish() {
  demo.closing = 0;
  demo.amount = 0;
  const screen = lcd();
  if (screen && saved.frame) {
    screen.uFrame.value = saved.frame;
    const icons = screen.uIcons.value as Vector4[];
    saved.icons.forEach((icon, i) => icons[i]?.copy(icon));
    if (saved.scale) (screen.uFrameScale.value as Vector2).copy(saved.scale);
    if (saved.grid) (screen.uGrid.value as Vector2).copy(saved.grid);
    screen.uFromMix.value = 0;
    screen.uSharpIcons.value = 0;
    if (saved.atlas) screen.uAtlas.value = saved.atlas;
  }
  saved.atlas = null;
  if (sharper.atlas) {
    sharper.atlas.dispose();
    sharper.atlas = null;
  }
  fade.mix = 0;
  fade.target = 0;
  live.crisp = false;
  if (held.active) end();
  if (view.main) hooks.frame = view.main;
  for (const slot of [...slots.values()]) {
    slot.gone = true;
    if (slot.frame) discard(slot.frame);
  }
  slots.clear();
  for (const old of retired) discard(old);
  retired.clear();
  view.frame = null;
  view.main = null;
  demo.picture = OWN;
  hooks.view = () => {};
  hooks.after = () => {};
  hooks.fit = null;
  emit();
}

function onScroll() {
  close();
}

function onKey(event: KeyboardEvent) {
  if (event.key === 'Escape') close();
}

export function openDemo() {
  if (demo.open || !hooks.laptop || !hooks.frame || !hooks.screen) return;
  const screen = lcd();
  if (!screen) return;
  if (!demo.closing) {
    saved.frame = screen.uFrame.value as Texture;
    saved.icons = (screen.uIcons.value as Vector4[]).map((icon) => icon.clone());
    saved.scale = (screen.uFrameScale.value as Vector2).clone();
    saved.grid = (screen.uGrid.value as Vector2).clone();
    (screen.uGrid.value as Vector2).set(1e5, 1e5);
    view.main = hooks.frame;
    demo.picture = OWN;
    const active = document.activeElement;
    saved.opener = active instanceof HTMLElement && active !== document.body && !active.closest('.demo-layer') ? active : null;
    fade.mix = 0;
  }
  const now = performance.now();
  fade.at = now;
  fade.target = 0;
  fade.span = view.frame ? BLEND : SWITCH;
  live.crisp = true;
  demo.since = demo.closing ? now - GLIDE * (1 - Math.min((now - demo.closing) / GLIDE, 1)) : now;
  demo.closing = 0;
  demo.open = true;
  live.fadeUntil = Math.max(live.fadeUntil, now + GLIDE + 150);
  live.channel = -1;
  live.lastScroll = -1e9;
  hooks.view = glide;
  hooks.after = after;
  hooks.fit = fit;
  document.documentElement.classList.add('demo-open');
  window.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('keydown', onKey);
  slotOf(demo.picture);
  screen.uSharpIcons.value = 1;
  void sharpenAtlas(screen);
  window.dispatchEvent(new Event('scooch:demo'));
  emit();
}

export function closeDemo() {
  close();
}

function project(u: number, v: number, camera: PerspectiveCamera, frame: NonNullable<ReturnType<typeof screenFrame>>, width: number, height: number) {
  const mesh = hooks.laptop;
  if (!mesh) return [0, 0];
  work.point.set(frame.origin.x + u * frame.extent.x, frame.origin.y + (1 - v) * frame.extent.y, frame.front).applyMatrix4(mesh.matrixWorld).project(camera);
  return [(work.point.x * 0.5 + 0.5) * width, (0.5 - work.point.y * 0.5) * height];
}

export function handles(camera: PerspectiveCamera, width: number, height: number) {
  const frame = screenFrame();
  const slot = current();
  if (!frame || !slot) return [];
  return slot.icons.map((rect) => {
    const [x0, y0] = project(rect.x, rect.y, camera, frame, width, height);
    const [x1, y1] = project(rect.x + rect.z, rect.y + rect.w, camera, frame, width, height);
    return { x: (x0 + x1) / 2, y: (y0 + y1) / 2, w: Math.abs(x1 - x0), h: Math.abs(y1 - y0) };
  });
}

export function screenBox(camera: PerspectiveCamera, width: number, height: number) {
  const frame = screenFrame();
  if (!frame) return null;
  const [x0, y0] = project(0, 0, camera, frame, width, height);
  const [x1, y1] = project(1, 1, camera, frame, width, height);
  return { left: Math.min(x0, x1), right: Math.max(x0, x1), top: Math.min(y0, y1), bottom: Math.max(y0, y1) };
}

function uvAt(clientX: number, clientY: number, camera: PerspectiveCamera, rect: DOMRect) {
  const mesh = hooks.laptop;
  const uniforms = lcd();
  if (!mesh || !uniforms || !screenFrame()) return null;
  work.ray.set(((clientX - rect.left) / rect.width) * 2 - 1, -((clientY - rect.top) / rect.height) * 2 + 1, 0.5).unproject(camera).sub(camera.position);
  const facing = work.ray.dot(work.normal);
  if (Math.abs(facing) < 1e-8) return null;
  const t = work.point.copy(work.center).sub(camera.position).dot(work.normal) / facing;
  const local = mesh.worldToLocal(work.point.copy(camera.position).addScaledVector(work.ray, t));
  const origin = uniforms.uOrigin.value as Vector2;
  const extent = uniforms.uExtent.value as Vector2;
  return [(local.x - origin.x) / extent.x, 1 - (local.y - origin.y) / extent.y];
}

function active() {
  const slot = current();
  return slot?.frame && slot.frame === view.frame ? slot : null;
}

export function pick(index: number, clientX: number, clientY: number, camera: PerspectiveCamera, rect: DOMRect) {
  const slot = active();
  const icon = slot?.icons[index];
  if (!slot?.frame || !icon) return false;
  const at = uvAt(clientX, clientY, camera, rect);
  drag.index = index;
  drag.dx = at ? at[0] - icon.x : icon.z / 2;
  drag.dy = at ? at[1] - icon.y : icon.w / 2;
  drag.x = clientX;
  drag.y = clientY;
  drag.moved = false;
  drag.slid = false;
  live.pressing = true;
  begin(slot.frame, 0);
  void ask(rects(slot, index, icon.x, icon.y), index, false);
  void shot('pick', { gain: 0.3 });
  return true;
}

export function move(clientX: number, clientY: number, camera: PerspectiveCamera, rect: DOMRect) {
  const slot = active();
  if (drag.index < 0 || !slot) return;
  if (!drag.moved && Math.hypot(clientX - drag.x, clientY - drag.y) <= 6) return;
  if (!drag.moved) {
    drag.moved = true;
    live.dragging = true;
    dragged();
    if (!demo.used) {
      demo.used = true;
      emit();
    }
  }
  const at = uvAt(clientX, clientY, camera, rect);
  const icon = slot.icons[drag.index];
  if (!at || !icon) return;
  icon.x = Math.min(Math.max(at[0] - drag.dx, 0), 1 - icon.z);
  icon.y = Math.min(Math.max(at[1] - drag.dy, 0), 1 - icon.w);
  void ask(rects(slot, drag.index, icon.x, icon.y), drag.index, false);
}

export function drop() {
  const slot = active();
  if (drag.index < 0) return;
  const icon = slot?.icons[drag.index];
  if (slot && icon) void ask(rects(slot, drag.index, icon.x, icon.y), drag.index, true);
  if (drag.moved) void shot('drop', { gain: 0.35 });
  drag.index = -1;
  live.pressing = false;
  live.dragging = false;
}

export function nudge(index: number, dx: number, dy: number) {
  const slot = active();
  const icon = slot?.icons[index];
  if (!slot?.frame || !icon) return;
  if (!held.active || held.frame !== slot.frame || work.nudged !== index) {
    work.nudged = index;
    begin(slot.frame, 0);
    void shot('pick', { gain: 0.3 });
  }
  dragged();
  drag.moved = true;
  if (!demo.used) {
    demo.used = true;
    emit();
  }
  icon.x = Math.min(Math.max(icon.x + dx * STEP, 0), 1 - icon.z);
  icon.y = Math.min(Math.max(icon.y + dy * STEP * (4 / 3), 0), 1 - icon.w);
  void ask(rects(slot, index, icon.x, icon.y), index, false);
  window.clearTimeout(work.timer);
  work.timer = window.setTimeout(() => {
    void ask(rects(slot, index, icon.x, icon.y), index, true);
    void shot('drop', { gain: 0.35 });
  }, 300);
}
