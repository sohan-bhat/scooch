import { quiet } from '@/lib/idle';
import { CanvasTexture, LinearFilter, LinearMipmapLinearFilter, NearestFilter, NoColorSpace, SRGBColorSpace, Vector4, type Matrix4 } from 'three';
import content from '@/content.json';

export const FONT = {
  display: '"Anybody Variable", "Arial Black", sans-serif',
  text: '"Instrument Sans Variable", Arial, sans-serif',
  mono: '"Martian Mono Variable", monospace',
};

const words = Object.fromEntries(content.beats.flatMap((beat) => ('caption' in beat && beat.caption ? [[beat.id, beat.caption]] : []))) as Record<string, string>;

export function caption(id: string) {
  return words[id] ?? '';
}

export function fontsReady() {
  return Promise.all([
    document.fonts.load(`700 64px ${FONT.mono}`),
    document.fonts.load(`600 64px ${FONT.text}`),
    document.fonts.load(`800 64px ${FONT.display}`),
  ]).catch(() => undefined);
}

export type Painter = (context: CanvasRenderingContext2D, width: number, height: number, amount: number) => void;

export function canvasTexture(width: number, height: number, flip = false, color = true) {
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  const texture = new CanvasTexture(canvas);
  texture.flipY = flip;
  if (color) {
    texture.colorSpace = SRGBColorSpace;
    texture.anisotropy = 8;
    texture.magFilter = LinearFilter;
    texture.minFilter = LinearMipmapLinearFilter;
  } else {
    texture.colorSpace = NoColorSpace;
    texture.magFilter = NearestFilter;
    texture.minFilter = NearestFilter;
    texture.generateMipmaps = false;
  }
  return { canvas, context, texture };
}

export function paintedTexture(width: number, height: number, paint: Painter, flip = false, now = true) {
  const { context, texture } = canvasTexture(width, height, flip);
  let last = -1;
  const draw = (amount = 1, force = false) => {
    if (!context || (amount === last && !force)) return;
    last = amount;
    context.clearRect(0, 0, width, height);
    paint(context, width, height, amount);
    texture.needsUpdate = true;
  };
  if (now) draw(1);
  return { texture, draw };
}

export function wrap(context: CanvasRenderingContext2D, text: string, width: number) {
  const lines: string[] = [];
  let line = '';
  for (const word of text.split(' ')) {
    const next = line ? `${line} ${word}` : word;
    if (line && context.measureText(next).width > width) {
      lines.push(line);
      line = word;
    } else line = next;
  }
  if (line) lines.push(line);
  return lines;
}

export const osdPainter: Painter = (context, width, height, amount) => {
  if (amount <= 0) return;
  const text = caption('problem').toUpperCase();
  const size = Math.round(height * 0.056);
  context.font = `700 ${size}px ${FONT.mono}`;
  context.textBaseline = 'alphabetic';
  context.textAlign = 'center';
  const lines = wrap(context, text, width * 0.86);
  const lead = size * 1.45;
  const bottom = height * 0.9;
  lines.forEach((line, index) => {
    const y = bottom - (lines.length - 1 - index) * lead;
    context.lineJoin = 'round';
    context.lineWidth = size * 0.28;
    context.strokeStyle = 'rgba(4, 10, 6, 0.92)';
    context.strokeText(line, width / 2, y);
    context.fillStyle = '#e8fbe2';
    context.fillText(line, width / 2, y);
  });
};

export type Reveal = { full: Painter; order: Painter; blank?: Painter };

const gray = (value: number) => {
  const level = Math.round(Math.min(Math.max(value, 0), 1) * 255);
  return `rgb(${level}, ${level}, ${level})`;
};

function typedLayout(context: CanvasRenderingContext2D, width: number) {
  const text = caption('fix');
  const size = Math.round(width * 0.112);
  context.font = `600 ${size}px ${FONT.mono}`;
  context.textBaseline = 'alphabetic';
  context.textAlign = 'left';
  return { text, size, lines: wrap(context, text, width * 0.84) };
}

