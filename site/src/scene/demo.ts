import type { Matrix4, PerspectiveCamera, Quaternion, Texture, Vector2, Vector3, Vector4 } from 'three';
import { shot } from '@/lib/audio';
import { story } from '@/story/state';
import { ask, begin, dragged, held } from './drag';
import { warm } from './engine';
import { quality } from './quality';
import { hooks, live } from './touch';

const GLIDE = 600;
const BLEND = 420;
const BLEND_REDUCED = 200;
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
};

const saved = { frame: null as Texture | null, icons: [] as Vector4[], scale: null as Vector2 | null, grid: null as Vector2 | null };
const fade = { mix: 0, at: 0 };
const work = {} as { target: Vector3; center: Vector3; normal: Vector3; up: Vector3; point: Vector3; ray: Vector3; quaternion: Quaternion; matrix: Matrix4; answers: number; timer: number };

function ready() {
  if (work.target || !hooks.laptop) return;
  const mesh = hooks.laptop;
  const vector = () => mesh.position.clone().set(0, 0, 0);
  Object.assign(work, { target: vector(), center: vector(), normal: vector(), up: vector(), point: vector(), ray: vector(), quaternion: mesh.quaternion.clone(), matrix: mesh.matrix.clone(), answers: 0, timer: 0 });
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

function view(camera: PerspectiveCamera) {
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

function after() {
  const screen = lcd();
  const tube = crt();
  const frame = hooks.frame;
  if (!screen || !tube || !frame || demo.amount <= 0) return;
  screen.uFrame.value = frame.texture;
  const icons = screen.uIcons.value as Vector4[];
  const source = tube.uIcons.value as Vector4[];
  for (let i = 0; i < icons.length && i < source.length; i++) icons[i].copy(source[i]);
  (screen.uFrameScale.value as Vector2).copy(tube.uFrameScale.value as Vector2);
  const now = performance.now();
  const step = (now - fade.at) / (reduced() ? BLEND_REDUCED : BLEND);
  fade.at = now;
  fade.mix = demo.open ? Math.max(fade.mix - step, 0) : Math.min(fade.mix + step, 1);
  screen.uFromMix.value = fade.mix * fade.mix * (3 - 2 * fade.mix);
  if (demo.camera) quality.focus(work.center, demo.camera.position, 1, work.center, work.point, story.dof * (1 - demo.amount));
  if (held.answers !== work.answers) {
    work.answers = held.answers;
    if (drag.moved && !drag.slid) {
      drag.slid = true;
      void shot('slide', { gain: 0.22 });
    }
  }
}

function rects(index: number, x: number, y: number) {
  const icons = hooks.icons ?? [];
  const out = new Float32Array(icons.length * 4);
  icons.forEach((rect, i) => {
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
  }
  fade.mix = 0;
  live.crisp = false;
  hooks.view = () => {};
  hooks.after = () => {};
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
    screen.uFrom.value = saved.frame;
    (screen.uFromScale.value as Vector2).copy(saved.scale);
    const from = screen.uFromIcons.value as Vector4[];
    saved.icons.forEach((icon, i) => from[i]?.copy(icon));
    (screen.uGrid.value as Vector2).set(1e5, 1e5);
    fade.mix = 1;
  }
  const now = performance.now();
  fade.at = now;
  live.crisp = true;
  demo.since = demo.closing ? now - GLIDE * (1 - Math.min((now - demo.closing) / GLIDE, 1)) : now;
  demo.closing = 0;
  demo.open = true;
  live.fadeUntil = Math.max(live.fadeUntil, now + GLIDE + 150);
  live.channel = -1;
  live.lastScroll = -1e9;
  hooks.view = view;
  hooks.after = after;
  document.documentElement.classList.add('demo-open');
  window.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('keydown', onKey);
  void warm(hooks.frame.scenery.info, rects(-1, 0, 0)).catch(() => undefined);
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
  const icons = hooks.icons ?? [];
  if (!frame) return [];
  return icons.map((rect) => {
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

export function pick(index: number, clientX: number, clientY: number, camera: PerspectiveCamera, rect: DOMRect) {
  const frame = hooks.frame;
  const icon = hooks.icons?.[index];
  if (!frame || !icon) return false;
  const at = uvAt(clientX, clientY, camera, rect);
  drag.index = index;
  drag.dx = at ? at[0] - icon.x : icon.z / 2;
  drag.dy = at ? at[1] - icon.y : icon.w / 2;
  drag.x = clientX;
  drag.y = clientY;
  drag.moved = false;
  drag.slid = false;
  live.icon.index = index;
  live.icon.x = icon.x;
  live.icon.y = icon.y;
  live.pressing = true;
  begin(frame, hooks.place());
  void ask(rects(index, icon.x, icon.y), index, false);
  void shot('pick', { gain: 0.3 });
  return true;
}

export function move(clientX: number, clientY: number, camera: PerspectiveCamera, rect: DOMRect) {
  if (drag.index < 0) return;
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
  const icon = hooks.icons?.[drag.index];
  if (!at || !icon) return;
  live.icon.x = Math.min(Math.max(at[0] - drag.dx, 0), 1 - icon.z);
  live.icon.y = Math.min(Math.max(at[1] - drag.dy, 0), 1 - icon.w);
  void ask(rects(drag.index, live.icon.x, live.icon.y), drag.index, false);
}

export function drop() {
  if (drag.index < 0) return;
  void ask(rects(drag.index, live.icon.x, live.icon.y), drag.index, true);
  if (drag.moved) void shot('drop', { gain: 0.35 });
  drag.index = -1;
  live.pressing = false;
  live.dragging = false;
}

export function nudge(index: number, dx: number, dy: number) {
  const frame = hooks.frame;
  const icon = hooks.icons?.[index];
  if (!frame || !icon) return;
  if (!held.active || live.icon.index !== index) {
    live.icon.index = index;
    live.icon.x = icon.x;
    live.icon.y = icon.y;
    begin(frame, hooks.place());
    void shot('pick', { gain: 0.3 });
  }
  dragged();
  drag.moved = true;
  if (!demo.used) {
    demo.used = true;
    emit();
  }
  live.icon.x = Math.min(Math.max(live.icon.x + dx * STEP, 0), 1 - icon.z);
  live.icon.y = Math.min(Math.max(live.icon.y + dy * STEP * (4 / 3), 0), 1 - icon.w);
  void ask(rects(index, live.icon.x, live.icon.y), index, false);
  window.clearTimeout(work.timer);
  work.timer = window.setTimeout(() => {
    void ask(rects(index, live.icon.x, live.icon.y), index, true);
    void shot('drop', { gain: 0.35 });
  }, 300);
}
