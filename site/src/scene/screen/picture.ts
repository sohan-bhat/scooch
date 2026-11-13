import { Texture, Vector2, Vector3, Vector4 } from 'three';

export const ICON_SLOTS = 6;
export const ATLAS_GRID = 4;
export const CURSOR_CELL = 6;
export const CURSOR_TIP = 0.066;

export const pictureHead = `
#define ICON_SLOTS ${ICON_SLOTS}
#define GRID ${ATLAS_GRID}.0

uniform sampler2D uFrame;
uniform highp sampler2D uMapA;
uniform highp sampler2D uMapB;
uniform float uMix;
uniform vec2 uScreen;
uniform sampler2D uAtlas;
uniform float uTime;
uniform vec4 uIcons[ICON_SLOTS];
uniform float uGhost[ICON_SLOTS];
uniform float uOutline;
uniform vec3 uCursor;
uniform vec2 uCursorSize;
`;

export const pictureBody = `
float hash(vec2 p) {
  vec3 q = fract(vec3(p.xyx) * 0.1031);
  q += dot(q, q.yzx + 33.33);
  return fract((q.x + q.y) * q.z);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x), mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
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

vec2 place(vec2 uv) {
  ivec2 pixel = ivec2(clamp(uv, 0.0, 0.9999) * uScreen);
  vec2 a = vec2(texelFetch(uMapA, ivec2(pixel.x, 0), 0).r, texelFetch(uMapA, ivec2(pixel.y, 1), 0).r);
  vec2 b = vec2(texelFetch(uMapB, ivec2(pixel.x, 0), 0).r, texelFetch(uMapB, ivec2(pixel.y, 1), 0).r);
  return mix(a, b, uMix);
}

vec3 picture(vec2 uv) {
  vec2 at = clamp(uv, 0.0, 1.0);
  return texture(uFrame, vec2(at.x, 1.0 - at.y)).rgb;
}

vec3 pictureSoft(vec2 uv, float lod) {
  vec2 at = clamp(uv, 0.0, 1.0);
  return textureLod(uFrame, vec2(at.x, 1.0 - at.y), lod).rgb;
}

vec4 atlasCell(float index, float row, vec2 q, vec2 dx, vec2 dy, vec2 scale) {
  vec2 cell = vec2(mod(index, GRID), row + floor(index / GRID));
  vec2 at = (cell + clamp(q, 0.002, 0.998)) / GRID;
  vec2 size = vec2(textureSize(uAtlas, 0));
  float footprint = max(length(dx * scale * size), length(dy * scale * size));
  float lod = clamp(log2(max(footprint, 1e-4)), 0.0, 10.0);
  return clamp(textureLod(uAtlas, at, lod), 0.0, 1.0);
}

vec3 icons(vec3 color, vec2 uv, vec2 dx, vec2 dy) {
  for (int i = 0; i < ICON_SLOTS; i++) {
    vec4 rect = uIcons[i];
    vec2 q = (uv - rect.xy) / rect.zw;
    float hit = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
    if (hit > 0.0) {
      vec2 scale = 1.0 / (rect.zw * GRID);
      vec4 filled = atlasCell(float(i), 0.0, q, dx, dy, scale);
      vec4 icon = filled;
      if (uOutline > 0.0) icon = mix(filled, atlasCell(float(i), 2.0, q, dx, dy, scale), uOutline);
      float alpha = icon.a * (1.0 - uGhost[i]);
      color = color * (1.0 - alpha) + icon.rgb * alpha;
    }
  }
  return color;
}

vec3 cursor(vec3 color, vec2 uv, vec2 dx, vec2 dy) {
  if (uCursor.z <= 0.0) return color;
  vec2 q = (uv - uCursor.xy) / uCursorSize + ${CURSOR_TIP};
  float hit = step(0.0, q.x) * step(q.x, 1.0) * step(0.0, q.y) * step(q.y, 1.0);
  if (hit > 0.0) {
    vec4 pointer = atlasCell(${CURSOR_CELL}.0, 0.0, q, dx, dy, 1.0 / (uCursorSize * GRID));
    color = color * (1.0 - pointer.a * uCursor.z) + pointer.rgb * pointer.a * uCursor.z;
  }
  return color;
}
`;

export type PictureUniforms = {
  uFrame: { value: Texture | null };
  uMapA: { value: Texture | null };
  uMapB: { value: Texture | null };
  uMix: { value: number };
  uScreen: { value: Vector2 };
  uAtlas: { value: Texture | null };
  uTime: { value: number };
  uIcons: { value: Vector4[] };
  uGhost: { value: number[] };
  uOutline: { value: number };
  uCursor: { value: Vector3 };
  uCursorSize: { value: Vector2 };
};

export function pictureUniforms(): PictureUniforms {
  return {
    uFrame: { value: null },
    uMapA: { value: null },
    uMapB: { value: null },
    uMix: { value: 0 },
    uScreen: { value: new Vector2(1280, 960) },
    uAtlas: { value: null },
    uTime: { value: 0 },
    uIcons: { value: Array.from({ length: ICON_SLOTS }, () => new Vector4(-1, -1, 0.01, 0.01)) },
    uGhost: { value: new Array(ICON_SLOTS).fill(0) },
    uOutline: { value: 0 },
    uCursor: { value: new Vector3(0.5, 0.5, 0) },
    uCursorSize: { value: new Vector2(0.06, 0.08) },
  };
}
