import { lazy, Suspense } from 'react';
import { Beats } from './Beats';

const Stage = lazy(() => import('@/scene/Stage'));

type Props = {
  live: boolean;
  beats: React.RefObject<HTMLDivElement[]>;
  canvas: React.Ref<HTMLDivElement>;
  section: React.Ref<HTMLElement>;
  onProgress: (value: number) => void;
  onReady: () => void;
};

export function Story({ live, beats, canvas, section, onProgress, onReady }: Props) {
  return (
    <section ref={section} id="story" className="story relative h-[700svh]" aria-label="Scooch in seven steps">
      <div className="stage sticky top-0 h-svh w-full overflow-hidden bg-room">
        <div
          aria-hidden
          className="stage-glow absolute inset-0 bg-[radial-gradient(50%_55%_at_70%_40%,rgb(229_89_26/0.13),transparent_70%),radial-gradient(90%_70%_at_50%_100%,rgb(31_26_21/0.9),transparent)] tall:bg-[radial-gradient(90%_40%_at_50%_30%,rgb(229_89_26/0.13),transparent_70%)]"
        />
        <div ref={canvas} className="absolute inset-0 opacity-0">
          {live ? (
            <Suspense fallback={null}>
              <Stage onProgress={onProgress} onReady={onReady} />
            </Suspense>
          ) : null}
        </div>
        <div aria-hidden className="pointer-events-none absolute inset-x-0 bottom-0 h-[46svh] bg-gradient-to-t from-room via-room/85 to-transparent wide:h-[16svh] wide:via-room/40" />
        <Beats refs={beats} />
      </div>
    </section>
  );
}
