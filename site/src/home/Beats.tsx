import { Button } from '@/components/ui/button';
import content from '@/content.json';
import { cn } from '@/lib/utils';

export const place: Record<string, string> = {
  hero: 'wide:left-0 wide:top-[47%] wide:-translate-y-1/2 wide:w-[min(45vw,40rem)]',
  problem: 'wide:right-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(32vw,30rem)]',
  sees: 'wide:left-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(31vw,30rem)]',
  fix: 'wide:left-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(31vw,30rem)]',
  follow: 'wide:left-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(31vw,30rem)]',
  rest: 'wide:right-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(36vw,32rem)]',
  end: 'wide:right-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(40vw,36rem)]',
};

type Beat = (typeof content.beats)[number];

export function BeatCopy({ beat, index, live }: { beat: Beat; index: number; live: boolean }) {
  const Heading = index > 0 ? 'h2' : live ? 'h1' : 'p';
  const role = Heading === 'p' ? { role: 'heading', 'aria-level': 1 } : {};
  const split = live ? { 'data-split': '' } : {};
  return (
    <>
      <Heading {...split} {...role} className={cn('text-paper', index === 0 ? 'text-(length:--text-hero)' : 'text-(length:--text-beat)')}>
        {beat.title}
      </Heading>
      {'support' in beat && beat.support ? (
        <p {...split} className="max-w-[30ch] text-(length:--text-support) leading-snug text-plastic">
          {beat.support}
        </p>
      ) : null}
      {'action' in beat && beat.action ? (
        <Button asChild className="pointer-events-auto mt-2">
          <a {...(live ? { 'data-action': '' } : {})} href="#install">
            {beat.action}
          </a>
        </Button>
      ) : null}
    </>
  );
}

export const beatFrame = 'absolute flex flex-col items-start gap-5 tall:inset-x-0 tall:bottom-[max(5svh,1.5rem)] tall:gap-4';

export function Beats({ refs }: { refs: React.RefObject<HTMLDivElement[]> }) {
  return (
    <div className="beats page-gutter pointer-events-none absolute inset-0 z-10" data-pending="">
      <div className="relative h-full w-full">
        {content.beats.map((beat, index) => (
          <div
            key={beat.id}
            ref={(element) => {
              if (element) refs.current[index] = element;
            }}
            data-beat={beat.id}
            className={cn(beatFrame, place[beat.id])}
          >
            <BeatCopy beat={beat} index={index} live />
          </div>
        ))}
      </div>
    </div>
  );
}
