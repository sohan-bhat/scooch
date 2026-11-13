import { Color, ShaderMaterial, Vector2, Vector3 } from 'three';
import { pictureBody, pictureHead, pictureUniforms, type PictureUniforms } from './picture';

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
${pictureHead}
uniform float uPower;
uniform vec2 uGrid;
uniform vec3 uLamp;
uniform vec3 uLampColor;
uniform float uLampLevel;
uniform vec3 uRim;

varying vec2 vUv;
varying vec3 vWorld;
varying vec3 vNormal;
${pictureBody}

void main() {
  vec2 uv = vUv;
  vec2 dx = dFdx(uv);
  vec2 dy = dFdy(uv);
  float inside = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);

  vec3 color = picture(uv);
  color = icons(color, uv, dx, dy);
  color = cursor(color, uv, dx, dy);

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
    color *= mix(vec3(1.0), stripes * rows * 2.4, near * 0.38);
  }

  vec2 centered = uv - 0.5;
  float tube = 1.0 + 0.07 * exp(-(1.0 - uv.y) * 7.0) + 0.03 * exp(-uv.y * 9.0);
  float blotch = 1.0 + (noise(uv * vec2(2.2, 1.6) + 4.0) * 0.65 + noise(uv * 5.3 + 9.0) * 0.35 - 0.5) * 0.08;
  float corners = 1.0 - 0.32 * pow(dot(centered * vec2(1.0, 1.2), centered * vec2(1.0, 1.2)), 1.4) * 2.2;
  float hum = 1.0 + 0.006 * sin(uTime * 2.1 + uv.y * 7.0);
  vec3 black = vec3(0.016, 0.018, 0.021);
  vec3 lit = black + color * tube * blotch * corners * hum * 1.12;
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

export type LcdUniforms = PictureUniforms & {
  uPower: { value: number };
  uGrid: { value: Vector2 };
  uOrigin: { value: Vector2 };
  uExtent: { value: Vector2 };
  uLamp: { value: Vector3 };
  uLampColor: { value: Color };
  uLampLevel: { value: number };
  uRim: { value: Color };
};

export function createLcdMaterial() {
  const uniforms: LcdUniforms = {
    ...pictureUniforms(),
    uPower: { value: 1 },
    uGrid: { value: new Vector2(640, 480) },
    uOrigin: { value: new Vector2() },
    uExtent: { value: new Vector2(1, 1) },
    uLamp: { value: new Vector3(0, 2, 0) },
    uLampColor: { value: new Color(1, 0.7, 0.4) },
    uLampLevel: { value: 1 },
    uRim: { value: new Color() },
  };
  const material = new ShaderMaterial({ name: 'scooch-lcd', vertexShader: vertex, fragmentShader: fragment, uniforms });
  return { material, uniforms };
}
