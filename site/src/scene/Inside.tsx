import { useFrame } from '@react-three/fiber';
import { useMemo } from 'react';
import {
  Color,
  DoubleSide,
  InstancedBufferAttribute,
  InstancedMesh,
  Matrix4,
  Mesh,
  Object3D,
  PlaneGeometry,
  Quaternion,
  ShaderMaterial,
  Vector3,
  Vector4,
  type Texture,
} from 'three';
import { tokenColor } from '@/lib/tokens';
import type { DemoManifest } from '@/story/demo';
import { story } from '@/story/state';
import { ICON_SLOTS, pictureBody, pictureHead, pictureUniforms } from './screen/picture';
import type { Frame } from './screen/scenery';

const planeVertex = `
${pictureHead}
uniform sampler2D uSaliency;
uniform float uRelief;
uniform float uSweep;
uniform vec2 uSize;
varying vec2 vUv;
varying float vHeight;
varying vec3 vNormal;
${pictureBody}

float sweepFront(vec2 uv) {
  float diagonal = (uv.x * 1.333 + uv.y) / 2.333;
  float front = mix(-0.25, 1.25, uSweep);
  return smoothstep(0.0, 0.12, front - diagonal);
}

uniform float uScenery;

float scenery(vec2 uv) {
  return 0.0;
}

float relief(vec2 uv) {
  float s = textureLod(uSaliency, place(clamp(uv, 0.0, 1.0)), 0.5).r;
  return smoothstep(0.32, 0.85, s) * sweepFront(uv);
}

void main() {
  vec2 uv = vec2(position.x / uSize.x + 0.5, 0.5 - position.y / uSize.y);
  float edge = smoothstep(0.0, 0.02, uv.x) * smoothstep(0.0, 0.02, uv.y) * smoothstep(0.0, 0.02, 1.0 - uv.x) * smoothstep(0.0, 0.02, 1.0 - uv.y);
  float h = (relief(uv) + scenery(uv) * uScenery) * edge;
  float e = 1.0 / 128.0;
  float hx = relief(uv + vec2(e, 0.0)) - relief(uv - vec2(e, 0.0));
  float hy = relief(uv + vec2(0.0, e)) - relief(uv - vec2(0.0, e));
  float height = uRelief * uSize.y * 0.055;
  vec3 displaced = position + vec3(0.0, 0.0, h * height);
  vec3 n = normalize(vec3(-hx * height / (2.0 * e * uSize.x), hy * height / (2.0 * e * uSize.y), 1.0));
  vUv = uv;
  vHeight = h;
  vNormal = n;
  gl_Position = projectionMatrix * viewMatrix * modelMatrix * vec4(displaced, 1.0);
}
`;

