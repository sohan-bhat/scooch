import { Button } from '@/components/ui/button';
import content from '@/content.json';
import { cn } from '@/lib/utils';

const place: Record<string, string> = {
  hero: 'wide:left-0 wide:top-[47%] wide:-translate-y-1/2 wide:w-[min(45vw,40rem)]',
  problem: 'wide:left-0 wide:top-[24vh] wide:w-[min(34vw,32rem)]',
  sees: 'wide:left-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(31vw,30rem)]',
  fix: 'wide:left-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(31vw,30rem)]',
  follow: 'wide:left-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(31vw,30rem)]',
  rest: 'wide:right-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(36vw,32rem)]',
  end: 'wide:right-0 wide:top-1/2 wide:-translate-y-1/2 wide:w-[min(40vw,36rem)]',
};

export function Beats({ refs }: { refs: React.RefObject<HTMLDivElement[]> }) {
  return (
    <div className="beats page-gutter pointer-events-none absolute inset-0 z-10" data-pending="">
      <div className="relative h-full w-full">
        {content.beats.map((beat, index) => {
          const Heading = index === 0 ? 'h1' : 'h2';
          return (
            <div
              key={beat.id}
              ref={(element) => {
                if (element) refs.current[index] = element;
              }}
              data-beat={beat.id}
              className={cn(
                'absolute flex flex-col items-start gap-5 tall:inset-x-0 tall:bottom-[max(5svh,1.5rem)] tall:gap-4',
                place[beat.id],
              )}
            >
              <Heading
                data-split
                className={cn(
                  'text-paper',
                  index === 0 ? 'text-(length:--text-hero)' : 'text-(length:--text-beat)',
                )}
              >
                {beat.title}
              </Heading>
              {'support' in beat && beat.support ? (
                <p data-split className="max-w-[30ch] text-(length:--text-support) leading-snug text-plastic">
                  {beat.support}
                </p>
              ) : null}
              {'action' in beat && beat.action ? (
                <Button asChild className="pointer-events-auto mt-2">
                  <a data-action href="#install">
                    {beat.action}
                  </a>
                </Button>
              ) : null}
            </div>
          );
        })}
      </div>
    </div>
  );
}
