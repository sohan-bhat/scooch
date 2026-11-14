import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { Box3, Mesh, Object3D, Plane, Raycaster, Vector2, Vector3 } from 'three';
import content from '@/content.json';
import { runtime } from '@/story/state';
import { DESK_TOP } from './layout';
import { ask, begin, end } from './drag';
import { warm } from './engine';
import { CHANNELS, hooks, live, liveId, obstacles, release, rimOf, things } from './touch';
import { hush } from './sound';
import { quiet } from '@/lib/idle';
import { acceleratedRaycast, computeBoundsTree } from 'three-mesh-bvh';

const DESK = { left: -0.98, right: 0.98, back: -0.36, front: 0.42 };
const QUIET = 250;
const HINT = 'scooch:hint';

const hint = { element: null as HTMLElement | null, done: false, shownAt: 0, readyAt: 0, point: new Vector3() };

function hinted() {
  try {
    return window.sessionStorage.getItem(HINT) === '1';
  } catch {
    return false;
  }
}

function markHinted() {
  try {
    window.sessionStorage.setItem(HINT, '1');
    return true;
  } catch {
    return false;
  }
}

function hideHint() {
  if (hint.shownAt && hint.element) hint.element.removeAttribute('data-show');
  hint.shownAt = 0;
}

function barrel(x: number, y: number) {
  const cx = x * 2 - 1;
  const cy = y * 2 - 1;
  const d = cx * cx + cy * cy;
  return [(cx * (1 + 0.035 * d)) * 0.5 + 0.5, (cy * (1 + 0.05 * d)) * 0.5 + 0.5];
}

function screenPoint(mesh: Object3D, point: Vector3) {
  if (!hooks.origin || !hooks.extent) return null;
  const local = mesh.worldToLocal(point.clone());
  const u = (local.x - hooks.origin.x) / hooks.extent.x;
  const v = 1 - (local.y - hooks.origin.y) / hooks.extent.y;
  return barrel((u - 0.5) / 0.94 + 0.5, (v - 0.5) / 0.93 + 0.5);
}

const bounded = new WeakSet<Object3D>();

