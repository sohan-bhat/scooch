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
uniform float uOn;
uniform float uOff;
uniform float uPower;
uniform vec2 uLines;
uniform vec3 uLamp;
uniform vec3 uLampColor;
uniform float uLampLevel;
uniform vec3 uRim;
uniform vec3 uGlass;
uniform float uDim;

varying vec2 vUv;
varying vec3 vWorld;
varying vec3 vNormal;
${pictureBody}

const float PI = 3.14159265;

vec2 barrel(vec2 uv) {
  vec2 c = uv * 2.0 - 1.0;
  c *= 1.0 + vec2(0.035, 0.05) * dot(c, c);
  return c * 0.5 + 0.5;
}

float roundedBox(vec2 p, vec2 extent, float radius) {
  vec2 q = abs(p) - extent + radius;
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - radius;
}

void envelope(out vec2 size, out float boost, out float visible, out float spot) {
  float o = clamp(uOn, 0.0, 1.0);
  float grow = smoothstep(0.0, 0.14, o);
  float open = smoothstep(0.14, 0.6, o);
  open = open * open * (3.0 - 2.0 * open);
  vec2 onSize = vec2(mix(0.04, 1.0, grow), mix(0.008, 1.0, open));
  float onBoost = 1.0 + 3.0 * (1.0 - smoothstep(0.18, 0.95, o));

  float f = clamp(uOff, 0.0, 1.0);
  float squash = smoothstep(0.0, 0.4, f);
  squash = squash * squash;
  float shrink = smoothstep(0.4, 0.74, f);
  float fade = 1.0 - smoothstep(0.74, 1.0, f);
  vec2 offSize = vec2(mix(1.0, 0.014, shrink * shrink), mix(1.0, 0.008, squash));
  float offBoost = 1.0 + 3.5 * smoothstep(0.05, 0.4, f);

  size = onSize * offSize;
  boost = onBoost * offBoost;
  visible = step(0.0001, o) * fade;
  spot = smoothstep(0.55, 0.74, f) * fade;
}

