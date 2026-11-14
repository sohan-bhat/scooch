import { Color, Matrix4, Vector2, Vector3, type Camera, type MeshStandardMaterial, type Texture } from 'three';
import { STREET, projector } from './Blinds';

const STRIPE_ANGLE = 0.1;
const STRIPE_PENUMBRA = 0.6;

export const windowLight = {
  uWindowMap: { value: null as Texture | null },
  uWindowFromView: { value: new Matrix4() },
  uWindowPosition: { value: new Vector3() },
  uWindowAxis: { value: new Vector3() },
  uWindowCone: { value: new Vector2(Math.cos(STREET.angle), Math.cos(STREET.angle * 0.75)) },
  uWindowColor: { value: new Color(0, 0, 0) },
  uStripeColor: { value: new Color(0, 0, 0) },
  uStripeAxis: { value: new Vector3() },
  uStripeUp: { value: new Vector3() },
  uStripeCone: { value: new Vector2(Math.cos(STRIPE_ANGLE), Math.cos(STRIPE_ANGLE * (1 - STRIPE_PENUMBRA))) },
  uStripeScale: { value: 1 / Math.tan(STRIPE_ANGLE) },
};

const declare = `uniform sampler2D uWindowMap;
uniform mat4 uWindowFromView;
uniform vec3 uWindowPosition;
uniform vec3 uWindowAxis;
uniform vec2 uWindowCone;
uniform vec3 uWindowColor;
uniform vec3 uStripeColor;
uniform vec3 uStripeAxis;
uniform vec3 uStripeUp;
uniform vec2 uStripeCone;
uniform float uStripeScale;
float windowBand(float v, float center) {
  return clamp((0.11 - abs(v - center)) / 0.03, 0.0, 1.0);
}
`;

const term = `{
  vec3 toWindow = uWindowPosition + vViewPosition;
  float windowDistance = max(dot(toWindow, toWindow), 1e-4);
  vec3 windowDirection = toWindow * inversesqrt(windowDistance);
  float windowLambert = saturate(dot(geometryNormal, windowDirection));
  #ifdef WINDOW_WRAP
  windowLambert = mix(windowLambert, saturate((dot(geometryNormal, windowDirection) + 0.75) / 1.75), uJuice);
  #endif
  if (windowLambert > 0.0) {
    vec3 windowRay = -windowDirection;
    vec4 windowCoord = uWindowFromView * vec4(-vViewPosition, 1.0);
    vec2 windowUv = windowCoord.xy / windowCoord.w;
    float blinds = (windowCoord.w > 0.0 && all(greaterThan(windowUv, vec2(0.0))) && all(lessThan(windowUv, vec2(1.0)))) ? texture2D(uWindowMap, windowUv).r : 0.0;
    blinds *= smoothstep(uWindowCone.x, uWindowCone.y, dot(windowRay, uWindowAxis));
    float along = dot(windowRay, uStripeAxis);
    float stripe = smoothstep(uStripeCone.x, uStripeCone.y, along);
    float v = 0.5 + 0.5 * dot(windowRay, uStripeUp) / max(along, 1e-3) * uStripeScale;
    stripe *= max(windowBand(v, 0.5), max(windowBand(v, 0.1), windowBand(v, 0.9)) * 0.45);
    vec3 windowIrradiance = (uWindowColor * blinds + uStripeColor * stripe) * windowLambert / windowDistance;
    reflectedLight.directDiffuse += windowIrradiance * BRDF_Lambert(material.diffuseColor);
  }
}
#include <lights_fragment_maps>`;

export function windowLit(material: MeshStandardMaterial) {
  if (material.userData.window) return;
  material.userData.window = true;
  const previous = material.onBeforeCompile;
  const key = material.customProgramCacheKey();
  material.onBeforeCompile = (shader, renderer) => {
    previous.call(material, shader, renderer);
    Object.assign(shader.uniforms, windowLight);
    shader.fragmentShader = shader.fragmentShader.replace('#include <common>', `#include <common>\n${declare}`).replace('#include <lights_fragment_maps>', term);
  };
  material.customProgramCacheKey = () => `${key}|window`;
}

const projection = projector();
const target = new Vector3();
const stripeTarget = new Vector3();
const axis = new Vector3();
const right = new Vector3();
const up = new Vector3();
const world = new Vector3(0, 1, 0);

export function placeStripe(aim: Vector3) {
  stripeTarget.copy(aim);
}

export function updateWindow(camera: Camera, street: Color, stripe: Color) {
  camera.updateMatrixWorld();
  windowLight.uWindowFromView.value.multiplyMatrices(projection, camera.matrixWorld);
  windowLight.uWindowPosition.value.copy(STREET.position).applyMatrix4(camera.matrixWorldInverse);
  windowLight.uWindowAxis.value.copy(target.copy(STREET.target).sub(STREET.position).normalize()).transformDirection(camera.matrixWorldInverse);
  axis.copy(stripeTarget).sub(STREET.position).normalize();
  right.crossVectors(axis, world).normalize();
  up.crossVectors(right, axis).normalize();
  windowLight.uStripeAxis.value.copy(axis).transformDirection(camera.matrixWorldInverse);
  windowLight.uStripeUp.value.copy(up).transformDirection(camera.matrixWorldInverse);
  windowLight.uWindowColor.value.copy(street);
  windowLight.uStripeColor.value.copy(stripe);
}
