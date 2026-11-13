import { Component, lazy, Suspense, type ReactNode } from 'react';
import { asset } from '@/lib/base';
import { Beats } from './Beats';

const Stage = lazy(() => import('@/scene/Stage'));

class Fallback extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch() {
    document.documentElement.classList.add('stills-mode');
  }

  render() {
    return this.state.failed ? null : this.props.children;
  }
}

type Props = {
  live: boolean;
  beats: React.RefObject<HTMLDivElement[]>;
  canvas: React.Ref<HTMLDivElement>;
  still: React.Ref<HTMLPictureElement>;
  section: React.Ref<HTMLElement>;
  onReady: () => void;
};

export function Story({ live, beats, canvas, still, section, onReady }: Props) {
  return (
    <section ref={section} id="story" className="story relative h-[700svh]" aria-label="Scooch in seven steps">
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
        <div aria-hidden className="stage-scrim pointer-events-none absolute inset-x-0 bottom-0 h-[46svh] bg-gradient-to-t from-room via-room/85 to-transparent wide:h-[16svh] wide:via-room/40" />
        <Beats refs={beats} />
      </div>
    </section>
  );
}
