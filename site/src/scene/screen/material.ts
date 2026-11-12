import { Color, ShaderMaterial, Texture, Vector2, Vector3, Vector4 } from 'three';

export const ICON_SLOTS = 6;
export const ATLAS_GRID = 4;
export const CURSOR_CELL = 6;
export const CURSOR_TIP = 0.066;

const vertex = `
uniform vec2 uOrigin;
uniform vec2 uExtent;
varying vec2 vUv;
varying vec3 vWorld;
varying vec3 vNormal;

void main() {
  vec2 local = (position.xy - uOrigin) / uExtent;
  vUv = vec2(local.x, 1.0 - local.y);
  vec4 world = modelMatrix * vec4(position, 1.0);
  vWorld = world.xyz;
  vNormal = normalize(mat3(modelMatrix) * normal);
  gl_Position = projectionMatrix * viewMatrix * world;
}
`;

const fragment = `
precision highp float;

#define ICON_SLOTS ${ICON_SLOTS}
#define GRID ${ATLAS_GRID}.0

uniform sampler2D uFrameA;
uniform sampler2D uFrameB;
uniform sampler2D uFrameC;
uniform sampler2D uSaliency;
uniform sampler2D uAtlas;
uniform float uMixAB;
uniform float uMixBC;
uniform float uSweep;
uniform float uPower;
uniform float uTime;
uniform float uAspect;
uniform vec2 uGrid;
uniform vec4 uIcons[ICON_SLOTS];
uniform float uGhost[ICON_SLOTS];
uniform vec3 uCursor;
uniform vec2 uCursorSize;
uniform vec3 uLamp;
uniform vec3 uLampColor;
uniform float uLampLevel;
uniform vec3 uRoom;
uniform vec3 uHeat;
uniform vec3 uGlow;
uniform vec3 uHot;
uniform vec3 uRim;

varying vec2 vUv;
varying vec3 vWorld;
varying vec3 vNormal;

float hash(vec2 p) {
  vec3 q = fract(vec3(p.xyx) * 0.1031);
  q += dot(q, q.yzx + 33.33);
  return fract((q.x + q.y) * q.z);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  float a = hash(i);
  float b = hash(i + vec2(1.0, 0.0));
  float c = hash(i + vec2(0.0, 1.0));
  float d = hash(i + vec2(1.0, 1.0));
  return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

float fbm(vec2 p) {
  float sum = 0.0;
  float amp = 0.5;
  for (int i = 0; i < 5; i++) {
    sum += amp * noise(p);
    p = p * 2.03 + vec2(17.1, 9.2);
    amp *= 0.5;
  }
  return sum;
}

vec3 wallpaper(vec2 uv) {
  vec3 a = texture(uFrameA, uv).rgb;
  vec3 b = texture(uFrameB, uv).rgb;
  vec3 c = texture(uFrameC, uv).rgb;
  return mix(mix(a, b, uMixAB), c, uMixBC);
}

vec3 heatmap(float s, vec2 uv) {
  float soft = texture(uSaliency, uv, 2.5).r;
  float level = clamp(max(s, soft * 0.85), 0.0, 1.0);
  vec3 color = uRoom * 0.55;
  color = mix(color, uHeat * 0.55, smoothstep(0.04, 0.45, level));
  color = mix(color, uHeat * 1.25, smoothstep(0.3, 0.7, level));
  color = mix(color, uGlow * 1.35, smoothstep(0.62, 0.92, level));
  color = mix(color, uHot * 1.6, smoothstep(0.9, 1.0, level));
  float lines = abs(fract(level * 7.0) - 0.5);
  float width = fwidth(level * 7.0) * 1.2;
  float contour = 1.0 - smoothstep(0.0, width + 0.001, lines - 0.46);
  color += uGlow * contour * 0.05 * smoothstep(0.05, 0.3, level);
  return color;
}

vec4 atlasCell(float index, float row, vec2 q, vec2 dx, vec2 dy, vec2 scale) {
  vec2 cell = vec2(mod(index, GRID), row + floor(index / GRID));
  vec2 at = (cell + clamp(q, 0.002, 0.998)) / GRID;
  return textureGrad(uAtlas, at, dx * scale, dy * scale);
}

void main() {
  vec2 uv = vUv;
  vec2 dx = dFdx(uv);
  vec2 dy = dFdy(uv);
  float inside = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);

  float reveal = step(0.5, uSweep);
  float wet = 0.0;
  float rim = 0.0;
  if (uSweep > 0.0005 && uSweep < 0.9995) {
    vec2 paper = vec2(uv.x * uAspect, uv.y);
    float diagonal = (paper.x + uv.y) / (uAspect + 1.0);
    float warp = fbm(paper * 2.3 + vec2(0.0, uTime * 0.025));
    float wobble = (warp - 0.5) * 0.13;
    float feather = (fbm(paper * 13.0 + warp * 3.0) - 0.5) * 0.024;
    float front = mix(-0.2, 1.2, uSweep);
    float behind = front - (diagonal + wobble + feather);
    reveal = smoothstep(0.0, 0.055, behind);
    wet = smoothstep(-0.035, 0.0, behind) * (1.0 - smoothstep(0.0, 0.07, behind));
    rim = exp(-abs(behind) * 170.0) * (0.6 + 0.5 * fbm(paper * 40.0)) + exp(-max(behind, 0.0) * 30.0) * step(0.0, behind) * 0.12;
  }

  vec3 photo = wallpaper(uv);
  vec3 color = photo * (1.0 - wet * 0.5);
  if (reveal > 0.0) {
    float s = texture(uSaliency, uv).r;
    color = mix(color, heatmap(s, uv), reveal);
  }

  vec2 ghostScale = vec2(1.0);
  for (int i = 0; i < ICON_SLOTS; i++) {
    vec4 rect = uIcons[i];
    vec2 q = (uv - rect.xy) / rect.zw;
    float hit = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
    if (hit > 0.0) {
      vec2 scale = 1.0 / (rect.zw * GRID);
      vec4 filled = atlasCell(float(i), 0.0, q, dx, dy, scale);
      vec4 outline = atlasCell(float(i), 2.0, q, dx, dy, scale);
      vec4 icon = mix(filled, outline, reveal);
      float alpha = icon.a * (1.0 - uGhost[i] * 0.45);
      color = color * (1.0 - alpha) + icon.rgb * (1.0 - uGhost[i] * 0.45);
    }
  }

  if (uCursor.z > 0.0) {
    vec2 q = (uv - uCursor.xy) / uCursorSize + ${CURSOR_TIP};
    float hit = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
    if (hit > 0.0) {
      vec4 pointer = atlasCell(${CURSOR_CELL}.0, 0.0, q, dx, dy, 1.0 / (uCursorSize * GRID));
      color = color * (1.0 - pointer.a * uCursor.z) + pointer.rgb * uCursor.z;
    }
  }

  vec3 ink = mix(uHeat, uGlow, 0.7);
  color += ink * rim;

  vec2 cell = uv * uGrid;
  vec2 sub = fract(cell);
  float cellPixels = 1.0 / max(length(dx * uGrid), length(dy * uGrid));
  float near = smoothstep(2.6, 4.4, cellPixels);
  if (near > 0.0) {
    vec3 stripes = vec3(
      smoothstep(0.02, 0.08, sub.x) * (1.0 - smoothstep(0.3, 0.36, sub.x)),
      smoothstep(0.34, 0.4, sub.x) * (1.0 - smoothstep(0.62, 0.68, sub.x)),
      smoothstep(0.66, 0.72, sub.x) * (1.0 - smoothstep(0.94, 1.0, sub.x))
    );
    float rows = smoothstep(0.02, 0.12, sub.y) * (1.0 - smoothstep(0.86, 0.98, sub.y));
    vec3 lattice = stripes * rows * 2.4;
    color *= mix(vec3(1.0), lattice, near * 0.38);
  }

  vec2 centered = uv - 0.5;
  float tube = 1.0 + 0.07 * exp(-(1.0 - uv.y) * 7.0) + 0.03 * exp(-uv.y * 9.0);
  float blotch = 1.0 + (fbm(uv * vec2(2.2, 1.6) + 4.0) - 0.5) * 0.08;
  float corners = 1.0 - 0.32 * pow(dot(centered * vec2(1.0, 1.2), centered * vec2(1.0, 1.2)), 1.4) * 2.2;
  float hum = 1.0 + 0.006 * sin(uTime * 2.1 + uv.y * 7.0);
  vec3 black = vec3(0.016, 0.018, 0.021);
  vec3 lit = black + color * tube * blotch * corners * hum * 1.28;
  color = mix(black * 0.7, lit, inside) * uPower + black * 0.4 * (1.0 - uPower);

  vec3 n = normalize(vNormal);
  vec3 v = normalize(cameraPosition - vWorld);
  vec3 l = normalize(uLamp - vWorld);
  vec3 h = normalize(l + v);
  float spec = pow(max(dot(n, h), 0.0), 18.0);
  float streak = smoothstep(0.55, 0.0, abs(dot(centered, normalize(vec2(1.0, -0.62))) - 0.12)) * 0.5;
  float fresnel = pow(1.0 - max(dot(n, v), 0.0), 5.0);
  vec3 glare = uLampColor * uLampLevel * (spec * 0.12 + streak * spec * 0.25 + 0.006);
  glare += uRim * fresnel * 0.08;

  gl_FragColor = vec4(color + glare, 1.0);
}
`;

