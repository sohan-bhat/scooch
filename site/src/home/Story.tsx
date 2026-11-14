import { Component, lazy, Suspense, type ReactNode } from 'react';
import content from '@/content.json';
import { asset } from '@/lib/base';
import { release } from '@/lib/load';
import { Beats } from './Beats';
import juice from './juice.json';

const frames = ['desktop', 'square', 'phone'] as const;

function Juice() {
  return (
    <div id="glass" className="stage-glass pointer-events-none absolute inset-0" role="progressbar" aria-label={content.story.loading} aria-valuemin={0} aria-valuemax={100} aria-valuenow={0}>
      {frames.map((name) => {
        const shape = juice[name];
        return (
          <svg key={name} data-frame={name} viewBox={`0 0 ${shape.width} ${shape.height}`} aria-hidden="true" focusable="false">
            <defs>
              <clipPath id={`juice-${name}`}>
                <path d={shape.hull} />
              </clipPath>
              <linearGradient id={`pour-${name}`} gradientUnits="userSpaceOnUse" x1={shape.left} x2={shape.right} y1="0" y2="0">
                {shape.stops.map((color, index) => (
                  <stop key={color + index} offset={0.1 + index * 0.2} stopColor={color} />
                ))}
              </linearGradient>
            </defs>
            <g clipPath={`url(#juice-${name})`}>
              <g className="juice-level" style={{ '--drop': shape.drop } as React.CSSProperties}>
                <rect x={shape.left} y={shape.top.cy} width={shape.right - shape.left} height={shape.bottom - shape.top.cy + shape.drop + 2} fill={`url(#pour-${name})`} />
                <ellipse cx={shape.top.cx} cy={shape.top.cy} rx={shape.top.rx} ry={shape.top.ry} fill={shape.surface} />
              </g>
            </g>
          </svg>
        );
      })}
    </div>
  );
}

const Stage = lazy(() => import('@/scene/Stage'));

class Fallback extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch() {
    document.documentElement.classList.add('stills-mode');
    release(false);
  }

  render() {
    return this.state.failed ? null : this.props.children;
  }
}

type Props = {
  live: boolean;
  beats: React.RefObject<HTMLElement[]>;
  canvas: React.Ref<HTMLDivElement>;
  still: React.Ref<HTMLPictureElement>;
  section: React.Ref<HTMLElement>;
  onReady: () => void;
};

export function Story({ live, beats, canvas, still, section, onReady }: Props) {
  return (
    <section ref={section} id="story" className="story relative h-[1400svh]" aria-label={content.story.label}>
      <div className="stage sticky top-0 h-svh w-full overflow-hidden bg-room">
        <div ref={canvas} className="absolute inset-0 opacity-0">
          {live ? (
            <Fallback>
              <Suspense fallback={null}>
                <Stage onReady={onReady} />
              </Suspense>
            </Fallback>
          ) : null}
        </div>
        <picture ref={still} className="stage-still pointer-events-none absolute inset-0" aria-hidden>
          <source media="(max-width: 767px), (orientation: portrait)" srcSet={asset('stills/hero-phone.webp')} width={1440} height={1694} />
          <source media="(max-aspect-ratio: 16/10)" srcSet={asset('stills/hero-square.webp')} width={1920} height={1917} />
          <img src={asset('stills/hero-desktop.webp')} alt="" width={3456} height={1350} fetchPriority="high" decoding="sync" />
        </picture>
        <Juice />
        <div aria-hidden className="stage-scrim pointer-events-none absolute inset-x-0 bottom-0 h-[46svh] bg-gradient-to-t from-room via-room/85 to-transparent wide:h-[16svh] wide:via-room/40" />
        <Beats refs={beats} />
        <div id="desk-controls" />
      </div>
    </section>
  );
}