void main() {
  vec2 uv = vUv;
  vec2 raster = vec2(0.47, 0.465);
  vec2 c = uv - 0.5;
  float frame = roundedBox(c, raster, 0.07);
  float edge = fwidth(frame) * 1.5 + 0.0005;
  float mask = 1.0 - smoothstep(-edge, edge, frame);

  vec2 size;
  float boost;
  float visible;
  float spot;
  envelope(size, boost, visible, spot);

  vec2 local = c / (raster * 2.0);
  vec2 squeezed = local / size;
  float within = step(abs(squeezed.x), 0.5) * step(abs(squeezed.y), 0.5);
  float soft = (1.0 - smoothstep(0.42, 0.5, abs(squeezed.y))) * (1.0 - smoothstep(0.46, 0.5, abs(squeezed.x)));
  vec2 pv = barrel(squeezed + 0.5);
  vec2 dx = dFdx(pv);
  vec2 dy = dFdy(pv);

  vec3 color = picture(pv);
  color = icons(color, pv, dx, dy);
  color = cursor(color, pv, dx, dy);
  vec3 halo = pictureSoft(pv, 4.5);
  float bright = dot(halo, vec3(0.2126, 0.7152, 0.0722));
  color += halo * smoothstep(0.45, 0.95, bright) * 0.35;

  float line = pv.y * uLines.y;
  float lineWidth = fwidth(line);
  float scanAmount = 1.0 - smoothstep(0.3, 0.75, lineWidth);
  float profile = 0.5 + 0.5 * cos(2.0 * PI * line);
  float scan = mix(1.0, 0.62 + 0.62 * profile, scanAmount * 0.85);

  float column = pv.x * uLines.x * 3.0;
  float columnWidth = fwidth(column);
  float maskAmount = 1.0 - smoothstep(0.35, 0.9, columnWidth);
  float phase = fract(column / 3.0) * 3.0;
  vec3 grille = vec3(
    1.0 - smoothstep(0.85, 1.0, phase) + smoothstep(2.85, 3.0, phase),
    smoothstep(0.85, 1.0, phase) * (1.0 - smoothstep(1.85, 2.0, phase)),
    smoothstep(1.85, 2.0, phase) * (1.0 - smoothstep(2.85, 3.0, phase))
  );
  vec3 phosphor = mix(vec3(1.0), grille * 2.2 + 0.18, maskAmount * 0.55);

  float flicker = 1.0 + 0.012 * sin(uTime * 61.0) + 0.006 * sin(uTime * 7.3 + pv.y * 3.0);
  float roll = 1.0 + 0.025 * smoothstep(0.0, 0.08, fract(pv.y * 0.6 - uTime * 0.11)) * (1.0 - smoothstep(0.08, 0.2, fract(pv.y * 0.6 - uTime * 0.11)));
  vec2 lens = local * 2.0;
  float vignette = 1.0 - 0.38 * pow(dot(lens * vec2(0.82, 0.9), lens * vec2(0.82, 0.9)), 1.6) * 0.5;

  vec3 lit = color * scan * phosphor * flicker * roll * vignette * 1.08;
  lit = mix(lit, vec3(1.0, 0.98, 0.95) * dot(lit, vec3(0.333)) * 1.6 + lit * 0.4, smoothstep(1.4, 3.0, boost) * 0.6) * boost;
  lit *= within * soft * visible;

  float dotDistance = length(c * vec2(1.0, 1.0));
  vec3 dotColor = vec3(1.0, 0.97, 0.92) * exp(-dotDistance * dotDistance * 9000.0) * 6.0 * spot;
  dotColor += vec3(0.6, 0.7, 0.9) * exp(-dotDistance * 60.0) * 0.25 * spot;

  vec3 glass = uGlass;
  vec3 screen = glass + (lit * uPower + dotColor) * mask * (1.0 - uDim);

  vec3 n = normalize(vNormal);
  vec3 v = normalize(cameraPosition - vWorld);
  vec3 l = normalize(uLamp - vWorld);
  vec3 h = normalize(l + v);
  float spec = pow(max(dot(n, h), 0.0), 60.0);
  float sheen = pow(max(dot(n, h), 0.0), 6.0);
  float fresnel = pow(1.0 - max(dot(n, v), 0.0), 4.0);
  vec3 glare = uLampColor * uLampLevel * (spec * 0.9 + sheen * 0.03);
  glare += uRim * fresnel * 0.18;

  gl_FragColor = vec4(screen + glare, 1.0);
}
`;

export type CrtUniforms = PictureUniforms & {
  uOn: { value: number };
  uOff: { value: number };
  uPower: { value: number };
  uLines: { value: Vector2 };
  uOrigin: { value: Vector2 };
  uExtent: { value: Vector2 };
  uLamp: { value: Vector3 };
  uLampColor: { value: Color };
  uLampLevel: { value: number };
  uRim: { value: Color };
  uGlass: { value: Color };
  uDim: { value: number };
};

export function createCrtMaterial() {
  const uniforms: CrtUniforms = {
    ...pictureUniforms(),
    uOn: { value: 1 },
    uOff: { value: 0 },
    uPower: { value: 1 },
    uLines: { value: new Vector2(320, 240) },
    uOrigin: { value: new Vector2() },
    uExtent: { value: new Vector2(1, 1) },
    uLamp: { value: new Vector3(0, 2, 0) },
    uLampColor: { value: new Color(1, 0.7, 0.4) },
    uLampLevel: { value: 1 },
    uRim: { value: new Color() },
    uGlass: { value: new Color(0.012, 0.014, 0.014) },
    uDim: { value: 0 },
  };
  const material = new ShaderMaterial({ name: 'scooch-crt', vertexShader: vertex, fragmentShader: fragment, uniforms });
  return { material, uniforms };
}