export type ScreenUniforms = {
  uFrameA: { value: Texture | null };
  uFrameB: { value: Texture | null };
  uFrameC: { value: Texture | null };
  uSaliency: { value: Texture | null };
  uAtlas: { value: Texture | null };
  uMixAB: { value: number };
  uMixBC: { value: number };
  uSweep: { value: number };
  uPower: { value: number };
  uTime: { value: number };
  uAspect: { value: number };
  uGrid: { value: Vector2 };
  uOrigin: { value: Vector2 };
  uExtent: { value: Vector2 };
  uIcons: { value: Vector4[] };
  uGhost: { value: number[] };
  uCursor: { value: Vector3 };
  uCursorSize: { value: Vector2 };
  uLamp: { value: Vector3 };
  uLampColor: { value: Color };
  uLampLevel: { value: number };
  uRoom: { value: Color };
  uHeat: { value: Color };
  uGlow: { value: Color };
  uHot: { value: Color };
  uRim: { value: Color };
};

export function createScreenMaterial() {
  const uniforms: ScreenUniforms = {
    uFrameA: { value: null },
    uFrameB: { value: null },
    uFrameC: { value: null },
    uSaliency: { value: null },
    uAtlas: { value: null },
    uMixAB: { value: 0 },
    uMixBC: { value: 0 },
    uSweep: { value: 0 },
    uPower: { value: 1 },
    uTime: { value: 0 },
    uAspect: { value: 4 / 3 },
    uGrid: { value: new Vector2(640, 480) },
    uOrigin: { value: new Vector2() },
    uExtent: { value: new Vector2(1, 1) },
    uIcons: { value: Array.from({ length: ICON_SLOTS }, () => new Vector4(-1, -1, 0.01, 0.01)) },
    uGhost: { value: new Array(ICON_SLOTS).fill(0) },
    uCursor: { value: new Vector3(0.5, 0.5, 0) },
    uCursorSize: { value: new Vector2(0.06, 0.08) },
    uLamp: { value: new Vector3(0, 2, 0) },
    uLampColor: { value: new Color(1, 0.7, 0.4) },
    uLampLevel: { value: 1 },
    uRoom: { value: new Color() },
    uHeat: { value: new Color() },
    uGlow: { value: new Color() },
    uHot: { value: new Color() },
    uRim: { value: new Color() },
  };
  const material = new ShaderMaterial({
    name: 'scooch-screen',
    vertexShader: vertex,
    fragmentShader: fragment,
    uniforms,
  });
  return { material, uniforms };
}
