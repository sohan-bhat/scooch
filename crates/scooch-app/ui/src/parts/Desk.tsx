import type { Icon } from '@phosphor-icons/react';
import { useEffect, useRef, useState } from 'react';
import type { Monitor, Rect, System } from '~/backend/types';
import { copy } from '~/lib/copy';
import { useSize } from '~/lib/useSize';

const BARS: Record<System, { edge: 'top' | 'bottom'; points: number; everywhere: boolean }> = {
  macos: { edge: 'top', points: 25, everywhere: true },
  windows: { edge: 'bottom', points: 48, everywhere: true },
  linux: { edge: 'top', points: 32, everywhere: false },
};

const SEAM = 3;

function bounds(monitors: Monitor[]) {
  const left = Math.min(...monitors.map((monitor) => monitor.frame.x));
  const top = Math.min(...monitors.map((monitor) => monitor.frame.y));
  const right = Math.max(...monitors.map((monitor) => monitor.frame.x + monitor.frame.w));
  const bottom = Math.max(...monitors.map((monitor) => monitor.frame.y + monitor.frame.h));
  return { left, top, width: right - left, height: bottom - top };
}

function Picture({ src }: { src: string }) {
  const [layers, setLayers] = useState<string[]>([src]);
  const last = useRef(src);
  useEffect(() => {
    if (src === last.current) return;
    last.current = src;
    setLayers((current) => [current[current.length - 1], src]);
  }, [src]);
  return layers.map((layer, index) => (
    <img
      key={layer}
      src={layer}
      alt=""
      draggable={false}
      onAnimationEnd={() => setLayers((current) => (current.length > 1 ? [current[current.length - 1]] : current))}
      className={`absolute inset-0 size-full object-cover ${index > 0 ? 'picture-in' : ''}`}
    />
  ));
}

function IconMark({ rect, sx, sy }: { rect: Rect; sx: number; sy: number }) {
  const width = rect.w * sx;
  const height = rect.h * sy;
  const tile = Math.max(3, Math.min(width * 0.7, height * 0.62));
  const label = Math.max(1.5, height * 0.09);
  return (
    <div className="icon-mark absolute top-0 left-0 flex flex-col items-center" style={{ width, height, transform: `translate(${rect.x * sx}px, ${rect.y * sy}px)`, gap: height * 0.08, paddingTop: height * 0.04 }}>
      <span className="block bg-paper/88 shadow-[0_1px_2px_rgb(20_17_14/0.4),inset_0_0_0_0.5px_rgb(255_255_255/0.6),0_0_0_0.5px_rgb(20_17_14/0.25)]" style={{ width: tile, height: tile, borderRadius: tile * 0.24 }} />
      <span className="block rounded-pill bg-paper/75 shadow-[0_0_0_0.5px_rgb(20_17_14/0.25)]" style={{ width: width * 0.74, height: label }} />
    </div>
  );
}

export function Desk({ monitors, system, empty }: { monitors: Monitor[]; system: System; empty?: Icon }) {
  const Empty = empty;
  const [ref, size] = useSize<HTMLDivElement>();
  const box = monitors.length > 0 ? bounds(monitors) : { left: 0, top: 0, width: 16, height: 10 };
  const scale = size.width > 0 ? Math.min(size.width / box.width, size.height / box.height) : 0;
  const offsetX = (size.width - box.width * scale) / 2;
  const offsetY = (size.height - box.height * scale) / 2;
  const seam = monitors.length > 1 ? SEAM : 0;
  const bar = BARS[system];
  return (
    <figure ref={ref} aria-label={copy.desk} className="relative m-0 size-full">
      {scale > 0 && monitors.length === 0 && (
        <div className="absolute rounded-[10px] bg-room-2 shadow-[inset_0_0_0_1px_rgb(243_236_220/0.08)]" style={{ left: offsetX, top: offsetY, width: box.width * scale, height: box.height * scale }} />
      )}
      {scale > 0 &&
        monitors.map((monitor, index) => {
          const left = offsetX + (monitor.frame.x - box.left) * scale + seam;
          const top = offsetY + (monitor.frame.y - box.top) * scale + seam;
          const width = monitor.frame.w * scale - seam * 2;
          const height = monitor.frame.h * scale - seam * 2;
          const showBar = bar.everywhere || index === 0;
          const barHeight = Math.max(2, bar.points * scale);
          const places = monitor.places;
          return (
            <div
              key={monitor.id}
              data-screen
              className="absolute overflow-hidden rounded-[8px] bg-room-3 shadow-[0_0_0_1px_rgb(243_236_220/0.12),0_28px_56px_-28px_rgb(0_0_0/0.95),0_10px_22px_-14px_rgb(0_0_0/0.7)]"
              style={{ left, top, width, height }}
            >
              {monitor.picture && <Picture src={monitor.picture} />}
              {!monitor.picture && Empty && (
                <div aria-hidden className="absolute inset-0 grid place-items-center text-paper/22">
                  <Empty weight="duotone" size={Math.min(width, height) * 0.22} />
                </div>
              )}
              {showBar && (
                <div
                  aria-hidden
                  data-depiction
                  className={system === 'macos' ? 'absolute inset-x-0 bg-paper/25 backdrop-blur-sm' : 'absolute inset-x-0 bg-ink/70'}
                  style={{ [bar.edge]: 0, height: barHeight }}
                />
              )}
              {places && places.width > 0 && places.height > 0 && (
                <div aria-hidden data-depiction className="absolute inset-0">
                  {places.icons.map((icon, iconIndex) => (
                    <IconMark key={iconIndex} rect={icon} sx={width / places.width} sy={height / places.height} />
                  ))}
                </div>
              )}
            </div>
          );
        })}
    </figure>
  );
}
