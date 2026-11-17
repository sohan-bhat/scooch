import { LinearFilter, NearestFilter, NoColorSpace, type Texture, type WebGLRenderer } from 'three';
import { quiet } from '@/lib/idle';

export type Pixels = { data: Uint8Array | Uint8ClampedArray; width: number; height: number };

const STRIP = 1 << 19;

type Source = HTMLCanvasElement | ImageBitmap | Pixels;

const AHEAD = 3;
const fences = new WeakMap<WebGL2RenderingContext, WebGLSync[]>();
const nextTask = () => new Promise<void>((resolve) => window.setTimeout(resolve, 0));

export async function caughtUp(context: WebGL2RenderingContext) {
  if (context.isContextLost()) return;
  const queue = fences.get(context) ?? [];
  fences.set(context, queue);
  const sync = context.fenceSync(context.SYNC_GPU_COMMANDS_COMPLETE, 0);
  if (sync) {
    context.flush();
    queue.push(sync);
  }
  for (;;) {
    if (context.isContextLost()) {
      queue.length = 0;
      return;
    }
    while (queue.length > 0 && context.getSyncParameter(queue[0], context.SYNC_STATUS) === context.SIGNALED) context.deleteSync(queue.shift()!);
    if (queue.length < AHEAD) return;
    await nextTask();
  }
}

const probes = new WeakMap<WebGL2RenderingContext, WebGLSync>();

export function gpuQuiet(context: WebGL2RenderingContext) {
  if (context.isContextLost()) return false;
  const probe = probes.get(context);
  if (!probe) {
    const sync = context.fenceSync(context.SYNC_GPU_COMMANDS_COMPLETE, 0);
    if (!sync) return true;
    context.flush();
    probes.set(context, sync);
    return false;
  }
  if (context.getSyncParameter(probe, context.SYNC_STATUS) !== context.SIGNALED) return false;
  context.deleteSync(probe);
  probes.delete(context);
  return true;
}

export function rendererCaughtUp(gl: WebGLRenderer) {
  return caughtUp(gl.getContext() as WebGL2RenderingContext);
}

export async function stripUpload(gl: WebGLRenderer, texture: Texture, source: Source, wait: () => Promise<void> = quiet) {
  if (!('data' in source) && !(source instanceof ImageBitmap)) {
    texture.needsUpdate = true;
    await wait();
    await rendererCaughtUp(gl);
    if ((gl.getContext() as WebGL2RenderingContext).isContextLost()) return false;
    gl.initTexture(texture);
    return true;
  }
  texture.source.dataReady = false;
  texture.mipmapsAutoUpdate = false;
  texture.needsUpdate = true;
  try {
    return await strips(gl, texture, source, wait);
  } finally {
    texture.source.dataReady = true;
    texture.mipmapsAutoUpdate = true;
  }
}

async function strips(gl: WebGLRenderer, texture: Texture, source: Source, wait: () => Promise<void>) {
  const { width, height } = source;
  await wait();
  const context = gl.getContext() as WebGL2RenderingContext;
  if (context.isContextLost()) return false;
  gl.initTexture(texture);
  const handle = (gl.properties.get(texture) as { __webglTexture?: WebGLTexture }).__webglTexture;
  if (!handle) return false;
  const alive = () => !context.isContextLost() && (gl.properties.get(texture) as { __webglTexture?: WebGLTexture }).__webglTexture === handle;
  const rows = Math.max(16, Math.floor(STRIP / (width * 4)));
  const image = source instanceof ImageBitmap ? source : null;
  for (let y = 0; y < height; y += rows) {
    const count = Math.min(rows, height - y);
    const part = image ? await createImageBitmap(image, 0, y, width, count, { imageOrientation: texture.flipY ? 'flipY' : 'from-image' }) : null;
    await wait();
    await caughtUp(context);
    if (!alive()) {
      part?.close();
      return false;
    }
    gl.state.bindTexture(context.TEXTURE_2D, handle);
    gl.state.pixelStorei(context.UNPACK_FLIP_Y_WEBGL, texture.flipY);
    gl.state.pixelStorei(context.UNPACK_PREMULTIPLY_ALPHA_WEBGL, texture.premultiplyAlpha);
    gl.state.pixelStorei(context.UNPACK_COLORSPACE_CONVERSION_WEBGL, texture.colorSpace === NoColorSpace ? context.NONE : context.BROWSER_DEFAULT_WEBGL);
    gl.state.pixelStorei(context.UNPACK_ALIGNMENT, 4);
    const at = texture.flipY ? height - y - count : y;
    if (part) {
      context.texSubImage2D(context.TEXTURE_2D, 0, 0, at, width, count, context.RGBA, context.UNSIGNED_BYTE, part);
      part.close();
    } else {
      const { data } = source as Pixels;
      context.texSubImage2D(context.TEXTURE_2D, 0, 0, at, width, count, context.RGBA, context.UNSIGNED_BYTE, data.subarray(y * width * 4, (y + count) * width * 4));
    }
  }
  await wait();
  if (!alive()) return false;
  if (texture.generateMipmaps && texture.minFilter !== LinearFilter && texture.minFilter !== NearestFilter) {
    gl.state.bindTexture(context.TEXTURE_2D, handle);
    context.generateMipmap(context.TEXTURE_2D);
  }
  return true;
}
