import { useMemo } from 'react';
import { Color, Matrix4, ShaderMaterial, UniformsLib, UniformsUtils, Vector2, type Texture } from 'three';
import { tokenColor } from '@/lib/tokens';

const vertex = `
varying vec3 vWorld;
#include <fog_pars_vertex>

void main() {
  vec4 world = modelMatrix * vec4(position, 1.0);
  vWorld = world.xyz;
  vec4 mvPosition = viewMatrix * world;
  gl_Position = projectionMatrix * mvPosition;
  #include <fog_vertex>
}
`;

const fragment = `
uniform vec3 uColor;
uniform vec3 uGlow;
uniform vec2 uCenter;
uniform float uLevel;
uniform float uFloor;
uniform sampler2D uBlinds;
uniform mat4 uBlindsMatrix;
uniform vec3 uBlindsColor;
uniform float uBlindsLevel;
uniform float uFade;
uniform sampler2D uCaption;
uniform float uCaptionLevel;
uniform vec2 uDrift;
varying vec3 vWorld;
#include <fog_pars_fragment>

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

void main() {
  vec2 p = mix(vWorld.xy, vWorld.xz, uFloor);
  vec2 offset = (p - uCenter) * vec2(0.75, 1.0);
  float pool = exp(-dot(offset, offset) * 2.4);
  float plaster = 0.9 + 0.07 * noise(p * 13.0) + 0.05 * noise(p * 47.0) + 0.02 * noise(p * 160.0);
  vec3 color = uColor * plaster * (0.22 + uGlow * pool * uLevel * 4.5);
  vec4 projected = uBlindsMatrix * vec4(vWorld, 1.0);
  vec2 window = projected.xy / max(projected.w, 1e-4);
  float front = step(0.0, projected.w) * step(0.0, window.x) * step(window.x, 1.0) * step(0.0, window.y) * step(window.y, 1.0);
  float cone = 1.0 - smoothstep(0.34, 0.5, length(window - 0.5));
  float slats = texture2D(uBlinds, window + uDrift).r;
  vec4 caption = texture2D(uCaption, window);
  slats = mix(slats, caption.r * 9.0, caption.g * uCaptionLevel);
  color += uBlindsColor * uColor * plaster * slats * cone * front * uBlindsLevel * 3.2;
  gl_FragColor = vec4(color * uFade, 1.0);
  #include <fog_fragment>
}
`;

export function useRoomMaterial(kind: 'wall' | 'floor', center: Vector2, blinds: Texture) {
  return useMemo(() => {
    const uniforms = UniformsUtils.merge([
      UniformsLib.fog,
      {
        uColor: { value: new Color() },
        uGlow: { value: new Color() },
        uCenter: { value: new Vector2() },
        uLevel: { value: 1 },
        uFloor: { value: kind === 'floor' ? 1 : 0 },
        uBlinds: { value: null as Texture | null },
        uBlindsMatrix: { value: new Matrix4() },
        uBlindsColor: { value: new Color() },
        uBlindsLevel: { value: 0 },
        uFade: { value: 1 },
        uCaption: { value: null as Texture | null },
        uCaptionLevel: { value: 0 },
        uDrift: { value: new Vector2() },
      },
    ]);
    uniforms.uColor.value = tokenColor(kind === 'wall' ? 'room-3' : 'room-2');
    uniforms.uGlow.value = tokenColor('bulb');
    uniforms.uCenter.value = center.clone();
    uniforms.uBlinds.value = blinds;
    uniforms.uBlindsColor.value = tokenColor('rim');
    return new ShaderMaterial({ name: `scooch-${kind}`, vertexShader: vertex, fragmentShader: fragment, uniforms, fog: true });
  }, [kind, center, blinds]);
}