export const typed: Reveal = {
  full: (context, width, height) => {
    const { size, lines } = typedLayout(context, width);
    context.fillStyle = 'rgba(28, 30, 44, 0.9)';
    lines.forEach((line, index) => context.fillText(line, width * 0.08, height * 0.2 + index * size * 1.62));
  },
  order: (context, width, height) => {
    const { text, size, lines } = typedLayout(context, width);
    context.fillStyle = gray(1);
    context.fillRect(0, 0, width, height);
    let start = 0;
    lines.forEach((line, index) => {
      const base = height * 0.2 + index * size * 1.62;
      for (let k = 0; k < line.length; k++) {
        const from = Math.floor(width * 0.08 + context.measureText(line.slice(0, k)).width);
        const to = Math.ceil(width * 0.08 + context.measureText(line.slice(0, k + 1)).width);
        context.fillStyle = gray((start + k + 0.5) / text.length);
        context.fillRect(from, base - size, to - from, size * 1.4);
      }
      start += line.length + 1;
    });
  },
};

function stickyBack(context: CanvasRenderingContext2D, width: number, height: number) {
  context.fillStyle = '#f1cf5a';
  context.fillRect(0, 0, width, height);
  const shade = context.createLinearGradient(0, 0, 0, height);
  shade.addColorStop(0, 'rgba(255, 246, 210, 0.22)');
  shade.addColorStop(0.18, 'rgba(255, 246, 210, 0)');
  shade.addColorStop(1, 'rgba(120, 84, 20, 0.12)');
  context.fillStyle = shade;
  context.fillRect(0, 0, width, height);
}

function stickyLayout(context: CanvasRenderingContext2D, width: number, height: number) {
  const size = Math.round(width * 0.158);
  context.font = `700 ${size}px ${FONT.text}`;
  context.textBaseline = 'alphabetic';
  context.textAlign = 'left';
  const lines = wrap(context, caption('grow'), width * 0.82);
  const top = height * 0.5 - ((lines.length - 1) * size * 1.18) / 2 + size * 0.35;
  const spans = lines.map((line) => context.measureText(line).width);
  return { size, lines, top, spans, total: spans.reduce((sum, span) => sum + span, 0) };
}

export const sticky: Reveal = {
  blank: stickyBack,
  full: (context, width, height) => {
    stickyBack(context, width, height);
    const { size, lines, top } = stickyLayout(context, width, height);
    context.fillStyle = 'rgba(30, 26, 36, 0.92)';
    lines.forEach((line, index) => context.fillText(line, width * 0.09, top + index * size * 1.18));
  },
  order: (context, width, height) => {
    const { size, lines, top, spans, total } = stickyLayout(context, width, height);
    context.fillStyle = gray(1);
    context.fillRect(0, 0, width, height);
    let before = 0;
    lines.forEach((_, index) => {
      const base = top + index * size * 1.18;
      const left = width * 0.09;
      const ramp = context.createLinearGradient(left, 0, left + spans[index], 0);
      ramp.addColorStop(0, gray(before / total));
      ramp.addColorStop(1, gray((before + spans[index]) / total));
      context.fillStyle = ramp;
      context.fillRect(0, base - size * 0.94, width, size * 1.18);
      before += spans[index];
    });
  },
};

export const dialPainter: Painter = (context, width, height) => {
  const text = caption('rest').toUpperCase();
  const size = Math.round(width * 0.068);
  context.font = `600 ${size}px ${FONT.text}`;
  context.textBaseline = 'middle';
  context.textAlign = 'center';
  context.fillStyle = '#231f1a';
  (context as CanvasRenderingContext2D & { letterSpacing?: string }).letterSpacing = `${Math.round(size * 0.14)}px`;
  context.fillText(text, width / 2, height * 0.635);
};

