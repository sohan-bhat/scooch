import { CanvasTexture, LinearFilter, LinearMipmapLinearFilter, SRGBColorSpace } from 'three';
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

export function paintedTexture(width: number, height: number, paint: Painter, flip = false) {
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  const texture = new CanvasTexture(canvas);
  texture.colorSpace = SRGBColorSpace;
  texture.flipY = flip;
  texture.anisotropy = 8;
  texture.magFilter = LinearFilter;
  texture.minFilter = LinearMipmapLinearFilter;
  let last = -1;
  const draw = (amount = 1, force = false) => {
    if (!context || (amount === last && !force)) return;
    last = amount;
    context.clearRect(0, 0, width, height);
    paint(context, width, height, amount);
    texture.needsUpdate = true;
  };
  draw(1);
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

export const typedPainter: Painter = (context, width, height, amount) => {
  const text = caption('fix');
  const shown = Math.round(text.length * Math.min(Math.max(amount, 0), 1));
  if (shown <= 0) return;
  const size = Math.round(width * 0.112);
  context.font = `600 ${size}px ${FONT.mono}`;
  context.textBaseline = 'alphabetic';
  context.textAlign = 'left';
  const lines = wrap(context, text, width * 0.84);
  let left = shown;
  lines.forEach((line, index) => {
    const part = line.slice(0, Math.max(left, 0));
    left -= line.length + 1;
    if (!part) return;
    context.fillStyle = 'rgba(28, 30, 44, 0.9)';
    context.fillText(part, width * 0.08, height * 0.2 + index * size * 1.62);
  });
};

export const stickyPainter: Painter = (context, width, height, amount) => {
  const text = caption('grow');
  context.fillStyle = '#f1cf5a';
  context.fillRect(0, 0, width, height);
  const shade = context.createLinearGradient(0, 0, 0, height);
  shade.addColorStop(0, 'rgba(255, 246, 210, 0.22)');
  shade.addColorStop(0.18, 'rgba(255, 246, 210, 0)');
  shade.addColorStop(1, 'rgba(120, 84, 20, 0.12)');
  context.fillStyle = shade;
  context.fillRect(0, 0, width, height);
  const size = Math.round(width * 0.158);
  context.font = `700 ${size}px ${FONT.text}`;
  context.textBaseline = 'alphabetic';
  context.textAlign = 'left';
  context.fillStyle = 'rgba(30, 26, 36, 0.92)';
  const lines = wrap(context, text, width * 0.82);
  const top = height * 0.5 - ((lines.length - 1) * size * 1.18) / 2 + size * 0.35;
  const total = lines.reduce((sum, line) => sum + context.measureText(line).width, 0);
  let left = total * Math.min(Math.max(amount, 0), 1);
  lines.forEach((line, index) => {
    const span = context.measureText(line).width;
    if (left <= 0) return;
    context.save();
    context.beginPath();
    context.rect(0, top + (index - 1) * size * 1.18 + size * 0.2, width * 0.09 + Math.min(left, span), size * 1.18);
    context.clip();
    context.fillText(line, width * 0.09, top + index * size * 1.18);
    context.restore();
    left -= span;
  });
};

export const dialPainter: Painter = (context, width, height) => {
  const text = caption('rest').toUpperCase();
  const size = Math.round(width * 0.075);
  context.font = `700 ${size}px ${FONT.mono}`;
  context.textBaseline = 'middle';
  context.textAlign = 'center';
  context.fillStyle = '#cfeec4';
  const lines = wrap(context, text, width * 0.62);
  lines.forEach((line, index) => context.fillText(line, width / 2, height * 0.66 + (index - (lines.length - 1) / 2) * size * 1.3));
};

export const wallPainter: Painter = (context, width, height) => {
  const text = caption('shapes');
  context.fillStyle = '#000';
  context.fillRect(0, 0, width, height);
  context.fillStyle = 'rgb(0, 255, 0)';
  context.fillRect(width * 0.18, height * 0.53, width * 0.64, height * 0.22);
  const size = Math.round(height * 0.07);
  context.font = `800 ${size}px ${FONT.display}`;
  context.textBaseline = 'middle';
  context.textAlign = 'center';
  context.fillStyle = 'rgb(255, 255, 0)';
  context.fillText(text, width / 2, height * 0.64);
};