function bounds(root: Object3D) {
  if (bounded.has(root)) return;
  bounded.add(root);
  const meshes: Mesh[] = [];
  root.traverse((child) => {
    if ((child as Mesh).isMesh) meshes.push(child as Mesh);
  });
  void (async () => {
    for (const mesh of meshes) {
      await quiet();
      if (!mesh.geometry.boundsTree) computeBoundsTree.call(mesh.geometry);
      mesh.raycast = acceleratedRaycast;
    }
  })();
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

const blocks: { minX: number; maxX: number; minZ: number; maxZ: number }[] = [];

function footprints(held: Object3D) {
  if (blocks.length) return;
  const box = new Box3();
  const size = new Box3().setFromObject(held).getSize(new Vector3());
  const pad = Math.max(size.x, size.z) * 0.5 + 0.008;
  for (const object of obstacles) {
    if (!visible(object)) continue;
    box.setFromObject(object);
    if (box.isEmpty() || box.max.y < DESK_TOP + 0.004) continue;
    blocks.push({ minX: box.min.x - pad, maxX: box.max.x + pad, minZ: box.min.z - pad, maxZ: box.max.z + pad });
  }
}

function settle(x: number, z: number, fromX: number, fromZ: number) {
  let px = x;
  let pz = z;
  for (let round = 0; round < 4; round++) {
    let moved = false;
    for (const block of blocks) {
      if (px <= block.minX || px >= block.maxX || pz <= block.minZ || pz >= block.maxZ) continue;
      const exits = [
        { d: px - block.minX, x: block.minX, z: pz },
        { d: block.maxX - px, x: block.maxX, z: pz },
        { d: pz - block.minZ, x: px, z: block.minZ },
        { d: block.maxZ - pz, x: px, z: block.maxZ },
      ].filter((exit) => exit.x >= DESK.left && exit.x <= DESK.right && exit.z >= DESK.back && exit.z <= DESK.front);
      if (!exits.length) return [fromX, fromZ];
      exits.sort((a, b) => a.d - b.d);
      px = exits[0].x;
      pz = exits[0].z;
      moved = true;
    }
    if (!moved) return [px, pz];
  }
  return [fromX, fromZ];
}

function isCrt(object: Object3D | null) {
  const material = (object as Mesh | null)?.material as { name?: string } | undefined;
  return material?.name === 'scooch-crt';
}

function visible(object: Object3D) {
  let at: Object3D | null = object;
  while (at) {
    if (!at.visible) return false;
    at = at.parent;
  }
  return true;
}

export function actions(now = performance.now()) {
  return {
    lamp() {
      live.lampTarget = live.lampTarget > 0.5 ? 0 : 1;
    },
    channel() {
      if (!live.power) return;
      const busy = hooks.showing();
      let next = live.channel >= 0 ? live.channel : CHANNELS.indexOf(hooks.current());
      for (let step = 0; step < CHANNELS.length; step++) {
        next = (next + 1) % CHANNELS.length;
        if (!busy.includes(CHANNELS[next])) break;
      }
      live.channel = next;
      live.noiseAt = now;
    },
    power() {
      live.power = !live.power;
      if (live.power) live.onAt = now;
      else live.offAt = now;
    },
    tape() {
      live.tape = !live.tape;
      live.tapeAt = now;
    },
    ring() {
      live.ringUntil = live.ringUntil > now ? 0 : now + 4000;
    },
    staple() {
      live.stapleAt = now;
    },
    sip() {
      if (now - live.sipAt < 1600) return;
      live.sipAt = now;
      live.juice = live.juice <= 0 ? 4 : live.juice - 1;
    },
  };
}

export function Live({ onAction }: { onAction: (name: string) => void }) {
  const { camera, gl } = useThree();
  const tools = useMemo(() => ({ ray: new Raycaster(), ndc: new Vector2(), plane: new Plane(new Vector3(0, 1, 0), -DESK_TOP), hit: new Vector3(), box: new Box3(), size: new Vector3() }), []);

  useEffect(() => {
    const element = gl.domElement;
    const press = { id: '', x: 0, y: 0, moved: false, region: '', startX: 0, startZ: 0, aim: 0, mesh: null as Object3D | null, dx: 0, dy: 0, rest: 0 };
    const onScroll = () => {
      live.lastScroll = performance.now();
    };
    const pick = (event: PointerEvent) => {
      const rect = element.getBoundingClientRect();
      tools.ndc.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
      tools.ray.setFromCamera(tools.ndc, camera);
      const roots = Object.values(things).filter((thing): thing is Object3D => !!thing && visible(thing));
      for (const root of roots) bounds(root);
      const hits = tools.ray.intersectObjects(roots, true).filter((hit) => visible(hit.object));
      const first = hits[0];
      if (!first) return { id: '', point: null as Vector3 | null, object: null as Object3D | null };
      const id = liveId(first.object);
      const glass = id === 'tv' ? hits.find((hit) => hit.distance < first.distance + 0.03 && isCrt(hit.object)) : undefined;
      const chosen = glass ?? first;
      return { id, point: chosen.point.clone(), object: chosen.object };
    };
    const deskPoint = (event: PointerEvent) => {
      const rect = element.getBoundingClientRect();
      tools.ndc.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
      tools.ray.setFromCamera(tools.ndc, camera);
      return tools.ray.ray.intersectPlane(tools.plane, tools.hit);
    };
    const onMove = (event: PointerEvent, movementX: number, movementY: number) => {
      if (!live.quiet || !runtime.ready) return;
      if (press.id) {
        const dx = event.clientX - press.x;
        const dy = event.clientY - press.y;
        if (Math.hypot(dx, dy) > 6) press.moved = true;
        if (!press.moved) return;
        if (press.id === 'icon' && press.mesh) {
          const rect = element.getBoundingClientRect();
          tools.ndc.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
          tools.ray.setFromCamera(tools.ndc, camera);
          const hit = tools.ray.intersectObject(press.mesh, false)[0];
          const at = hit ? screenPoint(press.mesh, hit.point) : null;
          const icon = hooks.icons?.[live.icon.index];
          if (at && icon) {
            live.icon.x = Math.min(Math.max(at[0] - press.dx, 0), 1 - icon.z);
            live.icon.y = Math.min(Math.max(at[1] - press.dy, 0), 1 - icon.w);
            void ask(rects(live.icon.index, live.icon.x, live.icon.y), live.icon.index, false);
          }
          return;
        }
        if (press.id === 'glass') {
          const point = deskPoint(event);
          if (point && things.glass) {
            footprints(things.glass);
            const [x, z] = settle(Math.min(Math.max(point.x, DESK.left), DESK.right), Math.min(Math.max(point.z, DESK.back), DESK.front), live.glass.x, live.glass.z);
            live.glass.x = x;
            live.glass.z = z;
          }
        } else if (press.id === 'lamp') {
          live.aimTarget = Math.min(Math.max(press.aim + dx * 0.0016, -0.3), 0.3);
        } else if (press.id === 'space') {
          live.look.tx = Math.min(Math.max(dx / 120, -3), 3);
          live.look.ty = Math.min(Math.max(dy / 140, -2), 2);
        }
        return;
      }
      const found = pick(event);
      window.clearTimeout(press.rest);
      const crt = found.id === 'tv' && isCrt(found.object);
      if (crt && live.channel < 0 && live.power && hooks.frame) {
        const frame = hooks.frame;
        press.rest = window.setTimeout(() => {
          if (live.quiet && live.hover === 'tv') void warm(frame.scenery.info, rects(-1, 0, 0)).catch(() => undefined);
        }, 160);
      }
      if (found.id === 'plant' && found.object) {
        live.sway.vx += movementX * 0.0009;
        live.sway.vz += movementY * 0.0009;
      }
      live.hover = found.id;
      element.style.cursor = found.id ? 'pointer' : '';
    };
    const onDown = (event: PointerEvent) => {
      if (!live.quiet || !runtime.ready || event.button > 0) return;
      const found = pick(event);
      press.id = found.id || (event.pointerType === 'mouse' ? 'space' : '');
      live.pressing = !!press.id;
      press.x = event.clientX;
      press.y = event.clientY;
      press.moved = false;
      press.aim = live.aimTarget;
      press.region = '';
      if (found.id === 'tv' && found.point && found.object) {
        const screen = isCrt(found.object);
        press.region = screen ? 'channel' : 'power';
        const at = screen && live.channel < 0 && live.power ? screenPoint(found.object, found.point) : null;
        const index = at ? (hooks.icons ?? []).findIndex((rect) => at[0] >= rect.x && at[0] <= rect.x + rect.z && at[1] >= rect.y && at[1] <= rect.y + rect.w) : -1;
        if (at && index >= 0 && hooks.frame) {
          const rect = hooks.icons![index];
          press.id = 'icon';
          press.mesh = found.object;
          press.dx = at[0] - rect.x;
          press.dy = at[1] - rect.y;
          live.icon.index = index;
          live.icon.x = rect.x;
          live.icon.y = rect.y;
          void warm(hooks.frame.scenery.info, rects(-1, 0, 0)).catch(() => undefined);
          begin(hooks.frame, hooks.place());
          void ask(rects(index, rect.x, rect.y), index, false);
          element.setPointerCapture(event.pointerId);
          return;
        }
      }
      if (press.id === 'glass' || press.id === 'lamp' || press.id === 'space') element.setPointerCapture(event.pointerId);
    };
    const onUp = (event: PointerEvent) => {
      live.pressing = false;
      if (!press.id) return;
      const id = press.id;
      press.id = '';
      if (element.hasPointerCapture(event.pointerId)) element.releasePointerCapture(event.pointerId);
      live.look.tx = 0;
      live.look.ty = 0;
      if (id === 'icon') {
        void ask(rects(live.icon.index, live.icon.x, live.icon.y), live.icon.index, true);
        return;
      }
      if (press.moved) return;
      const act = actions();
      hideHint();
      if (id === 'lamp') act.lamp();
      else if (id === 'tv') press.region === 'power' ? act.power() : act.channel();
      else if (id === 'cassette') act.tape();
      else if (id === 'clock') act.ring();
      else if (id === 'stapler') act.staple();
      else if (id === 'glass') act.sip();
      else return;
      onAction(id === 'tv' ? press.region : id);
    };
    const onLeave = () => {
      live.hover = '';
      element.style.cursor = '';
    };
    const moved = { event: null as PointerEvent | null, frame: 0, x: 0, y: 0 };
    const onPointer = (event: PointerEvent) => {
      moved.event = event;
      moved.x += event.movementX;
      moved.y += event.movementY;
      if (moved.frame) return;
      moved.frame = requestAnimationFrame(() => {
        moved.frame = 0;
        const latest = moved.event;
        const x = moved.x;
        const y = moved.y;
        moved.event = null;
        moved.x = 0;
        moved.y = 0;
        if (latest) onMove(latest, x, y);
      });
    };
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('wheel', onScroll, { passive: true });
    element.addEventListener('pointermove', onPointer);
    element.addEventListener('pointerdown', onDown);
    element.addEventListener('pointerup', onUp);
    element.addEventListener('pointercancel', onUp);
    element.addEventListener('pointerleave', onLeave);
    return () => {
      window.clearTimeout(press.rest);
      window.removeEventListener('scroll', onScroll);
      window.removeEventListener('wheel', onScroll);
      cancelAnimationFrame(moved.frame);
      element.removeEventListener('pointermove', onPointer);
      element.removeEventListener('pointerdown', onDown);
      element.removeEventListener('pointerup', onUp);
      element.removeEventListener('pointercancel', onUp);
      element.removeEventListener('pointerleave', onLeave);
    };
  }, [camera, gl, tools, onAction]);

  useFrame(({ camera }, delta) => {
    const now = performance.now();
    const quiet = now - live.lastScroll > QUIET;
    if (live.quiet && !quiet) {
      release(now);
      hush();
      end();
      live.hover = '';
      gl.domElement.style.cursor = '';
    }
    live.quiet = quiet;
    if (!quiet) hideHint();
    if (runtime.ready && !hint.readyAt) {
      hint.readyAt = now;
      hint.done = hinted() || runtime.still >= 0;
      hint.element = document.querySelector<HTMLElement>('[data-overlay="hint"]');
    }
    if (hint.readyAt && !hint.done && quiet && now - Math.max(live.lastScroll, hint.readyAt) > 2000 && things.lamp && hint.element) {
      things.lamp.getWorldPosition(hint.point).project(camera);
      if (Math.abs(hint.point.x) < 0.85 && Math.abs(hint.point.y) < 0.85 && hint.point.z < 1) {
        hint.done = true;
        markHinted();
        hint.element.textContent = content.story.hint;
        hint.element.setAttribute('data-show', '');
        hint.shownAt = now;
      }
    }
    if (hint.shownAt && now - hint.shownAt > 7000) hideHint();
    const step = Math.min(delta, 0.05);
    const warm = live.lampTarget > live.lamp ? 2.2 : 7;
    live.lamp += (live.lampTarget - live.lamp) * (1 - Math.exp(-step * warm));
    live.aim += (live.aimTarget - live.aim) * (1 - Math.exp(-step * 6));
    live.look.x += (live.look.tx - live.look.x) * (1 - Math.exp(-step * 9));
    live.look.y += (live.look.ty - live.look.y) * (1 - Math.exp(-step * 9));
    live.sway.vx += -live.sway.x * 40 * step - live.sway.vx * 4 * step;
    live.sway.vz += -live.sway.z * 40 * step - live.sway.vz * 4 * step;
    live.sway.x += live.sway.vx * step;
    live.sway.z += live.sway.vz * step;

    for (const [id, root] of Object.entries(things)) {
      const target = live.hover === id && live.quiet ? 0.55 : 0;
      for (const level of rimOf(root)) {
        if (level.value === 0 && target === 0) continue;
        level.value += (target - level.value) * (1 - Math.exp(-step * 10));
        if (level.value < 0.001 && target === 0) level.value = 0;
      }
    }

    const lamp = things.lamp;
    if (lamp) lamp.rotation.y = live.aim;

    const cassette = things.cassette;
    if (cassette) {
      const base = cassette.userData.base ?? (cassette.userData.base = cassette.position.clone());
      const shake = live.tape ? 0.00035 : 0;
      cassette.position.set(base.x + Math.sin(now * 0.19) * shake, base.y + Math.abs(Math.sin(now * 0.31)) * shake, base.z);
    }

    const clock = things.clock;
    if (clock) {
      const base = clock.userData.base ?? (clock.userData.base = clock.rotation.clone());
      const ringing = live.ringUntil > now;
      clock.rotation.set(base.x, base.y, base.z + (ringing ? Math.sin(now * 0.11) * 0.035 : 0));
      if (!ringing && live.ringUntil) live.ringUntil = 0;
    }

    const stapler = things.stapler;
    const top = stapler ? (stapler.userData.top ??= stapler.getObjectByName('vintage_stapler_top')) as Object3D | undefined : undefined;
    if (top) {
      const base = top.userData.base ?? (top.userData.base = top.rotation.clone());
      const t = (now - live.stapleAt) / 1000;
      const press = t < 0 || t > 0.5 ? 0 : t < 0.09 ? t / 0.09 : Math.exp(-(t - 0.09) * 11) * Math.cos((t - 0.09) * 30);
      top.rotation.set(base.x, base.y, base.z - press * 0.09);
    }

    const plant = things.plant ? (things.plant.userData.leaves ??= things.plant.getObjectByName('potted_plant_04_plant')) as Object3D | undefined : undefined;
    if (plant) {
      const base = plant.userData.base ?? (plant.userData.base = plant.rotation.clone());
      plant.rotation.set(base.x + live.sway.z, base.y, base.z - live.sway.x);
    }

  });

  return null;
}