const planeFragment = `
precision highp float;
${pictureHead}
uniform float uRelief;
uniform float uSweep;
uniform float uFade;
uniform vec2 uSize;
uniform vec3 uKey;
uniform vec3 uHeat;
uniform vec3 uGlow;
uniform vec3 uHot;
uniform vec4 uTiles[${ICON_SLOTS}];
uniform float uTileHeight[${ICON_SLOTS}];
uniform vec2 uShadowDir;
uniform sampler2D uFrameB;
uniform float uBlend;
varying vec2 vUv;
varying float vHeight;
varying vec3 vNormal;
${pictureBody}

float box(vec2 p, vec4 rect, float blur) {
  vec2 lo = rect.xy;
  vec2 hi = rect.xy + rect.zw;
  vec2 a = smoothstep(lo - blur, lo + blur, p);
  vec2 b = 1.0 - smoothstep(hi - blur, hi + blur, p);
  return a.x * a.y * b.x * b.y;
}

void main() {
  vec2 uv = vUv;
  vec3 photo = mix(picture(uv), texture(uFrameB, vec2(clamp(uv.x, 0.0, 1.0), 1.0 - clamp(uv.y, 0.0, 1.0))).rgb, uBlend);
  float grey = dot(photo, vec3(0.2126, 0.7152, 0.0722));

  float shadow = 0.0;
  for (int i = 0; i < ${ICON_SLOTS}; i++) {
    vec4 rect = uTiles[i];
    float lift = uTileHeight[i];
    vec4 fallen = vec4(rect.xy + uShadowDir * lift, rect.zw);
    vec4 core = vec4(fallen.xy + fallen.zw * vec2(0.14, 0.06), fallen.zw * vec2(0.72, 0.7));
    shadow = max(shadow, box(uv, core, 0.004 + lift * 0.35) * smoothstep(0.0, 0.004, lift));
  }

  vec3 n = normalize(vNormal);
  float light = 0.8 + 0.3 * max(dot(n, normalize(uKey)), 0.0);
  float steep = 1.0 - clamp(n.z, 0.0, 1.0);
  float level = clamp(vHeight, 0.0, 1.0);
  float lines = abs(fract(level * 7.0) - 0.5);
  float width = fwidth(level * 7.0) * 1.3;
  float contour = (1.0 - smoothstep(0.0, width + 0.001, lines - 0.45)) * smoothstep(0.05, 0.25, level);
  float base = smoothstep(0.02, 0.3, level) * (1.0 - smoothstep(0.3, 0.85, level));

  vec3 color = photo * light;
  color *= mix(1.0, 0.58, uRelief * (1.0 - smoothstep(0.04, 0.3, level)));
  color *= 1.0 - base * 0.35 * uRelief;
  float wall = smoothstep(0.12, 0.55, steep) * smoothstep(0.02, 0.12, level);
  vec3 ember = mix(uHeat * 0.75, uGlow, smoothstep(0.2, 0.9, level));
  color = mix(color, ember * (0.65 + 0.35 * light), wall * uRelief);
  color += uHeat * base * 0.12 * uRelief;
  color += mix(uHeat, uGlow, 0.65) * contour * 0.3 * uRelief;
  if (uSweep > 0.001 && uSweep < 0.999) {
    float diagonal = (uv.x * 1.333 + uv.y) / 2.333;
    float front = mix(-0.25, 1.25, uSweep) - diagonal;
    float wobble = (fbm(uv * vec2(6.0, 4.5) + uTime * 0.05) - 0.5) * 0.05;
    color += mix(uHeat, uGlow, 0.7) * exp(-abs(front + wobble) * 70.0) * 0.55;
  }
  color *= 1.0 - shadow * 0.6;
  vec2 c = uv - 0.5;
  color *= 1.0 - 0.3 * pow(dot(c, c) * 2.2, 1.6);
  gl_FragColor = vec4(color * uFade, 1.0);
}
`;

const tileVertex = `
attribute float aCell;
attribute float aGhost;
varying vec2 vUv;
varying float vCell;
varying float vGhost;
void main() {
  vUv = vec2(uv.x, 1.0 - uv.y);
  vCell = aCell;
  vGhost = aGhost;
  gl_Position = projectionMatrix * viewMatrix * modelMatrix * instanceMatrix * vec4(position, 1.0);
}
`;

const tileFragment = `
precision highp float;
uniform sampler2D uAtlas;
uniform float uOutline;
uniform float uFade;
varying vec2 vUv;
varying float vCell;
varying float vGhost;
#define GRID 4.0
vec4 cell(float index, float row) {
  vec2 at = (vec2(mod(index, GRID), row + floor(index / GRID)) + clamp(vUv, 0.002, 0.998)) / GRID;
  return texture(uAtlas, at);
}
void main() {
  vec4 icon = mix(cell(vCell, 0.0), cell(vCell, 2.0), uOutline);
  if (icon.a < 0.35) discard;
  gl_FragColor = vec4(icon.rgb * uFade * (1.0 - vGhost * 0.25), 1.0);
}
`;

export type Pose = { position: Vector3; quaternion: Quaternion; scale: number };

export type Stage = {
  glass: Pose;
  space: Pose;
  width: number;
  height: number;
};

export function createInside(frame: Frame, saliency: Texture, atlas: Texture, stage: Stage) {
  const geometry = new PlaneGeometry(stage.width, stage.height, 192, 144);
  const uniforms = {
    ...pictureUniforms(),
    uSaliency: { value: saliency },
    uRelief: { value: 0 },
    uScenery: { value: 0 },
    uSweep: { value: 0 },
    uFade: { value: 1 },
    uSize: { value: [stage.width, stage.height] },
    uKey: { value: new Vector3(-0.4, 0.5, 1) },
    uHeat: { value: tokenColor('lamp') },
    uGlow: { value: tokenColor('pad') },
    uHot: { value: tokenColor('paper') },
    uTiles: { value: Array.from({ length: ICON_SLOTS }, () => new Vector4(-1, -1, 0, 0)) },
    uTileHeight: { value: new Array(ICON_SLOTS).fill(0) },
    uShadowDir: { value: [0.18, 0.26] },
    uFrameB: { value: null as Texture | null },
    uBlend: { value: 0 },
  };
  uniforms.uFrame.value = frame.texture;
  uniforms.uFrameB.value = frame.texture;
  uniforms.uAtlas.value = atlas;
  const plane = new Mesh(
    geometry,
    new ShaderMaterial({ name: 'scooch-inside', vertexShader: planeVertex, fragmentShader: planeFragment, uniforms }),
  );
  plane.frustumCulled = false;

  const tileUniforms = { uAtlas: { value: atlas }, uOutline: { value: 0 }, uFade: { value: 1 } };
  const tiles = new InstancedMesh(
    new PlaneGeometry(1, 1),
    new ShaderMaterial({ name: 'scooch-tiles', vertexShader: tileVertex, fragmentShader: tileFragment, uniforms: tileUniforms, side: DoubleSide }),
    ICON_SLOTS,
  );
  tiles.geometry.setAttribute('aCell', new InstancedBufferAttribute(Float32Array.from({ length: ICON_SLOTS }, (_, index) => index), 1));
  tiles.geometry.setAttribute('aGhost', new InstancedBufferAttribute(new Float32Array(ICON_SLOTS), 1));
  tiles.frustumCulled = false;

  const group = new Object3D();
  group.add(plane, tiles);
  group.visible = false;
  return { group, plane, tiles, uniforms, tileUniforms };
}