export const WALL = { left: -0.45, right: 0.75, bottom: 1.84, top: 2.08, z: -0.975 };

export function wallTexture(projector: Matrix4) {
  const size = 1024;
  const scale = 1400;
  const width = Math.round((WALL.right - WALL.left) * scale);
  const height = Math.round((WALL.top - WALL.bottom) * scale);
  const text = document.createElement('canvas');
  text.width = width;
  text.height = height;
  const draw = text.getContext('2d', { willReadFrequently: true });
  const out = document.createElement('canvas');
  out.width = size;
  out.height = size;
  const target = out.getContext('2d');
  const texture = new CanvasTexture(out);
  texture.colorSpace = NoColorSpace;
  texture.flipY = false;
  texture.magFilter = LinearFilter;
  texture.minFilter = LinearFilter;
  texture.generateMipmaps = false;
  const inverse = projector.clone().invert();
  const near = new Vector4();
  const far = new Vector4();
  const paint = async () => {
    if (!draw || !target) return;
    draw.fillStyle = '#000';
    draw.fillRect(0, 0, width, height);
    const font = Math.round(height * 0.36);
    draw.font = `800 ${font}px ${FONT.display}`;
    draw.textAlign = 'center';
    draw.textBaseline = 'middle';
    draw.fillStyle = '#fff';
    draw.fillText(caption('shapes'), width / 2, height * 0.54, width * 0.92);
    const letters = draw.getImageData(0, 0, width, height).data;
    const image = target.createImageData(size, size);
    const corners = [
      [WALL.left, WALL.bottom],
      [WALL.right, WALL.bottom],
      [WALL.left, WALL.top],
      [WALL.right, WALL.top],
    ].map(([x, y]) => {
      const point = new Vector4(x, y, WALL.z, 1).applyMatrix4(projector);
      return [point.x / point.w, point.y / point.w];
    });
    const u0 = Math.max(0, Math.floor(Math.min(...corners.map((c) => c[0])) * size) - 8);
    const u1 = Math.min(size, Math.ceil(Math.max(...corners.map((c) => c[0])) * size) + 8);
    const v0 = Math.max(0, Math.floor(Math.min(...corners.map((c) => c[1])) * size) - 8);
    const v1 = Math.min(size, Math.ceil(Math.max(...corners.map((c) => c[1])) * size) + 8);
    for (let v = v0; v < v1; v++) {
      if ((v - v0) % 16 === 0) await quiet();
      for (let u = u0; u < u1; u++) {
        const su = (u + 0.5) / size;
        const sv = (v + 0.5) / size;
        near.set(su, sv, 0.1, 1).applyMatrix4(inverse);
        far.set(su, sv, 0.9, 1).applyMatrix4(inverse);
        const ax = near.x / near.w, ay = near.y / near.w, az = near.z / near.w;
        const bx = far.x / far.w, by = far.y / far.w, bz = far.z / far.w;
        const t = (WALL.z - az) / (bz - az);
        const x = ax + (bx - ax) * t;
        const y = ay + (by - ay) * t;
        const tx = (x - WALL.left) / (WALL.right - WALL.left);
        const ty = (WALL.top - y) / (WALL.top - WALL.bottom);
        const edge = Math.min(tx, 1 - tx, ty, 1 - ty);
        if (edge <= -0.02) continue;
        const region = Math.min(Math.max((edge + 0.02) / 0.06, 0), 1);
        let letter = 0;
        if (edge > 0) {
          const px = Math.min(width - 1, Math.max(0, Math.floor(tx * width)));
          const py = Math.min(height - 1, Math.max(0, Math.floor(ty * height)));
          letter = letters[(py * width + px) * 4];
        }
        const at = (v * size + u) * 4;
        image.data[at] = letter;
        image.data[at + 1] = Math.round(region * 255);
        image.data[at + 3] = 255;
      }
    }
    target.putImageData(image, 0, 0);
    texture.needsUpdate = true;
  };
  return { texture, paint };
}
