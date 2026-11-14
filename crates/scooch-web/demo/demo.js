import { Remote } from '../pkg/engine.js';

const VERTEX = `#version 300 es
void main() {
  vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}`;

const SAMPLERS = ['canvas', 'map_a', 'map_b', 'field_a', 'field_b', 'look_a', 'look_b', 'pad_a', 'pad_b', 'paints_a', 'paints_b', 'ramps_a', 'ramps_b'];

/**
 * @param {WebGL2RenderingContext} gl
 * @param {number} type
 * @param {string} source
 */
function shader(gl, type, source) {
  const s = /** @type {WebGLShader} */ (gl.createShader(type));
  gl.shaderSource(s, source);
  gl.compileShader(s);
  if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(String(gl.getShaderInfoLog(s)));
  return s;
}

/**
 * @param {WebGL2RenderingContext} gl
 * @param {number} target
 * @param {boolean} smooth
 */
function texture(gl, target, smooth) {
  const t = /** @type {WebGLTexture} */ (gl.createTexture());
  gl.bindTexture(target, t);
  gl.texParameteri(target, gl.TEXTURE_MIN_FILTER, smooth ? gl.LINEAR : gl.NEAREST);
  gl.texParameteri(target, gl.TEXTURE_MAG_FILTER, smooth ? gl.LINEAR : gl.NEAREST);
  gl.texParameteri(target, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(target, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  return t;
}

/**
 * @param {number} width
 * @param {number} height
 * @param {number} count
 */
function icons(width, height, count) {
  const out = new Float32Array(count * 4);
  for (let i = 0; i < count; i++) out.set([width - 10 - (Math.floor(i / 4) + 1) * 90 + 7, 35 + (i % 4) * 100 + 3, 76, 94], i * 4);
  return out;
}

async function main() {
  const [width, height] = [640, 480];
  const meta = await (await fetch('assets/picture.json')).json();
  const picture = new Uint8Array(await (await fetch('assets/picture.rgb')).arrayBuffer());
  const analysis = new Uint8Array(await (await fetch('assets/analysis.bin')).arrayBuffer());
  const source = await (await fetch('../../scooch-core/shaders/frame.glsl')).text();
  const engine = await Remote.create(picture, meta.width, meta.height, analysis);
  await engine.screen(width, height);

  const glCanvas = new OffscreenCanvas(width, height);
  const gl = /** @type {WebGL2RenderingContext} */ (glCanvas.getContext('webgl2'));
  gl.getExtension('EXT_color_buffer_float');
  const program = /** @type {WebGLProgram} */ (gl.createProgram());
  gl.attachShader(program, shader(gl, gl.VERTEX_SHADER, VERTEX));
  gl.attachShader(program, shader(gl, gl.FRAGMENT_SHADER, source));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(String(gl.getProgramInfoLog(program)));
  gl.useProgram(program);
  SAMPLERS.forEach((name, unit) => gl.uniform1i(gl.getUniformLocation(program, name), unit));

  const rgba = new Uint8Array(meta.width * meta.height * 4);
  for (let i = 0; i < meta.width * meta.height; i++) rgba.set([picture[i * 3], picture[i * 3 + 1], picture[i * 3 + 2], 255], i * 4);
  const canvasTexture = texture(gl, gl.TEXTURE_2D, true);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.SRGB8_ALPHA8, meta.width, meta.height, 0, gl.RGBA, gl.UNSIGNED_BYTE, rgba);
  gl.generateMipmap(gl.TEXTURE_2D);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);

  const target = texture(gl, gl.TEXTURE_2D, false);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.SRGB8_ALPHA8, width, height, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
  const fbo = gl.createFramebuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, target, 0);
  gl.viewport(0, 0, width, height);

  /** @param {import('../pkg/engine.js').Tables} t */
  function draw(t) {
    const map = texture(gl, gl.TEXTURE_2D, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R32F, t.mapWidth, 2, 0, gl.RED, gl.FLOAT, t.map);
    const field = texture(gl, gl.TEXTURE_2D, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R16F, t.gridWidth, t.gridHeight, 0, gl.RED, gl.HALF_FLOAT, t.field);
    const look = texture(gl, gl.TEXTURE_2D, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, t.gridWidth, t.gridHeight, 0, gl.RGBA, gl.UNSIGNED_BYTE, t.look);
    const pad = texture(gl, gl.TEXTURE_2D, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.SRGB8_ALPHA8, t.gridWidth, t.gridHeight, 0, gl.RGBA, gl.UNSIGNED_BYTE, t.pad);
    const paints = texture(gl, gl.TEXTURE_2D_ARRAY, true);
    gl.texImage3D(gl.TEXTURE_2D_ARRAY, 0, gl.SRGB8_ALPHA8, t.paintWidth, t.paintHeight, 8, 0, gl.RGBA, gl.UNSIGNED_BYTE, t.paints);
    const ramps = texture(gl, gl.TEXTURE_2D, true);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, 512, 2, 0, gl.RGBA, gl.UNSIGNED_BYTE, t.ramps);
    const bound = [canvasTexture, map, map, field, field, look, look, pad, pad, paints, paints, ramps, ramps];
    bound.forEach((tex, unit) => {
      gl.activeTexture(gl.TEXTURE0 + unit);
      gl.bindTexture(unit === 9 || unit === 10 ? gl.TEXTURE_2D_ARRAY : gl.TEXTURE_2D, tex);
    });
    const c = t.constants;
    const u = (/** @type {string} */ name) => gl.getUniformLocation(program, name);
    gl.uniform2f(u('size'), c[0], c[1]);
    gl.uniform2f(u('grid'), c[2], c[3]);
    gl.uniform2f(u('paint_grid'), c[4], c[5]);
    gl.uniform1f(u('s'), c[6]);
    ['unit', 'ramp_low', 'ramp_high', 'reach', 'grain_amount', 'grain_scale', 'grain_floor', 'grain_lod', 'core_lift', 'core_drop', 'volume_lit', 'volume_dark'].forEach((name, i) => gl.uniform1f(u(name), c[7 + i]));
    gl.uniform2f(u('shadow_at'), c[20], c[21]);
    gl.uniform2f(u('rim_at'), c[22], c[23]);
    gl.uniform2f(u('belly_at'), c[24], c[25]);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    const pixels = new Uint8ClampedArray(width * height * 4);
    gl.readPixels(0, 0, width, height, gl.RGBA, gl.UNSIGNED_BYTE, pixels);
    [map, field, look, pad, paints, ramps].forEach((tex) => gl.deleteTexture(tex));
    const flipped = new Uint8ClampedArray(pixels.length);
    for (let y = 0; y < height; y++) flipped.set(pixels.subarray((height - 1 - y) * width * 4, (height - y) * width * 4), y * width * 4);
    return new ImageData(flipped, width, height);
  }

  const view = /** @type {HTMLCanvasElement} */ (document.getElementById('view'));
  const ctx = /** @type {CanvasRenderingContext2D} */ (view.getContext('2d'));
  /**
   * @param {ImageData} frame
   * @param {Float32Array} set
   * @param {number} held
   */
  function show(frame, set, held) {
    ctx.putImageData(frame, 0, 0);
    for (let i = 0; i < set.length / 4; i++) {
      ctx.fillStyle = i === held ? 'rgba(255,255,255,0.85)' : 'rgba(255,255,255,0.55)';
      ctx.fillRect(set[i * 4] + 12, set[i * 4 + 1] + 8, 52, 52);
    }
  }

  const set = icons(width, height, 12);
  let tables = await engine.drop(set);
  show(draw(tables), set, -1);
  let held = -1;
  /** @param {PointerEvent} e */
  const at = (e) => {
    const box = view.getBoundingClientRect();
    return [((e.clientX - box.left) * width) / box.width, ((e.clientY - box.top) * height) / box.height];
  };
  view.addEventListener('pointerdown', (e) => {
    const [x, y] = at(e);
    for (let i = 0; i < set.length / 4; i++) {
      if (x >= set[i * 4] && x < set[i * 4] + set[i * 4 + 2] && y >= set[i * 4 + 1] && y < set[i * 4 + 1] + set[i * 4 + 3]) held = i;
    }
  });
  view.addEventListener('pointermove', async (e) => {
    if (held < 0) return;
    const [x, y] = at(e);
    set[held * 4] = x - set[held * 4 + 2] / 2;
    set[held * 4 + 1] = y - set[held * 4 + 3] / 2;
    const answer = await engine.drag(set, held, 15);
    if (answer) show(draw(answer), set, held);
  });
  view.addEventListener('pointerup', async () => {
    if (held < 0) return;
    held = -1;
    show(draw(await engine.drop(set)), set, -1);
  });

  const frames = [await createImageBitmap(view)];
  const moving = 1;
  for (let step = 1; step <= 6; step++) {
    set[moving * 4] -= 55;
    set[moving * 4 + 1] += 22;
    const answer = await engine.drag(set, moving, 15);
    if (answer) tables = answer;
    show(draw(tables), set, moving);
    frames.push(await createImageBitmap(view));
  }
  tables = await engine.drop(set);
  show(draw(tables), set, -1);
  frames.push(await createImageBitmap(view));

  const kept = frames;
  const scale = 0.5;
  const strip = document.createElement('canvas');
  strip.width = Math.round(width * scale) * kept.length;
  strip.height = Math.round(height * scale);
  const s2 = /** @type {CanvasRenderingContext2D} */ (strip.getContext('2d'));
  kept.forEach((f, i) => s2.drawImage(/** @type {ImageBitmap} */ (f), i * Math.round(width * scale), 0, Math.round(width * scale), Math.round(height * scale)));
  /** @type {any} */ (window).strip = strip.toDataURL('image/png');
}

main().catch((error) => {
  /** @type {any} */ (window).strip = `error: ${error}`;
});
