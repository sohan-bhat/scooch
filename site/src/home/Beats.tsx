import { Button } from '@/components/ui/button';
import content from '@/content.json';

export const captions = content.beats.filter((beat): beat is { id: string; caption: string } => 'caption' in beat && typeof beat.caption === 'string');

const overlay = 'absolute flex flex-col items-start gap-5 tall:inset-x-0 tall:bottom-[max(5svh,1.5rem)] tall:gap-4';

export function Hero({ live }: { live: boolean }) {
  const Heading = live ? 'h1' : 'p';
  const role = Heading === 'p' ? { role: 'heading', 'aria-level': 1 } : {};
  return (
    <>
      <Heading {...role} className="text-(length:--text-hero) text-paper">
        {content.story.title}
      </Heading>
      <Button asChild className="pointer-events-auto mt-2">
        <a href="#install">{content.story.action}</a>
      </Button>
    </>
  );
}

export function Closing() {
  return (
    <>
      <h2 className="text-(length:--text-beat) text-paper">{content.story.end}</h2>
      <Button asChild className="pointer-events-auto mt-2">
        <a href="#install">{content.story.action}</a>
      </Button>
    </>
  );
}

export function Beats({ refs }: { refs: React.RefObject<HTMLElement[]> }) {
  const keep = (index: number) => (element: HTMLElement | null) => {
    if (element) refs.current[index] = element;
  };
  return (
    <div className="beats page-gutter pointer-events-none absolute inset-0 z-10" data-pending="">
      <div className="relative h-full w-full">
        <div ref={keep(0)} data-overlay="hero" className={`${overlay} wide:top-[19vh] wide:left-0 wide:w-[min(40vw,36rem)]`}>
          <Hero live />
        </div>
        <ol className="sr-only">
          {captions.map((beat) => (
            <li key={beat.id}>{beat.caption}</li>
          ))}
        </ol>
        {captions.map((beat, index) => (
          <p
            key={beat.id}
            ref={keep(index + 2)}
            aria-hidden
            data-overlay={beat.id}
            className="phone-caption absolute inset-x-0 bottom-[max(6svh,1.75rem)] hidden text-(length:--text-support) leading-snug text-paper"
          >
            {beat.caption}
          </p>
        ))}
        <div ref={keep(1)} data-overlay="end" className={`${overlay} wide:top-1/2 wide:right-0 wide:w-[min(40vw,36rem)] wide:-translate-y-1/2`}>
          <Closing />
        </div>
        <p data-overlay="hint" aria-live="polite" className="hint absolute bottom-[max(4svh,1.25rem)] left-1/2 w-max -translate-x-1/2 text-(length:--text-support) text-plastic opacity-0" />
      </div>
    </div>
  );
}
