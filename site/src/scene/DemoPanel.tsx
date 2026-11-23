import { useEffect, useRef, useSyncExternalStore } from 'react';
import { createPortal } from 'react-dom';
import { ToggleGroup } from 'radix-ui';
import content from '@/content.json';
import { Download } from '@/home/Download';
import { asset } from '@/lib/base';
import './demo.css';
import words from './demo.json';
import { PICTURES, choose, closeDemo, demo, drop, handles, move, nudge, pick, screenBox, subscribe, thumbOf } from './demo';
import { hooks } from './touch';

const names = ['Trip', 'Taxes', 'Notes', 'Resume', 'Shot', 'Recipes'];
const ARROWS: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

function snapshot() {
  return demo.version;
}

export default function DemoPanel() {
  useSyncExternalStore(subscribe, snapshot, snapshot);
  const layer = useRef<HTMLDivElement>(null);
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const shown = demo.open || demo.closing > 0;
  const count = hooks.manifest?.icons.length ?? 0;

  useEffect(() => {
    if (!shown) return;
    let frame = 0;
    const place = () => {
      const host = layer.current;
      const camera = demo.camera;
      if (host && camera) {
        const rect = host.getBoundingClientRect();
        const spots = handles(camera, rect.width, rect.height);
        spots.forEach((spot, i) => {
          const button = buttons.current[i];
          if (!button) return;
          const w = Math.max(44, spot.w);
          const h = Math.max(44, spot.h);
          button.style.transform = `translate(${(spot.x - w / 2).toFixed(1)}px, ${(spot.y - h / 2).toFixed(1)}px)`;
          button.style.width = `${w.toFixed(1)}px`;
          button.style.height = `${h.toFixed(1)}px`;
        });
      }
      frame = requestAnimationFrame(place);
    };
    frame = requestAnimationFrame(place);
    return () => cancelAnimationFrame(frame);
  }, [shown]);

  useEffect(() => {
    if (demo.open) buttons.current[0]?.focus({ preventScroll: true });
  }, [shown]);

  const host = document.querySelector('.stage');
  if (!host || !shown) return null;

  const rectOf = () => (layer.current as HTMLDivElement).getBoundingClientRect();
  const outside = (event: React.PointerEvent) => {
    const camera = demo.camera;
    if (!camera || event.target !== event.currentTarget) return;
    const rect = rectOf();
    const box = screenBox(camera, rect.width, rect.height);
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    if (!box || x < box.left || x > box.right || y < box.top || y > box.bottom) closeDemo();
  };

  return createPortal(
    <div ref={layer} className="demo-layer absolute inset-0 z-20" data-open={demo.open ? '' : undefined} role="group" aria-label={content.story.desk.demo} onPointerDown={outside}>
      {Array.from({ length: count }, (_, i) => (
        <button
          key={i}
          ref={(element) => {
            buttons.current[i] = element;
          }}
          type="button"
          aria-label={names[i] ?? `Icon ${i + 1}`}
          className="demo-icon absolute top-0 left-0 cursor-grab touch-none rounded-field focus-visible:outline-2 focus-visible:outline-pad active:cursor-grabbing"
          onPointerDown={(event) => {
            const camera = demo.camera;
            if (!camera) return;
            event.currentTarget.setPointerCapture(event.pointerId);
            pick(i, event.clientX, event.clientY, camera, rectOf());
          }}
          onPointerMove={(event) => {
            const camera = demo.camera;
            if (!camera || !event.currentTarget.hasPointerCapture(event.pointerId)) return;
            move(event.clientX, event.clientY, camera, rectOf());
          }}
          onPointerUp={(event) => {
            if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
            drop();
          }}
          onPointerCancel={() => drop()}
          onKeyDown={(event) => {
            const arrow = ARROWS[event.key];
            if (!arrow) return;
            event.preventDefault();
            nudge(i, arrow[0], arrow[1]);
          }}
        />
      ))}
      <div className="demo-bar page-gutter pointer-events-none absolute inset-x-0 bottom-[max(5svh,1.5rem)] flex flex-col items-center gap-4">
        {demo.used ? null : <p className="text-(length:--text-support) text-paper">{words.line}</p>}
        <div className="pointer-events-auto flex flex-col items-center gap-4 sm:flex-row sm:gap-6">
          <ToggleGroup.Root
            type="single"
            value={demo.picture}
            onValueChange={(value) => {
              if (value) choose(value);
            }}
            loop
            aria-label={words.pictures}
            className="flex items-center gap-2"
          >
            {PICTURES.map((id) => {
              const thumb = thumbOf(id);
              return (
                <ToggleGroup.Item
                  key={id}
                  value={id}
                  aria-label={words.names[id as keyof typeof words.names]}
                  className="demo-pick"
                >
                  {thumb ? <img src={asset(thumb)} alt="" draggable={false} decoding="async" /> : null}
                </ToggleGroup.Item>
              );
            })}
          </ToggleGroup.Root>
          <div className="flex items-center gap-6">
            <Download size="md" onPick={() => closeDemo()} />
            <button type="button" className="link-quiet cursor-pointer text-[0.9375rem] font-medium" onClick={() => closeDemo()}>
              {words.back}
            </button>
          </div>
        </div>
      </div>
    </div>,
    host,
  );
}
