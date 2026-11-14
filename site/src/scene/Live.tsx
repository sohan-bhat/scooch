import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { Box3, Mesh, Object3D, Plane, Raycaster, Vector2, Vector3 } from 'three';
import { runtime } from '@/story/state';
import { DESK_TOP } from './layout';
import { ask, begin, end } from './drag';
import { hooks, live, liveId, release, rimOf, things } from './touch';
import { hush } from './sound';

const DESK = { left: -0.98, right: 0.98, back: -0.36, front: 0.42 };
const QUIET = 250;

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
      live.channel = (Math.max(live.channel, 0) + 1) % 4;
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
      live.coffee = live.coffee <= 0 ? 4 : live.coffee - 1;
    },
  };
}

export function Live({ onAction }: { onAction: (name: string) => void }) {
  const { camera, gl } = useThree();
  const tools = useMemo(() => ({ ray: new Raycaster(), ndc: new Vector2(), plane: new Plane(new Vector3(0, 1, 0), -DESK_TOP), hit: new Vector3(), box: new Box3(), size: new Vector3() }), []);

  useEffect(() => {
    const element = gl.domElement;
    const press = { id: '', x: 0, y: 0, moved: false, region: '', startX: 0, startZ: 0, aim: 0, mesh: null as Object3D | null, dx: 0, dy: 0 };
    const onScroll = () => {
      live.lastScroll = performance.now();
    };
    const pick = (event: PointerEvent) => {
      const rect = element.getBoundingClientRect();
      tools.ndc.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
      tools.ray.setFromCamera(tools.ndc, camera);
      const roots = Object.values(things).filter((thing): thing is Object3D => !!thing && visible(thing));
      const hits = tools.ray.intersectObjects(roots, true).filter((hit) => visible(hit.object));
      const first = hits[0];
      if (!first) return { id: '', point: null as Vector3 | null, object: null as Object3D | null };
      return { id: liveId(first.object), point: first.point.clone(), object: first.object };
    };
    const deskPoint = (event: PointerEvent) => {
      const rect = element.getBoundingClientRect();
      tools.ndc.set(((event.clientX - rect.left) / rect.width) * 2 - 1, -((event.clientY - rect.top) / rect.height) * 2 + 1);
      tools.ray.setFromCamera(tools.ndc, camera);
      return tools.ray.ray.intersectPlane(tools.plane, tools.hit);
    };
    const onMove = (event: PointerEvent) => {
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
        if (press.id === 'cup') {
          const point = deskPoint(event);
          if (point) {
            live.cup.x = Math.min(Math.max(point.x, DESK.left), DESK.right);
            live.cup.z = Math.min(Math.max(point.z, DESK.back), DESK.front);
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
      if (found.id === 'plant' && found.object) {
        live.sway.vx += event.movementX * 0.0009;
        live.sway.vz += event.movementY * 0.0009;
      }
      live.hover = found.id;
      element.style.cursor = found.id ? 'pointer' : '';
    };
    const onDown = (event: PointerEvent) => {
      if (!live.quiet || !runtime.ready || event.button > 0) return;
      const found = pick(event);
      press.id = found.id || (event.pointerType === 'mouse' ? 'space' : '');
      press.x = event.clientX;
      press.y = event.clientY;
      press.moved = false;
      press.aim = live.aimTarget;
      press.region = '';
      if (found.id === 'tv' && found.point && found.object) {
        const screen = (found.object as Mesh).material && ((found.object as Mesh).material as { name?: string }).name === 'scooch-crt';
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
          begin(hooks.frame, hooks.place());
          void ask(rects(index, rect.x, rect.y), index, false);
          element.setPointerCapture(event.pointerId);
          return;
        }
      }
      if (press.id === 'cup' || press.id === 'lamp' || press.id === 'space') element.setPointerCapture(event.pointerId);
    };
    const onUp = (event: PointerEvent) => {
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
      if (id === 'lamp') act.lamp();
      else if (id === 'tv') press.region === 'power' ? act.power() : act.channel();
      else if (id === 'cassette') act.tape();
      else if (id === 'clock') act.ring();
      else if (id === 'stapler') act.staple();
      else if (id === 'cup') act.sip();
      else return;
      onAction(id === 'tv' ? press.region : id);
    };
    const onLeave = () => {
      live.hover = '';
      element.style.cursor = '';
    };
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('wheel', onScroll, { passive: true });
    element.addEventListener('pointermove', onMove);
    element.addEventListener('pointerdown', onDown);
    element.addEventListener('pointerup', onUp);
    element.addEventListener('pointercancel', onUp);
    element.addEventListener('pointerleave', onLeave);
    return () => {
      window.removeEventListener('scroll', onScroll);
      window.removeEventListener('wheel', onScroll);
      element.removeEventListener('pointermove', onMove);
      element.removeEventListener('pointerdown', onDown);
      element.removeEventListener('pointerup', onUp);
      element.removeEventListener('pointercancel', onUp);
      element.removeEventListener('pointerleave', onLeave);
    };
  }, [camera, gl, tools, onAction]);

  useFrame((_, delta) => {
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
      for (const level of rimOf(root)) level.value += (target - level.value) * (1 - Math.exp(-step * 10));
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
    const top = stapler?.getObjectByName('vintage_stapler_top');
    if (top) {
      const base = top.userData.base ?? (top.userData.base = top.rotation.clone());
      const t = (now - live.stapleAt) / 1000;
      const press = t < 0 || t > 0.5 ? 0 : t < 0.09 ? t / 0.09 : Math.exp(-(t - 0.09) * 11) * Math.cos((t - 0.09) * 30);
      top.rotation.set(base.x, base.y, base.z - press * 0.09);
    }

    const plant = things.plant?.getObjectByName('potted_plant_04_plant');
    if (plant) {
      const base = plant.userData.base ?? (plant.userData.base = plant.rotation.clone());
      plant.rotation.set(base.x + live.sway.z, base.y, base.z - live.sway.x);
    }

    const cup = things.cup;
    if (cup) {
      if (!live.cup.ready) {
        live.cup.homeX = live.cup.x = cup.position.x;
        live.cup.homeZ = live.cup.z = cup.position.z;
        cup.userData.y = cup.position.y;
        live.cup.ready = true;
      }
      cup.position.x += (live.cup.x - cup.position.x) * (1 - Math.exp(-step * 12));
      cup.position.z += (live.cup.z - cup.position.z) * (1 - Math.exp(-step * 12));
      const t = (now - live.sipAt) / 1000;
      const lift = t < 0 || t > 1.5 ? 0 : Math.sin(Math.min(t / 1.5, 1) * Math.PI);
      cup.position.y = cup.userData.y + lift * 0.11;
      cup.rotation.x = -Math.max(0, Math.sin(Math.min(t / 1.5, 1) * Math.PI) - 0.4) * (t < 1.5 && t > 0 ? 0.9 : 0);
      const coffee = cup.getObjectByName('coffee');
      if (coffee) {
        const level = live.coffee / 4;
        coffee.visible = live.coffee > 0;
        coffee.position.y = coffee.userData.low + (coffee.userData.high - coffee.userData.low) * level;
        coffee.scale.setScalar(0.72 + 0.28 * level);
      }
    }
  });

  return null;
}