const scratch = { matrix: new Matrix4(), position: new Vector3(), quaternion: new Quaternion(), scale: new Vector3(), color: new Color(), normal: new Vector3(), up: new Vector3(0, 1, 0) };

export type Pad = { center: Vector3; quaternion: Quaternion; screen: { width: number; height: number } };
type Pads = { laptop: Pad; console: Pad | null } | null;
type Place = { position: Vector3; quaternion: Quaternion; width: number; height: number };

const ease = (t: number) => {
  const x = Math.min(Math.max(t, 0), 1);
  return x * x * (3 - 2 * x);
};

const ASPECT = { wide: 2560 / 1080, tall: 1080 / 1920 };

function padPlace(pad: Pad, out: Place) {
  scratch.normal.set(0, 0, 1).applyQuaternion(pad.quaternion);
  out.position.copy(pad.center).addScaledVector(scratch.normal, 0.008);
  out.quaternion.copy(pad.quaternion);
  out.width = pad.screen.width;
  out.height = pad.screen.height;
  return out;
}

function blendPlace(a: Place, b: Place, t: number, arc: number, out: Place) {
  out.position.lerpVectors(a.position, b.position, t).addScaledVector(scratch.up, Math.sin(Math.PI * t) * arc);
  out.quaternion.slerpQuaternions(a.quaternion, b.quaternion, t);
  out.width = a.width + (b.width - a.width) * t;
  out.height = a.height + (b.height - a.height) * t;
  return out;
}

export function useInside(
  manifest: DemoManifest,
  frame: Frame,
  saliency: Texture,
  atlas: Texture,
  stage: Stage | null,
  gallery: { current: Record<string, Frame> },
  pads: Pads,
) {
  const inside = useMemo(() => (stage ? createInside(frame, saliency, atlas, stage) : null), [frame, saliency, atlas, stage]);
  const places = useMemo(
    () => ({
      space: { position: new Vector3(), quaternion: new Quaternion(), width: 0, height: 0 } as Place,
      from: { position: new Vector3(), quaternion: new Quaternion(), width: 0, height: 0 } as Place,
      to: { position: new Vector3(), quaternion: new Quaternion(), width: 0, height: 0 } as Place,
      now: { position: new Vector3(), quaternion: new Quaternion(), width: 0, height: 0 } as Place,
    }),
    [],
  );
  const layouts = useMemo(() => {
    const ids = manifest.icons.map((icon) => icon.id);
    const of = (id: string) => {
      const picture = manifest.pictures.find((candidate) => candidate.id === id);
      return picture ? ids.map((icon) => picture.icons[icon]) : null;
    };
    return { ids, wide: of('wide'), tall: of('tall'), swans: of('swans'), moon: of('moon') };
  }, [manifest]);

  useFrame((state) => {
    if (!inside || !stage) return;
    const lift = story.lift;
    const fly = story.fly;
    inside.group.visible = lift > 0.0005 && fly < 1.999;
    if (!inside.group.visible) return;
    const t = lift * lift * (3 - 2 * lift);
    const space = places.space;
    space.position.lerpVectors(stage.glass.position, stage.space.position, t);
    space.quaternion.slerpQuaternions(stage.glass.quaternion, stage.space.quaternion, t);
    const scale = stage.glass.scale + (stage.space.scale - stage.glass.scale) * t;
    space.width = stage.width * scale;
    space.height = stage.height * scale;

    const main = manifest.frames.before.icons;
    const { drag } = manifest;
    const own = (index: number) => {
      const id = layouts.ids[index];
      const rect = main[id];
      if (id !== drag.icon) return rect;
      return [drag.from[0] + (drag.to[0] - drag.from[0]) * story.drag, drag.from[1] + (drag.to[1] - drag.from[1]) * story.drag, rect[2], rect[3]];
    };
    const frames = gallery.current;
    const sized = (id: string, out: Place) => {
      out.position.copy(space.position);
      out.quaternion.copy(space.quaternion);
      if (id === 'wide') {
        out.width = space.width * 1.3;
        out.height = out.width / ASPECT.wide;
      } else if (id === 'tall') {
        out.height = space.height * 1.24;
        out.width = out.height * ASPECT.tall;
      } else {
        out.width = space.width;
        out.height = space.height;
      }
      return out;
    };

    let first = 'main';
    let second = 'main';
    let k = 0;
    let place = places.now;
    let raise = 1;
    if (fly > 0.0005 && pads) {
      const leg = fly < 1 ? 0 : 1;
      k = ease(fly - leg);
      first = leg === 0 ? 'main' : 'swans';
      second = leg === 0 ? 'swans' : 'moon';
      const from = leg === 0 ? sized('main', places.from) : padPlace(pads.laptop, places.from);
      const to = leg === 0 ? padPlace(pads.laptop, places.to) : pads.console ? padPlace(pads.console, places.to) : padPlace(pads.laptop, places.to);
      place = blendPlace(from, to, k, leg === 0 ? 0.16 : 0.24, places.now);
      raise = leg === 0 ? 1 - k : 0;
    } else {
      const shape = story.shape;
      const order = ['main', 'wide', 'tall', 'main'];
      const leg = Math.min(Math.floor(shape), 2);
      k = shape <= 0.0005 || shape >= 2.9995 ? 0 : ease(shape - leg);
      first = shape >= 2.9995 ? 'main' : order[leg];
      second = shape >= 2.9995 ? 'main' : order[leg + 1];
      place = blendPlace(sized(first, places.from), sized(second, places.to), k, 0, places.now);
    }

    inside.group.position.copy(place.position);
    inside.group.quaternion.copy(place.quaternion);
    inside.group.scale.setScalar(1);
    inside.plane.scale.set(place.width / stage.width, place.height / stage.height, place.height / stage.height);

    const u = inside.uniforms;
    u.uTime.value = state.clock.elapsedTime;
    const { from, to, s } = frame.select(story.place);
    u.uMapA.value = frame.states[from].map;
    u.uMapB.value = frame.states[to].map;
    u.uMix.value = s;
    const pick = (id: string) => (id === 'main' ? frame : (frames[id] ?? frame));
    u.uFrame.value = pick(first).texture;
    u.uFrameB.value = pick(second).texture;
    u.uBlend.value = first === second ? 0 : Math.min(Math.max((k - 0.18) / 0.64, 0), 1);
    u.uSweep.value = story.sweep;
    u.uRelief.value = story.relief;
    u.uFade.value = 1;
    inside.tileUniforms.uOutline.value = 0;

    const rects = (id: string, index: number) => {
      if (id === 'main') return own(index);
      const layout = layouts[id as 'wide' | 'tall' | 'swans' | 'moon'];
      return layout?.[index] ?? own(index);
    };
    const tileHeight = Math.min(Math.max((lift - 0.15) / 0.5, 0), 1) * 0.1 * raise;
    for (let i = 0; i < manifest.icons.length && i < ICON_SLOTS; i++) {
      const a = rects(first, i);
      const b = rects(second, i);
      const x = a[0] + (b[0] - a[0]) * k;
      const y = a[1] + (b[1] - a[1]) * k;
      const w = a[2] + (b[2] - a[2]) * k;
      const h = a[3] + (b[3] - a[3]) * k;
      let height = tileHeight;
      if (layouts.ids[i] === drag.icon && fly <= 0.0005 && story.shape <= 0.0005) {
        height += story.grab * 0.05 + Math.sin(Math.PI * Math.min(Math.max(story.drag, 0), 1)) * story.grab * 0.08;
      }
      u.uTiles.value[i].set(x, y, w, h);
      u.uTileHeight.value[i] = height;
      scratch.position.set((x + w / 2 - 0.5) * place.width, (0.5 - (y + h / 2)) * place.height, height * place.height);
      scratch.scale.set(w * place.width, h * place.height, 1);
      scratch.matrix.compose(scratch.position, scratch.quaternion.identity(), scratch.scale);
      inside.tiles.setMatrixAt(i, scratch.matrix);
    }
    inside.tiles.instanceMatrix.needsUpdate = true;
  });

  return inside;
}
